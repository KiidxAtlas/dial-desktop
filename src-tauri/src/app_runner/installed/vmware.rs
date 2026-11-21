//! VMware Fusion and Workstation console for one locally registered VM.
//!
//! The console is the VM's own loopback VNC (RFB) endpoint, enabled by the user in the VM's remote
//! display settings. Dial never edits the `.vmx`: it reads the VNC port from it, starts the VM with
//! `vmrun ... nogui` only when it is not already running, and connects only after [`binding`] has
//! proved that the process listening on that port is this VM's `vmware-vmx`, bound to loopback.
//! Disconnecting never stops the VM. Input goes to the guest through RFB; `vmrun` is never used
//! for input.

mod binding;
mod keys;
#[cfg(test)]
mod tests;
mod vmrun;
pub(super) use vmrun::kind_for;
mod vmx;

use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use super::inventory::Machine;
use crate::app_runner::input::{Frame, Gesture, Low, Modifier, qcode_for};
use crate::app_runner::rfb;

/// How long a VM that Dial started may take to open its VNC port.
const START_WAIT: Duration = Duration::from_secs(90);
/// How long an already running VM may take to open its VNC port before the console gives up.
const RUNNING_WAIT: Duration = Duration::from_secs(10);
/// Pause between binding checks while the VM is starting.
const BINDING_POLL: Duration = Duration::from_millis(500);
/// Longest wait for the first full screen.
const FIRST_FRAME_WAIT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Most modifiers one key press may carry; more than four is refused.
const MAX_MODIFIERS: usize = 4;

/// Timing of the start-up wait; tests shorten it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Waits {
    pub(super) running: Duration,
    pub(super) started: Duration,
    pub(super) poll: Duration,
}

impl Waits {
    const DEFAULT: Self = Self {
        running: RUNNING_WAIT,
        started: START_WAIT,
        poll: BINDING_POLL,
    };
}

/// A connected VMware console. Dropping it closes the connection and leaves the VM running.
pub(in crate::app_runner) struct Console {
    viewer: rfb::Viewer,
}

impl Console {
    /// Connect to the console of one inventoried VM. Refuses unless the VM's `.vmx` enables VNC,
    /// the VNC port is bound to loopback, and the listener belongs to that VM's process.
    pub(super) fn open(vm: &Machine) -> Result<Self, String> {
        let mut tool = vmrun::Tool::new(&vm.tool)?;
        Self::open_with(&vm.locator, &mut tool, &binding::Lsof, Waits::DEFAULT)
    }

    fn open_with(
        locator: &str,
        runner: &mut dyn vmrun::Runner,
        probe: &dyn binding::Probe,
        waits: Waits,
    ) -> Result<Self, String> {
        // Every refusal here happens before the VM is started or any socket is opened.
        let (vmx_path, settings) = vmx::load(Path::new(locator))?;
        probe.supported()?;
        let vmx = vmx_path
            .to_str()
            .ok_or_else(|| "the VM path is not valid UTF-8".to_owned())?;
        let running = runner
            .running()?
            .iter()
            .any(|listed| vmx::same_file(listed, &vmx_path));
        if !running {
            if binding::verify(probe, settings.port, vmx)? != binding::Proof::NotListening {
                return Err(
                    "The console port is already in use; refresh the VM list before starting."
                        .into(),
                );
            }
            runner.start_nogui(vmx)?;
        }
        let wait = if running {
            waits.running
        } else {
            waits.started
        };
        let owner = binding::wait_for_binding(probe, settings.port, vmx, wait, waits.poll)?;
        let stream = TcpStream::connect_timeout(
            &SocketAddr::from((settings.ip, settings.port)),
            CONNECT_TIMEOUT,
        )
        .map_err(|_| {
            format!(
                "could not connect to the VM's console on loopback port {}",
                settings.port
            )
        })?;
        // The connected socket's server end must belong to the proven process, before any RFB byte.
        let client = stream
            .local_addr()
            .map_err(|_| "could not read the console connection's address".to_owned())?;
        binding::check_connection(probe, settings.port, client, owner)?;
        stream
            .set_nodelay(true)
            .map_err(|_| "the VM's console connection failed".to_owned())?;
        let session = rfb::Session::connect(stream)?;
        Ok(Self {
            viewer: rfb::Viewer::new(session),
        })
    }

    /// The current screen, without waiting: an update already received, or the last frame. The
    /// viewer keeps one request outstanding, so the next change is on its way between polls and
    /// the run's channel (which input also needs) is never held waiting for the guest.
    pub(super) fn frame(&mut self) -> Result<Frame, String> {
        self.frame_within(Duration::ZERO)
    }

    /// [`Self::frame`], waiting up to `wait` for a change.
    pub(super) fn frame_within(&mut self, wait: Duration) -> Result<Frame, String> {
        self.viewer.frame(wait, FIRST_FRAME_WAIT)
    }

    /// Send one gesture to the guest. `width` and `height` are the frame size the gesture was
    /// made against; pointer positions are scaled to the guest framebuffer.
    pub(super) fn input(&mut self, event: &Gesture, width: u32, height: u32) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Err("no guest frame is available".into());
        }
        match event {
            Gesture::Click { x, y, button } => {
                if *x >= width || *y >= height {
                    return Err("click is outside the guest frame".into());
                }
                let mask = button_mask(button)?;
                let gx = scale(*x, width, self.viewer.size().0);
                let gy = scale(*y, height, self.viewer.size().1);
                self.viewer.session.pointer(mask, gx, gy)?;
                self.viewer.session.pointer(0, gx, gy)
            }
            Gesture::Press { code, modifiers } => {
                let key = qcode_for(code).ok_or_else(|| "unsupported physical key".to_owned())?;
                if modifiers.len() > MAX_MODIFIERS
                    || modifiers
                        .iter()
                        .enumerate()
                        .any(|(i, m)| modifiers[..i].contains(m))
                {
                    return Err("duplicate key modifiers".into());
                }
                // Resolve every keysym before the first event so a refusal sends nothing.
                let mods: Vec<&str> = modifiers
                    .iter()
                    .map(|m| modifier_qcode(*m))
                    .filter(|m| *m != key)
                    .collect();
                let mut sequence = Vec::with_capacity(mods.len() * 2 + 2);
                for m in &mods {
                    sequence.push((true, keys::keysym(m)?));
                }
                let key_sym = keys::keysym(key)?;
                sequence.push((true, key_sym));
                sequence.push((false, key_sym));
                for m in mods.iter().rev() {
                    sequence.push((false, keys::keysym(m)?));
                }
                for (down, keysym) in sequence {
                    self.viewer.session.key(down, keysym)?;
                }
                Ok(())
            }
            _ => Err("that input needs the live screen".into()),
        }
    }

    /// Stream steps: moves and held buttons as RFB pointer events, wheel notches as buttons 4/5,
    /// keys as key events. `width` and `height` are the frame the steps were made against.
    pub(super) fn stream(&mut self, steps: &[Low], width: u32, height: u32) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Err("no guest frame is available".into());
        }
        for step in steps {
            match step {
                Low::Mouse { x, y, buttons, .. } => {
                    let (gx, gy) = (scale(*x, width, self.viewer.size().0), scale(*y, height, self.viewer.size().1));
                    self.viewer.session.pointer(rfb_buttons(*buttons), gx, gy)?;
                }
                Low::Wheel { x, y, buttons, dy } => {
                    let (gx, gy) = (scale(*x, width, self.viewer.size().0), scale(*y, height, self.viewer.size().1));
                    let held = rfb_buttons(*buttons);
                    let notch = if *dy > 0 { 0x10 } else { 0x08 };
                    for _ in 0..dy.unsigned_abs() {
                        self.viewer.session.pointer(held | notch, gx, gy)?;
                        self.viewer.session.pointer(held, gx, gy)?;
                    }
                }
                Low::Key { qcode, down, .. } => self.viewer.session.key(*down, keys::keysym(qcode)?)?,
            }
        }
        Ok(())
    }
}

fn scale(value: u32, from: u32, to: u32) -> u16 {
    let scaled = u64::from(value) * u64::from(to) / u64::from(from);
    scaled.min(u64::from(to.saturating_sub(1))) as u16
}

/// DOM button bits (1 left, 2 right, 4 middle) to RFB's (1 left, 2 middle, 4 right).
fn rfb_buttons(dom: u8) -> u8 {
    (dom & 1) | if dom & 4 != 0 { 2 } else { 0 } | if dom & 2 != 0 { 4 } else { 0 }
}

fn button_mask(button: &str) -> Result<u8, String> {
    match button {
        "left" => Ok(1),
        "middle" => Ok(2),
        "right" => Ok(4),
        other => Err(format!("the mouse button {other} is not supported")),
    }
}

fn modifier_qcode(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::Shift => "shift",
        Modifier::Control => "ctrl",
        Modifier::Alt => "alt",
        Modifier::Meta => "meta_l",
    }
}
