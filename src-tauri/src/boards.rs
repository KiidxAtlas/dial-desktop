//! User-operated serial workbench for anything that shows up as a serial port. Only explicit
//! user actions (a sent line, keystrokes in the live console, a control-line button) reach the
//! device.
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use std::collections::VecDeque;

mod agent;
pub mod android;
pub mod apple;
pub mod project;
mod repl;
pub mod usb;
pub use agent::{call as agent_call, tools as agent_tools};
use serialport::{SerialPort, SerialPortInfo, SerialPortType};
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

#[derive(Serialize)]
pub struct Board {
    path: String,
    /// `usb`, `bluetooth`, `builtin` or `other`.
    transport: &'static str,
    product: Option<String>,
    manufacturer: Option<String>,
    serial_number: Option<String>,
    usb_id: Option<String>,
    /// Best guess at what the device is, from its USB IDs and product name.
    kind: Option<&'static str>,
    /// How to interact with that kind of device.
    hint: Option<&'static str>,
}

// Model-specific USB IDs come from Arduino's boards.txt and CircuitPython's board
// mpconfigboard.mk files. The picker reads the same catalog for names and wiring.
#[derive(serde::Deserialize)]
struct CatalogModel {
    name: String,
    usb: Vec<String>,
}

fn catalog_model(vid: u16, pid: u16) -> Option<&'static str> {
    static MODELS: std::sync::OnceLock<Vec<CatalogModel>> = std::sync::OnceLock::new();
    let models = MODELS.get_or_init(|| {
        serde_json::from_str(include_str!("../../src/boardCatalog.json")).expect("valid board catalog")
    });
    let id = format!("{vid:04x}:{pid:04x}");
    models.iter().find(|m| m.usb.contains(&id)).map(|m| m.name.as_str())
}

/// Identify common boards and USB-serial chips. A guess only: many vendors reuse these IDs.
fn identify(vid: u16, pid: u16, product: &str) -> Option<(&'static str, &'static str)> {
    let product = product.to_ascii_lowercase();
    let repl = "Often MicroPython or CircuitPython: open the console and press Enter (or Ctrl-C) for a >>> prompt.";
    let uart = "USB-to-UART bridge, common on ESP32 and Arduino-compatible boards. Boot logs are usually at 115200 baud.";
    let sketch = "Prints whatever its sketch sends with Serial.print; match the sketch’s baud rate.";
    if let Some(name) = catalog_model(vid, pid) {
        return Some((name, if vid == 0x239a { repl } else { sketch }));
    }
    Some(match (vid, pid) {
        _ if product.contains("flipper") => ("Flipper Zero", "Has a command line: open the console, press Enter, then type help."),
        (0x2341, 0x0043 | 0x0001 | 0x0243 | 0x006a) | (0x2a03, 0x0043) => ("Arduino Uno", sketch),
        (0x2341 | 0x2a03, _) => ("Arduino board", sketch),
        (0x1b4f, _) => ("SparkFun board", "Match the model printed on the board before wiring it."),
        (0x303a, _) => ("Espressif ESP32 (native USB)", "Shows the boot log and app output; MicroPython builds give a >>> prompt."),
        (0x10c4, 0xea60 | 0xea70) => ("Silicon Labs CP210x bridge", uart),
        (0x1a86, _) => ("WCH CH340/CH9102 bridge", uart),
        (0x0403, _) => ("FTDI USB-serial bridge", uart),
        (0x2e8a, _) => ("Raspberry Pi RP2040/RP2350", repl),
        (0x239a, _) => ("Adafruit board", repl),
        (0x16c0, 0x0483) => ("Teensy", "Prints whatever its sketch sends with Serial.print."),
        (0x0d28, _) => ("Arm DAPLink / micro:bit", "Debug probe serial port: shows the target’s UART output."),
        (0x1366, _) => ("SEGGER J-Link", "Debug probe serial port: shows the target’s UART output."),
        (0x1915, _) => ("Nordic nRF device", "Zephyr and nRF Connect apps often have a shell: press Enter, then type help."),
        (0x0483, 0x5740) => ("STM32 virtual COM port", "Firmware-defined; press Enter in the console to look for a prompt."),
        _ => return None,
    })
}

fn board(port: SerialPortInfo) -> Option<Board> {
    // macOS reports dial-in and callout aliases for the same interface. Use the callout port.
    if cfg!(target_os = "macos") && port.port_name.starts_with("/dev/tty.") {
        return None;
    }
    let (transport, usb) = match port.port_type {
        SerialPortType::UsbPort(info) => ("usb", Some(info)),
        SerialPortType::BluetoothPort => ("bluetooth", None),
        SerialPortType::PciPort => ("builtin", None),
        SerialPortType::Unknown => ("other", None),
    };
    let found = usb
        .as_ref()
        .and_then(|u| identify(u.vid, u.pid, u.product.as_deref().unwrap_or_default()));
    Some(Board {
        path: port.port_name,
        transport,
        usb_id: usb.as_ref().map(|u| format!("{:04x}:{:04x}", u.vid, u.pid)),
        product: usb.as_ref().and_then(|u| u.product.clone()),
        manufacturer: usb.as_ref().and_then(|u| u.manufacturer.clone()),
        serial_number: usb.and_then(|u| u.serial_number),
        kind: found.map(|f| f.0),
        hint: found.map(|f| f.1),
    })
}

/// Every serial port, plugged-in USB devices first.
fn inventory() -> Result<Vec<Board>, String> {
    let mut boards: Vec<_> = serialport::available_ports()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(board)
        .collect();
    boards.sort_by(|a, b| (a.transport != "usb", &a.path).cmp(&(b.transport != "usb", &b.path)));
    Ok(boards)
}

#[tauri::command]
pub async fn board_ports(webview: tauri::Webview) -> Result<Vec<Board>, String> {
    crate::app::main_only(&webview, "list serial ports")?;
    tauri::async_runtime::spawn_blocking(inventory)
        .await
        .map_err(|e| e.to_string())?
}


const LIMIT: usize = 8192;
#[derive(Serialize)]
pub struct Capture {
    text: String,
    bytes: usize,
    capped: bool,
}

fn sample(reader: &mut impl Read, duration: Duration) -> Result<Capture, String> {
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut buf = [0; 512];
    while bytes.len() < LIMIT && start.elapsed() < duration {
        let take = (LIMIT - bytes.len()).min(buf.len());
        match reader.read(&mut buf[..take]) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::Interrupted) => continue,
            Err(e) => return Err(format!("Serial read stopped: {e}")),
        }
    }
    let text = String::from_utf8_lossy(&bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|&c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    Ok(Capture {
        text,
        bytes: bytes.len(),
        capped: bytes.len() == LIMIT,
    })
}

#[derive(serde::Deserialize)]
pub struct SerialLine {
    text: String,
    ending: String,
}

impl SerialLine {
    fn encode(self) -> Result<Vec<u8>, String> {
        if self.text.trim().is_empty()
            || self.text.len() > 512
            || self.text.chars().any(char::is_control)
        {
            return Err(
                "Enter a single command of up to 512 bytes without control characters".into(),
            );
        }
        let suffix = match self.ending.as_str() {
            "lf" => "\n",
            "crlf" => "\r\n",
            "cr" => "\r",
            "none" => "",
            _ => return Err("Unsupported serial line ending".into()),
        };
        let mut bytes = self.text.into_bytes();
        bytes.extend_from_slice(suffix.as_bytes());
        Ok(bytes)
    }
}

/// Framing beyond the baud rate. Unset fields mean 8 data bits, no parity, 1 stop bit and no
/// flow control, which is what nearly every board uses.
#[derive(serde::Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Framing {
    data_bits: Option<u8>,
    parity: Option<String>,
    stop_bits: Option<u8>,
    flow: Option<String>,
}

fn builder(path: &str, baud: u32, framing: &Framing) -> Result<serialport::SerialPortBuilder, String> {
    use serialport::{DataBits, FlowControl, Parity, StopBits};
    if !(50..=4_000_000).contains(&baud) {
        return Err("Choose a baud rate between 50 and 4,000,000".into());
    }
    let data_bits = match framing.data_bits.unwrap_or(8) {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        8 => DataBits::Eight,
        _ => return Err("Unsupported data bits".into()),
    };
    let parity = match framing.parity.as_deref().unwrap_or("none") {
        "none" => Parity::None,
        "odd" => Parity::Odd,
        "even" => Parity::Even,
        _ => return Err("Unsupported parity".into()),
    };
    let stop_bits = match framing.stop_bits.unwrap_or(1) {
        1 => StopBits::One,
        2 => StopBits::Two,
        _ => return Err("Unsupported stop bits".into()),
    };
    let flow = match framing.flow.as_deref().unwrap_or("none") {
        "none" => FlowControl::None,
        "software" => FlowControl::Software,
        "hardware" => FlowControl::Hardware,
        _ => return Err("Unsupported flow control".into()),
    };
    Ok(serialport::new(path, baud)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits)
        .flow_control(flow)
        .timeout(Duration::from_millis(100)))
}

fn open_port(path: &str, baud: u32, framing: &Framing) -> Result<Box<dyn SerialPort>, String> {
    let builder = builder(path, baud, framing)?;
    if live_path().as_deref() == Some(path) {
        return Err("The live console is using this port. Disconnect it first.".into());
    }
    if !inventory()?.iter().any(|b| b.path == path) {
        return Err("Serial port is no longer connected".into());
    }
    // A console that just closed may hold the port for one more read timeout.
    let mut tries = 0;
    loop {
        match builder.clone().open() {
            Ok(port) => return Ok(port),
            Err(_) if tries < 3 => tries += 1,
            Err(e) => return Err(format!("Could not open port: {e}")),
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[tauri::command]
pub async fn board_capture(
    webview: tauri::Webview,
    path: String,
    baud: u32,
    line: Option<SerialLine>,
) -> Result<Capture, String> {
    crate::app::main_only(&webview, "capture serial output")?;
    tauri::async_runtime::spawn_blocking(move || {
        let payload = line.map(SerialLine::encode).transpose()?;
        let mut port = open_port(&path, baud, &Framing::default())?;
        if let Some(bytes) = payload {
            port.write_all(&bytes)
                .map_err(|e| format!("Serial write failed (possibly partially sent): {e}"))?;
        }
        sample(&mut port, Duration::from_secs(5))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What the live console reports, as `board-serial` app events. Both the panel and Dial's board
/// tools can open the console, so every event names its session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SerialEvent {
    Opened { id: u32, path: String, baud: u32 },
    /// Received bytes, exactly as they arrived (the console decodes or hex-dumps them).
    Data { id: u32, bytes: Vec<u8> },
    /// Something done to the device that its output alone would not show.
    Note { id: u32, text: String },
    /// The connection ended: `reason` is `None` after a requested disconnect.
    Closed { id: u32, reason: Option<String> },
}
const SERIAL_EVENT: &str = "board-serial";

/// The newest console bytes, counted from the start of the session so a reader can ask for
/// whatever arrived after an offset.
#[derive(Default)]
struct Log {
    bytes: VecDeque<u8>,
    total: u64,
}
const LOG_LIMIT: usize = 256 * 1024;

impl Log {
    fn push(&mut self, data: &[u8]) {
        self.bytes.extend(data);
        let extra = self.bytes.len().saturating_sub(LOG_LIMIT);
        self.bytes.drain(..extra);
        self.total += data.len() as u64;
    }
    /// The kept bytes from `offset` on, and the offset just past them.
    fn since(&self, offset: u64) -> (Vec<u8>, u64) {
        let first = self.total - self.bytes.len() as u64;
        let skip = offset.clamp(first, self.total) - first;
        (self.bytes.iter().skip(skip as usize).copied().collect(), self.total)
    }
}

/// The one open live console. Capture and send refuse its port while it is open.
struct Live {
    id: u32,
    path: String,
    baud: u32,
    framing: Framing,
    writer: Box<dyn SerialPort>,
    stop: Arc<AtomicBool>,
    log: Arc<Mutex<Log>>,
}
static LIVE: Mutex<Option<Live>> = Mutex::new(None);
static CONNECT: Mutex<()> = Mutex::new(());
static NEXT_LIVE: AtomicU32 = AtomicU32::new(0);

fn live() -> std::sync::MutexGuard<'static, Option<Live>> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}
fn live_path() -> Option<String> {
    live().as_ref().map(|l| l.path.clone())
}
/// The open console: its id, port and baud rate.
fn live_info() -> Option<(u32, String, u32)> {
    live().as_ref().map(|l| (l.id, l.path.clone(), l.baud))
}
/// Console `id`'s output from `offset` on, and the offset just past it.
fn log_since(id: u32, offset: u64) -> Result<(Vec<u8>, u64), String> {
    let log = live().as_ref().filter(|l| l.id == id).map(|l| Arc::clone(&l.log)).ok_or(CLOSED)?;
    let log = log.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(log.since(offset))
}
const CLOSED: &str = "The serial console is closed";

/// Copy port output to `sink` until `stop` is set or the read fails (for example, unplugged).
fn pump(reader: &mut impl Read, stop: &AtomicBool, sink: impl Fn(&[u8])) -> Option<String> {
    let mut buf = [0u8; 4096];
    loop {
        if stop.load(Ordering::Relaxed) {
            break None;
        }
        match reader.read(&mut buf) {
            Ok(0) => break Some("The device closed the connection".to_string()),
            Ok(n) => sink(&buf[..n]),
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::Interrupted | ErrorKind::WouldBlock) => {}
            Err(e) => break Some(format!("Serial read stopped: {e}")),
        }
    }
}

/// Open `path` as the live, two-way console, replacing any console already open. Returns the
/// session id; output follows as `board-serial` events and is kept for `log_since`.
fn connect_locked(app: &AppHandle, path: String, baud: u32, framing: Framing) -> Result<u32, String> {
    disconnect(None);
    let mut reader = open_port(&path, baud, &framing)?;
    let writer = reader.try_clone().map_err(|e| format!("Could not open port for writing: {e}"))?;
    let id = NEXT_LIVE.fetch_add(1, Ordering::Relaxed) + 1;
    let stop = Arc::new(AtomicBool::new(false));
    let log = Arc::new(Mutex::new(Log::default()));
    let prev = live().replace(Live { id, path: path.clone(), baud, framing, writer, stop: Arc::clone(&stop), log: Arc::clone(&log) });
    if let Some(prev) = prev {
        prev.stop.store(true, Ordering::Relaxed);
    }
    let _ = app.emit(SERIAL_EVENT, SerialEvent::Opened { id, path, baud });
    let app = app.clone();
    std::thread::Builder::new()
        .name("dial-serial-console".into())
        .spawn(move || {
            let reason = pump(&mut reader, &stop, |bytes| {
                log.lock().unwrap_or_else(PoisonError::into_inner).push(bytes);
                let _ = app.emit(SERIAL_EVENT, SerialEvent::Data { id, bytes: bytes.to_vec() });
            });
            drop(reader);
            disconnect(Some(id));
            let _ = app.emit(SERIAL_EVENT, SerialEvent::Closed { id, reason });
        })
        .map_err(|e| {
            disconnect(Some(id));
            format!("Could not start the serial reader: {e}")
        })?;
    Ok(id)
}

fn connect(app: &AppHandle, path: String, baud: u32, framing: Framing) -> Result<u32, String> {
    let _guard = CONNECT.lock().unwrap_or_else(PoisonError::into_inner);
    connect_locked(app, path, baud, framing)
}

#[tauri::command]
pub async fn board_connect(
    app: AppHandle,
    webview: tauri::Webview,
    path: String,
    baud: u32,
    framing: Option<Framing>,
) -> Result<u32, String> {
    crate::app::main_only(&webview, "open a serial console")?;
    tauri::async_runtime::spawn_blocking(move || connect(&app, path, baud, framing.unwrap_or_default()))
        .await
        .map_err(|e| e.to_string())?
}

/// The open console and the end of its output, for a panel that opens after the console did.
#[derive(Serialize)]
pub struct LiveView {
    id: u32,
    path: String,
    baud: u32,
    bytes: Vec<u8>,
}

#[tauri::command]
pub fn board_live(webview: tauri::Webview) -> Result<Option<LiveView>, String> {
    crate::app::main_only(&webview, "read the serial console")?;
    let Some((id, path, baud)) = live_info() else {
        return Ok(None);
    };
    let (mut bytes, _) = log_since(id, 0)?;
    bytes.drain(..bytes.len().saturating_sub(32 * 1024));
    Ok(Some(LiveView { id, path, baud, bytes }))
}

/// Close console `id` (or whichever is open); closing one that already ended is fine.
fn disconnect(id: Option<u32>) {
    let mut slot = live();
    if slot.as_ref().is_some_and(|l| id.is_none_or(|id| l.id == id))
        && let Some(l) = slot.take()
    {
        l.stop.store(true, Ordering::Relaxed);
    }
}

/// Close the console so another program can use its port, keeping what `resume` needs.
fn suspend() -> Option<(String, u32, Framing)> {
    let held = live().as_ref().map(|l| (l.path.clone(), l.baud, l.framing.clone()))?;
    disconnect(None);
    // The reader lets go of the port within one read timeout.
    std::thread::sleep(Duration::from_millis(300));
    Some(held)
}

/// Reopen a suspended console, waiting for a board that restarts after flashing to come back.
fn resume(app: &AppHandle, (path, baud, framing): (String, u32, Framing)) {
    for _ in 0..40 {
        let connected = {
            let _guard = CONNECT.lock().unwrap_or_else(PoisonError::into_inner);
            live().is_some() || connect_locked(app, path.clone(), baud, framing.clone()).is_ok()
        };
        if connected {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[tauri::command]
pub fn board_disconnect(webview: tauri::Webview, id: u32) -> Result<(), String> {
    crate::app::main_only(&webview, "close a serial console")?;
    disconnect(Some(id));
    Ok(())
}

/// Send `data` to console `id`.
fn write_live(id: u32, data: &[u8]) -> Result<(), String> {
    let mut slot = live();
    let l = slot.as_mut().filter(|l| l.id == id).ok_or(CLOSED)?;
    l.writer
        .write_all(data)
        .and_then(|()| l.writer.flush())
        .map_err(|e| format!("Serial write failed: {e}"))
}

/// Send `data` (keystrokes, a paste, a line or raw bytes) to console `id`.
#[tauri::command]
pub async fn board_write(webview: tauri::Webview, id: u32, data: Vec<u8>) -> Result<(), String> {
    crate::app::main_only(&webview, "write to a serial console")?;
    if data.len() > 16 * 1024 {
        return Err("Send at most 16 KiB at a time".into());
    }
    tauri::async_runtime::spawn_blocking(move || write_live(id, &data))
        .await
        .map_err(|e| e.to_string())?
}

/// Run `change` on console `id`'s port, reporting failures as a control-line problem.
fn with_live(id: u32, change: impl FnOnce(&mut dyn SerialPort) -> serialport::Result<()>) -> Result<(), String> {
    let mut slot = live();
    let l = slot.as_mut().filter(|l| l.id == id).ok_or(CLOSED)?;
    change(l.writer.as_mut()).map_err(|e| format!("Could not set control lines: {e}"))
}

/// Hold the DTR and RTS lines as given. Many boards wire them to reset or bootloader pins.
#[tauri::command]
pub async fn board_lines(webview: tauri::Webview, id: u32, dtr: bool, rts: bool) -> Result<(), String> {
    crate::app::main_only(&webview, "set serial control lines")?;
    tauri::async_runtime::spawn_blocking(move || {
        with_live(id, |port| {
            port.write_data_terminal_ready(dtr)?;
            port.write_request_to_send(rts)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Send a break (the data line held low for a quarter second), which some bootloaders, consoles
/// and LIN/DMX-style devices treat as an attention signal.
#[tauri::command]
pub async fn board_break(webview: tauri::Webview, id: u32) -> Result<(), String> {
    crate::app::main_only(&webview, "send a serial break")?;
    tauri::async_runtime::spawn_blocking(move || {
        with_live(id, |port| port.set_break())?;
        std::thread::sleep(Duration::from_millis(250));
        with_live(id, |port| port.clear_break())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pulse the DTR and RTS lines the way common auto-reset circuits expect: RTS resets ESP32
/// boards (DTR stays released so they boot normally), and re-asserting DTR resets Arduinos.
fn reset(id: u32) -> Result<(), String> {
    for (dtr, rts, wait) in [(false, true, 100), (false, false, 50), (true, false, 0)] {
        with_live(id, |port| {
            port.write_data_terminal_ready(dtr)?;
            port.write_request_to_send(rts)
        })?;
        std::thread::sleep(Duration::from_millis(wait));
    }
    Ok(())
}

#[tauri::command]
pub async fn board_reset(webview: tauri::Webview, id: u32) -> Result<(), String> {
    crate::app::main_only(&webview, "reset a serial board")?;
    tauri::async_runtime::spawn_blocking(move || reset(id))
        .await
        .map_err(|e| e.to_string())?
}

/// The console as a MicroPython wire: writes go to the port, reads follow the console's log.
struct LiveWire {
    id: u32,
    offset: u64,
}

impl repl::Wire for LiveWire {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        write_live(self.id, bytes)
    }
    fn recv(&mut self) -> Result<Vec<u8>, String> {
        let (bytes, next) = log_since(self.id, self.offset)?;
        self.offset = next;
        Ok(bytes)
    }
}

/// One MicroPython exchange at a time: a second would interleave with the first.
static REPL: Mutex<()> = Mutex::new(());

/// Run `job` on console `id`'s MicroPython prompt.
fn with_repl<T>(id: u32, job: impl FnOnce(&mut LiveWire) -> Result<T, String>) -> Result<T, String> {
    let _one = REPL.try_lock().map_err(|_| "The device is busy with another run")?;
    let (_, offset) = log_since(id, u64::MAX)?;
    job(&mut LiveWire { id, offset })
}

/// Run MicroPython `code` on the device behind console `id`, waiting up to `seconds` for it.
#[tauri::command]
pub async fn board_run(webview: tauri::Webview, id: u32, code: String, seconds: Option<u64>) -> Result<repl::Output, String> {
    crate::app::main_only(&webview, "run code on a board")?;
    let wait = Duration::from_secs(seconds.unwrap_or(10).min(120));
    tauri::async_runtime::spawn_blocking(move || with_repl(id, |wire| repl::exec(wire, &code, wait)))
        .await
        .map_err(|e| e.to_string())?
}

/// Save `content` as file `name` on the MicroPython device behind console `id`.
#[tauri::command]
pub async fn board_put(webview: tauri::Webview, id: u32, name: String, content: String) -> Result<(), String> {
    crate::app::main_only(&webview, "save a file on a board")?;
    tauri::async_runtime::spawn_blocking(move || with_repl(id, |wire| repl::put(wire, &name, content.as_bytes())))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serial_command_is_one_bounded_line() {
        let encode = |text: &str, ending: &str| {
            SerialLine {
                text: text.into(),
                ending: ending.into(),
            }
            .encode()
        };
        for (ending, suffix) in [("lf", "\n"), ("crlf", "\r\n"), ("cr", "\r"), ("none", "")] {
            assert_eq!(
                encode(" status", ending).unwrap(),
                format!(" status{suffix}").as_bytes()
            );
        }
        for invalid in [" ", "a\nb", "a\0b", &"é".repeat(257)] {
            assert!(encode(invalid, "lf").is_err());
        }
        assert!(encode("ok", "unknown").is_err());
    }
    /// Hardware smoke check; scanning must not open or reset the attached board.
    #[test]
    #[ignore = "requires DIAL_TEST_USB_PORT to name an attached USB serial device"]
    fn discovers_attached_port() {
        let expected = std::env::var("DIAL_TEST_USB_PORT").unwrap();
        assert!(inventory().unwrap().iter().any(|b| b.path == expected));
    }
    #[test]
    fn sample_is_bounded_and_plain_text() {
        let mut input = &b"boot\r\nok\r\0\xff"[..];
        let capture = sample(&mut input, Duration::from_secs(1)).unwrap();
        assert_eq!(capture.text, "boot\nok\n�");
        assert_eq!(capture.bytes, 11);
        assert!(!capture.capped);
        let data = vec![b'x'; LIMIT + 1];
        let mut input = data.as_slice();
        let capture = sample(&mut input, Duration::from_secs(1)).unwrap();
        assert!(capture.capped);
        assert_eq!(input.len(), 1);
    }
    #[test]
    fn lists_non_usb_ports_without_usb_details() {
        let found = board(SerialPortInfo {
            port_name: "/dev/cu.debug".into(),
            port_type: SerialPortType::Unknown,
        })
        .unwrap();
        assert_eq!((found.transport, found.usb_id, found.kind), ("other", None, None));
    }
    #[test]
    fn console_log_keeps_the_newest_bytes_and_reads_from_an_offset() {
        let mut log = Log::default();
        log.push(b"hello ");
        log.push(b"board");
        assert_eq!(log.since(0), (b"hello board".to_vec(), 11));
        assert_eq!(log.since(6), (b"board".to_vec(), 11));
        assert_eq!(log.since(u64::MAX), (Vec::new(), 11));
        log.push(&vec![b'x'; LOG_LIMIT]);
        let (kept, total) = log.since(0);
        assert_eq!((kept.len(), total), (LOG_LIMIT, 11 + LOG_LIMIT as u64));
        assert_eq!(log.since(total - 2).0, b"xx");
    }
    #[test]
    fn framing_defaults_to_8n1_and_rejects_values_no_port_supports() {
        let framing = |json: &str| serde_json::from_str::<Framing>(json).unwrap();
        assert!(builder("p", 115200, &Framing::default()).is_ok());
        assert!(builder("p", 250_000, &framing(r#"{"dataBits":7,"parity":"even","stopBits":2,"flow":"hardware"}"#)).is_ok());
        assert!(builder("p", 49, &Framing::default()).is_err());
        for bad in [r#"{"dataBits":9}"#, r#"{"parity":"mark"}"#, r#"{"stopBits":3}"#, r#"{"flow":"x"}"#] {
            assert!(builder("p", 9600, &framing(bad)).is_err(), "{bad}");
        }
    }
    #[test]
    fn identifies_common_boards_and_leaves_unknown_alone() {
        assert_eq!(identify(0x0483, 0x5740, "Flipper Ana7one").unwrap().0, "Flipper Zero");
        assert_eq!(identify(0x0483, 0x5740, "").unwrap().0, "STM32 virtual COM port");
        assert_eq!(identify(0x10c4, 0xea60, "CP2102").unwrap().0, "Silicon Labs CP210x bridge");
        assert_eq!(identify(0x2341, 0x0042, "USB Serial").unwrap().0, "Arduino Mega 2560");
        assert_eq!(identify(0x239a, 0x80f8, "").unwrap().0, "Adafruit QT Py RP2040");
        assert_eq!(identify(0x2341, 0xffff, "").unwrap().0, "Arduino board");
        assert_eq!(identify(0x1b4f, 0xffff, "").unwrap().0, "SparkFun board");
        assert!(identify(0x1234, 0x5678, "Widget").is_none());
    }
    #[test]
    fn live_console_streams_bytes_until_stopped_or_ended() {
        let seen = std::cell::RefCell::new(Vec::new());
        // Bytes arrive untouched (even invalid UTF-8); end of input reports why the console closed.
        let mut input = std::io::Read::chain(&b">: \xc3"[..], &b"\xa9\xff\r\n"[..]);
        let reason = pump(&mut input, &AtomicBool::new(false), |bytes| seen.borrow_mut().extend_from_slice(bytes));
        assert_eq!(*seen.borrow(), b">: \xc3\xa9\xff\r\n");
        assert!(reason.is_some());
        let mut input = &b"ignored"[..];
        assert_eq!(pump(&mut input, &AtomicBool::new(true), |_| panic!("stopped")), None);
    }
}
