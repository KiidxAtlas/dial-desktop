//! The RFB (VNC) client behind the live screen of a VMware VM and of Dial's own QEMU guest.
//!
//! Only what the console needs: protocol 3.3 to 3.8, security type None, 32-bit true colour, and
//! the Raw and DesktopSize encodings. Password authentication is refused before any password is
//! read or sent, so no credential enters this process.
//!
//! [`Viewer`] keeps the screen: it always has one update request outstanding, polls for the reply
//! without blocking, and writes only the rectangles the server sent into its RGBA copy.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use super::input::Frame;

/// Largest accepted screen side; matches the PPM limit in `input::frame_from_ppm`.
pub const MAX_SIDE: u32 = 4096;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_NAME: u32 = 1024;
const MAX_REASON: u32 = 4096;
const MAX_CUT_TEXT: u32 = 1 << 20;
const MAX_RECTS: u16 = 256;
const SECURITY_NONE: u8 = 1;
const SECURITY_VNC_AUTH: u8 = 2;
const ENCODING_RAW: i32 = 0;
const ENCODING_DESKTOP_SIZE: i32 = -223;
/// Refusal when the server requires VNC password authentication, which this client does not support.
const PASSWORD_REFUSED: &str =
    "the VM's VNC console requires password authentication, which is unsupported";
/// Total raw pixel bytes one FramebufferUpdate may carry: one full 4096x4096 screen at 32 bpp.
/// Checked per rectangle before its buffer is allocated, so spoofed headers cannot exhaust memory.
const MAX_UPDATE_BYTES: u64 = (MAX_SIDE as u64) * (MAX_SIDE as u64) * 4;

/// A connected byte stream an RFB session can run over: loopback TCP or a Unix socket.
pub trait Stream: Read + Write + Send {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()>;
}

impl Stream for TcpStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_write_timeout(self, timeout)
    }
    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        TcpStream::set_nonblocking(self, nonblocking)
    }
}

#[cfg(unix)]
impl Stream for std::os::unix::net::UnixStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, timeout)
    }
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_write_timeout(self, timeout)
    }
    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_nonblocking(self, nonblocking)
    }
}

/// One decoded part of a FramebufferUpdate.
pub enum Update {
    Pixels {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        /// BGRX rows, four bytes per pixel.
        data: Vec<u8>,
    },
    Resize {
        width: u32,
        height: u32,
    },
}

/// An RFB session over a connected socket. The session owns the socket; dropping it closes it.
pub struct Session<S: Stream = TcpStream> {
    stream: S,
    width: u32,
    height: u32,
}

impl<S: Stream> Session<S> {
    pub fn connect(mut stream: S) -> Result<Self, String> {
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(transport)?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(transport)?;
        handshake(&mut stream)?;
        write(&mut stream, &[1])?; // ClientInit: shared
        let (width, height) = server_init(&mut stream)?;
        let mut session = Self {
            stream,
            width,
            height,
        };
        session.send(&set_pixel_format())?;
        session.send(&set_encodings())?;
        Ok(session)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Ask for the whole screen (`incremental` false) or only what changed.
    pub fn request(&mut self, incremental: bool) -> Result<(), String> {
        let mut msg = vec![3, u8::from(incremental), 0, 0, 0, 0];
        msg.extend_from_slice(&(self.width as u16).to_be_bytes());
        msg.extend_from_slice(&(self.height as u16).to_be_bytes());
        self.send(&msg)
    }

    /// Wait up to `wait` for the next FramebufferUpdate (`Duration::ZERO`: only one already
    /// arrived). Bell and clipboard messages are consumed. Returns `Ok(None)` when nothing arrived
    /// in time; the connection stays usable.
    pub fn next_update(&mut self, wait: Duration) -> Result<Option<Vec<Update>>, String> {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let mut kind = [0u8; 1];
            let read = if left.is_zero() {
                // Poll: a message already in the socket, or nothing.
                self.stream.set_nonblocking(true).map_err(transport)?;
                let read = self.stream.read(&mut kind);
                self.stream.set_nonblocking(false).map_err(transport)?;
                read
            } else {
                self.stream
                    .set_read_timeout(Some(left))
                    .map_err(transport)?;
                self.stream.read(&mut kind)
            };
            match read {
                Ok(0) => return Err("the VM closed its console connection".into()),
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(transport(error)),
            }
            // The rest of a message is read with the full timeout, never cut short mid-message.
            self.stream
                .set_read_timeout(Some(IO_TIMEOUT))
                .map_err(transport)?;
            match kind[0] {
                0 => return self.framebuffer_update().map(Some),
                2 => {}
                3 => {
                    // ServerCutText: three padding bytes, a length, then the text. The text is read
                    // and discarded: clipboard contents never leave this client.
                    let _padding: [u8; 3] = read_array(&mut self.stream)?;
                    let len = u32::from_be_bytes(read_array(&mut self.stream)?);
                    if len > MAX_CUT_TEXT {
                        return Err("the VM sent too much clipboard text".into());
                    }
                    let mut discarded = vec![0; len as usize];
                    fill(&mut self.stream, &mut discarded)?;
                }
                1 => {
                    return Err(
                        "the VM changed its colour map, which this client does not support".into(),
                    );
                }
                _ => return Err("the VM sent a message this client does not understand".into()),
            }
        }
    }

    pub fn pointer(&mut self, buttons: u8, x: u16, y: u16) -> Result<(), String> {
        let mut msg = vec![5, buttons];
        msg.extend_from_slice(&x.to_be_bytes());
        msg.extend_from_slice(&y.to_be_bytes());
        self.send(&msg)
    }

    pub fn key(&mut self, down: bool, keysym: u32) -> Result<(), String> {
        let mut msg = vec![4, u8::from(down), 0, 0];
        msg.extend_from_slice(&keysym.to_be_bytes());
        self.send(&msg)
    }

    fn send(&mut self, msg: &[u8]) -> Result<(), String> {
        write(&mut self.stream, msg)
    }

    fn framebuffer_update(&mut self) -> Result<Vec<Update>, String> {
        let _padding: [u8; 1] = read_array(&mut self.stream)?;
        let count = u16::from_be_bytes(read_array(&mut self.stream)?);
        if count > MAX_RECTS {
            return Err("the VM sent too many screen regions at once".into());
        }
        let mut updates = Vec::with_capacity(count as usize);
        let mut budget = MAX_UPDATE_BYTES;
        for _ in 0..count {
            let x = u32::from(u16::from_be_bytes(read_array(&mut self.stream)?));
            let y = u32::from(u16::from_be_bytes(read_array(&mut self.stream)?));
            let width = u32::from(u16::from_be_bytes(read_array(&mut self.stream)?));
            let height = u32::from(u16::from_be_bytes(read_array(&mut self.stream)?));
            let encoding = i32::from_be_bytes(read_array(&mut self.stream)?);
            match encoding {
                ENCODING_RAW => {
                    if x + width > self.width || y + height > self.height {
                        return Err("the VM sent a screen region outside its screen".into());
                    }
                    let bytes = u64::from(width) * u64::from(height) * 4;
                    if bytes > budget {
                        return Err(
                            "the VM sent more screen data in one update than allowed".into()
                        );
                    }
                    budget -= bytes;
                    let mut data = vec![0; bytes as usize];
                    fill(&mut self.stream, &mut data)?;
                    updates.push(Update::Pixels {
                        x,
                        y,
                        width,
                        height,
                        data,
                    });
                }
                ENCODING_DESKTOP_SIZE => {
                    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
                        return Err("the VM reported an unsupported screen size".into());
                    }
                    self.width = width;
                    self.height = height;
                    updates.push(Update::Resize { width, height });
                }
                _ => {
                    return Err("the VM used a screen encoding this client does not support".into());
                }
            }
        }
        Ok(updates)
    }
}

/// The screen behind one session: RGBA pixels kept up to date from the server's rectangles.
pub struct Viewer<S: Stream = TcpStream> {
    pub session: Session<S>,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    frame: Option<Frame>,
    /// A FramebufferUpdateRequest is outstanding and its reply has not been read yet.
    requested: bool,
    /// The next request must ask for the whole screen (first frame, or after a resize).
    needs_full: bool,
}

impl<S: Stream> Viewer<S> {
    pub fn new(session: Session<S>) -> Self {
        let (width, height) = session.size();
        Self {
            session,
            width,
            height,
            rgba: vec![0; pixel_bytes(width, height)],
            frame: None,
            requested: false,
            needs_full: true,
        }
    }

    /// The framebuffer size, which pointer positions are scaled to.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The current screen. Waits up to `wait` for a change (`Duration::ZERO`: none), and up to
    /// `first` for the first screen. An unchanged screen returns the very same frame.
    pub fn frame(&mut self, wait: Duration, first: Duration) -> Result<Frame, String> {
        if !self.requested {
            let full = self.needs_full || self.frame.is_none();
            self.session.request(!full)?;
            self.needs_full = false;
            self.requested = true;
        }
        let wait = if self.frame.is_some() { wait } else { first };
        match self.session.next_update(wait)? {
            Some(updates) => {
                self.requested = false;
                self.apply(updates);
                let frame = Frame {
                    width: self.width,
                    height: self.height,
                    rgba: self.rgba.as_slice().into(),
                };
                self.frame = Some(frame.clone());
                // Ask for the next change at once, so it is already on its way at the next poll.
                let full = self.needs_full;
                self.session.request(!full)?;
                self.needs_full = false;
                self.requested = true;
                Ok(frame)
            }
            None => self
                .frame
                .clone()
                .ok_or_else(|| "the VM did not send its screen".into()),
        }
    }

    /// Write the server's rectangles (BGRX) into the RGBA copy; only those pixels are converted.
    fn apply(&mut self, updates: Vec<Update>) {
        for update in updates {
            match update {
                Update::Resize { width, height } => {
                    self.width = width;
                    self.height = height;
                    self.rgba = vec![0; pixel_bytes(width, height)];
                    self.needs_full = true;
                }
                Update::Pixels {
                    x,
                    y,
                    width,
                    height,
                    data,
                } => {
                    let row = width as usize * 4;
                    for r in 0..height as usize {
                        let dst = ((y as usize + r) * self.width as usize + x as usize) * 4;
                        let src = &data[r * row..(r + 1) * row];
                        for (out, &[b, g, red, _]) in self.rgba[dst..dst + row]
                            .as_chunks_mut::<4>()
                            .0
                            .iter_mut()
                            .zip(src.as_chunks::<4>().0)
                        {
                            *out = [red, g, b, 255];
                        }
                    }
                }
            }
        }
    }
}

fn pixel_bytes(width: u32, height: u32) -> usize {
    width as usize * height as usize * 4
}

fn handshake(stream: &mut (impl Read + Write)) -> Result<(), String> {
    let version: [u8; 12] = read_array(stream)?;
    if &version[..4] != b"RFB " || version[11] != b'\n' {
        return Err("the VM's console sent an unknown greeting".into());
    }
    let major = digits(&version[4..7])?;
    let minor = digits(&version[8..11])?;
    if major != 3 || minor < 3 {
        return Err("the VM's console speaks an unsupported protocol version".into());
    }
    // Reply with the newest version this client shares with the server.
    let reply = match minor {
        3..=6 => 3,
        7 => 7,
        _ => 8,
    };
    write(stream, format!("RFB 003.{reply:03}\n").as_bytes())?;
    if reply == 3 {
        // Protocol 3.3: the server picks the security type and sends no result.
        return match u32::from_be_bytes(read_array(stream)?) {
            1 => Ok(()),
            2 => Err(PASSWORD_REFUSED.into()),
            0 => Err(refusal(stream)?),
            _ => Err("the VM's console offers no security type this client supports".into()),
        };
    }
    let count = read_array::<1>(stream)?[0];
    if count == 0 {
        return Err(refusal(stream)?);
    }
    let mut types = vec![0u8; count as usize];
    fill(stream, &mut types)?;
    if types.contains(&SECURITY_NONE) {
        write(stream, &[SECURITY_NONE])?;
    } else if types.contains(&SECURITY_VNC_AUTH) {
        return Err(PASSWORD_REFUSED.into());
    } else {
        return Err("the VM's console offers no security type this client supports".into());
    }
    if u32::from_be_bytes(read_array(stream)?) != 0 {
        return Err(refusal(stream)?);
    }
    Ok(())
}

fn server_init(stream: &mut impl Read) -> Result<(u32, u32), String> {
    let width = u32::from(u16::from_be_bytes(read_array(stream)?));
    let height = u32::from(u16::from_be_bytes(read_array(stream)?));
    let _pixel_format: [u8; 16] = read_array(stream)?;
    let name_len = u32::from_be_bytes(read_array(stream)?);
    if name_len > MAX_NAME {
        return Err("the VM's console name is too long".into());
    }
    let mut name = vec![0; name_len as usize];
    fill(stream, &mut name)?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err("the VM's screen size is unsupported".into());
    }
    Ok((width, height))
}

/// The server's refusal reason, read so the connection is consumed. The text is the server's own
/// refusal sentence and is bounded; it never includes anything Dial sent.
fn refusal(stream: &mut impl Read) -> Result<String, String> {
    let len = u32::from_be_bytes(read_array(stream)?);
    if len > MAX_REASON {
        return Ok("the VM refused the console connection".into());
    }
    let mut reason = vec![0; len as usize];
    fill(stream, &mut reason)?;
    let reason = String::from_utf8_lossy(&reason);
    Ok(format!(
        "the VM refused the console connection: {}",
        reason.trim().chars().take(200).collect::<String>()
    ))
}

/// Pixel format: 32 bpp, depth 24, little-endian, true colour, 8 bits per channel, red at bit 16.
fn set_pixel_format() -> Vec<u8> {
    let mut msg = vec![0u8; 20];
    msg[0] = 0;
    msg[4] = 32;
    msg[5] = 24;
    msg[6] = 0;
    msg[7] = 1;
    msg[8..10].copy_from_slice(&255u16.to_be_bytes());
    msg[10..12].copy_from_slice(&255u16.to_be_bytes());
    msg[12..14].copy_from_slice(&255u16.to_be_bytes());
    msg[14] = 16;
    msg[15] = 8;
    msg[16] = 0;
    msg
}

fn set_encodings() -> Vec<u8> {
    let mut msg = vec![2, 0];
    msg.extend_from_slice(&2u16.to_be_bytes());
    msg.extend_from_slice(&ENCODING_RAW.to_be_bytes());
    msg.extend_from_slice(&ENCODING_DESKTOP_SIZE.to_be_bytes());
    msg
}

fn digits(bytes: &[u8]) -> Result<u32, String> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "the VM's console sent an unknown greeting".to_owned())
}

fn transport(error: io::Error) -> String {
    match error.kind() {
        io::ErrorKind::UnexpectedEof => "the VM closed its console connection".into(),
        _ => "the VM's console connection failed".into(),
    }
}

fn write(stream: &mut impl Write, bytes: &[u8]) -> Result<(), String> {
    stream.write_all(bytes).map_err(transport)
}

fn fill(stream: &mut impl Read, buf: &mut [u8]) -> Result<(), String> {
    stream.read_exact(buf).map_err(transport)
}

fn read_array<const N: usize>(stream: &mut impl Read) -> Result<[u8; N], String> {
    let mut buf = [0u8; N];
    fill(stream, &mut buf)?;
    Ok(buf)
}

#[cfg(all(test, unix))]
pub(crate) mod tests;
