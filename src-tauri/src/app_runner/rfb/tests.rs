//! The viewer over a Unix socket pair, against a scripted RFB server (the QEMU display path).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::{Session, Viewer};

/// A scripted server: handshake (3.8, security None), a `w`x`h` screen, then one reply per
/// FramebufferUpdateRequest from `replies`. Reports each request's incremental flag. With
/// `close_when_done`, a request after the last reply closes the socket (QEMU exiting).
pub(crate) fn server(
    mut s: UnixStream,
    w: u16,
    h: u16,
    replies: Vec<Vec<u8>>,
    requests: mpsc::Sender<bool>,
    close_when_done: bool,
) {
    s.write_all(b"RFB 003.008\n").unwrap();
    let mut version = [0u8; 12];
    s.read_exact(&mut version).unwrap();
    s.write_all(&[1, 1]).unwrap();
    let mut chosen = [0u8; 1];
    s.read_exact(&mut chosen).unwrap();
    s.write_all(&0u32.to_be_bytes()).unwrap();
    let mut init = [0u8; 1];
    s.read_exact(&mut init).unwrap();
    let mut msg = Vec::new();
    msg.extend_from_slice(&w.to_be_bytes());
    msg.extend_from_slice(&h.to_be_bytes());
    msg.extend_from_slice(&[32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0]);
    msg.extend_from_slice(&4u32.to_be_bytes());
    msg.extend_from_slice(b"qemu");
    s.write_all(&msg).unwrap();
    let mut replies = replies.into_iter();
    loop {
        let mut kind = [0u8; 1];
        if s.read_exact(&mut kind).is_err() {
            return;
        }
        match kind[0] {
            0 => s.read_exact(&mut [0u8; 19]).unwrap(),
            2 => {
                let mut head = [0u8; 3];
                s.read_exact(&mut head).unwrap();
                let n = usize::from(u16::from_be_bytes([head[1], head[2]]));
                s.read_exact(&mut vec![0u8; n * 4]).unwrap();
            }
            3 => {
                let mut b = [0u8; 9];
                s.read_exact(&mut b).unwrap();
                let _ = requests.send(b[0] != 0);
                match replies.next() {
                    Some(reply) => s.write_all(&reply).unwrap(),
                    None if close_when_done => return,
                    None => {}
                }
            }
            _ => return,
        }
    }
}

/// One FramebufferUpdate with Raw rectangles `(x, y, w, h, bgrx)`.
pub(crate) fn update(rects: &[(u16, u16, u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut msg = vec![0, 0];
    msg.extend_from_slice(&(rects.len() as u16).to_be_bytes());
    for (x, y, w, h, data) in rects {
        for v in [x, y, w, h] {
            msg.extend_from_slice(&v.to_be_bytes());
        }
        msg.extend_from_slice(&0i32.to_be_bytes());
        msg.extend_from_slice(data);
    }
    msg
}

pub(crate) fn bgrx(n: usize, b: u8, g: u8, r: u8) -> Vec<u8> {
    (0..n).flat_map(|_| [b, g, r, 0]).collect()
}

fn viewer(w: u16, h: u16, replies: Vec<Vec<u8>>) -> (Viewer<UnixStream>, mpsc::Receiver<bool>) {
    let (client, server_end) = UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || server(server_end, w, h, replies, tx, false));
    (Viewer::new(Session::connect(client).unwrap()), rx)
}

fn pixel(frame: &super::Frame, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * frame.width + x) * 4) as usize;
    frame.rgba[at..at + 4].try_into().unwrap()
}

#[test]
fn rectangles_patch_only_their_pixels_and_convert_to_rgba() {
    let (mut viewer, requests) = viewer(
        3,
        2,
        vec![
            update(&[(0, 0, 3, 2, bgrx(6, 10, 20, 30))]),
            update(&[(2, 1, 1, 1, bgrx(1, 1, 2, 3))]),
        ],
    );
    let first = viewer.frame(Duration::ZERO, Duration::from_secs(2)).unwrap();
    assert_eq!((first.width, first.height), (3, 2));
    assert_eq!(pixel(&first, 0, 0), [30, 20, 10, 255]);
    let second = viewer.frame(Duration::from_secs(2), Duration::ZERO).unwrap();
    assert_eq!(pixel(&second, 2, 1), [3, 2, 1, 255], "the changed pixel");
    for (x, y) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1)] {
        assert_eq!(pixel(&second, x, y), [30, 20, 10, 255], "untouched pixel {x},{y}");
    }
    // The first frame is not altered by the later update.
    assert_eq!(pixel(&first, 2, 1), [30, 20, 10, 255]);
    // The first request asks for everything; each later one only for changes, and the next request
    // is sent as soon as a reply arrives (pipelined), before the caller polls again.
    let flags: Vec<bool> = requests.iter().take(3).collect();
    assert_eq!(flags, [false, true, true]);
}

#[test]
fn a_poll_without_a_change_returns_the_same_frame_without_waiting() {
    let (mut viewer, _requests) = viewer(2, 1, vec![update(&[(0, 0, 2, 1, bgrx(2, 0, 0, 9))])]);
    let first = viewer.frame(Duration::ZERO, Duration::from_secs(2)).unwrap();
    let started = Instant::now();
    let again = viewer.frame(Duration::ZERO, Duration::from_secs(2)).unwrap();
    assert!(started.elapsed() < Duration::from_millis(200), "a zero-wait poll must not block");
    assert!(std::sync::Arc::ptr_eq(&first.rgba, &again.rgba));
}

#[test]
fn a_resize_reallocates_and_asks_for_the_whole_screen_again() {
    let mut resize = vec![0, 0];
    resize.extend_from_slice(&2u16.to_be_bytes());
    for v in [0u16, 0, 1, 1] {
        resize.extend_from_slice(&v.to_be_bytes());
    }
    resize.extend_from_slice(&(-223i32).to_be_bytes());
    for v in [0u16, 0, 1, 1] {
        resize.extend_from_slice(&v.to_be_bytes());
    }
    resize.extend_from_slice(&0i32.to_be_bytes());
    resize.extend_from_slice(&bgrx(1, 5, 6, 7));
    let (mut viewer, requests) = viewer(
        2,
        2,
        vec![update(&[(0, 0, 2, 2, bgrx(4, 0, 0, 0))]), resize],
    );
    viewer.frame(Duration::ZERO, Duration::from_secs(2)).unwrap();
    let resized = viewer.frame(Duration::from_secs(2), Duration::ZERO).unwrap();
    assert_eq!((resized.width, resized.height), (1, 1));
    assert_eq!(pixel(&resized, 0, 0), [7, 6, 5, 255]);
    assert_eq!(viewer.size(), (1, 1));
    let flags: Vec<bool> = requests.iter().take(3).collect();
    assert_eq!(flags, [false, true, false], "after a resize the next request is full");
}

#[test]
fn a_closed_display_is_an_error_not_a_stale_frame() {
    let (client, server_end) = UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel();
    let replies = vec![update(&[(0, 0, 1, 1, bgrx(1, 0, 0, 0))])];
    let server = thread::spawn(move || server(server_end, 1, 1, replies, tx, true));
    let mut viewer = Viewer::new(Session::connect(client).unwrap());
    viewer.frame(Duration::ZERO, Duration::from_secs(2)).unwrap();
    // QEMU exits: its end of the socket closes once the pipelined request has been read.
    assert_eq!(rx.iter().take(2).count(), 2);
    server.join().unwrap();
    let error = viewer
        .frame(Duration::from_secs(2), Duration::ZERO)
        .expect_err("a closed display must not keep serving the last frame");
    assert!(error.contains("console connection"), "{error}");
}
