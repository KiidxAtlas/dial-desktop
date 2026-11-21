//! Screen capture: size limits, the failure budget, and the live screen's damage tracking. Each
//! capture is compared with the one before it in 64-pixel tiles; the window then asks for what
//! changed since the frame it shows and gets only those rectangles, as raw RGBA.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use super::input::Frame;

/// Largest screendump the runner will read. A larger file is refused, not truncated.
pub const MAX_CAPTURE_BYTES: u64 = 64 * 1024 * 1024;
/// Consecutive failed captures before a run is marked failed (about five seconds at the default
/// interval), so a broken screen is shown rather than retried silently forever.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 10;

/// Read `path`, refusing anything over `limit` bytes.
pub fn read_capped(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    // One byte past the limit tells an over-large file apart from one exactly at it.
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e: io::Error| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("the screen image is larger than {limit} bytes"));
    }
    Ok(bytes)
}

/// Counts capture failures in a row.
#[derive(Debug, Default)]
pub struct Budget {
    failures: u32,
}

impl Budget {
    pub fn succeeded(&mut self) {
        self.failures = 0;
    }

    /// Record a failure; `true` once the run has used up its budget.
    pub fn failed(&mut self) -> bool {
        self.failures += 1;
        self.failures >= MAX_CONSECUTIVE_FAILURES
    }
}

/// Side of a damage tile, in pixels.
pub const TILE: u32 = 64;
/// How many recent frames' damage is kept; a window further behind gets the whole frame.
const HISTORY: usize = 120;
/// Size of a packet's header: seq (u64), width, height, rectangle count, flags (u32 each).
pub const PACKET_HEADER: usize = 24;
/// Header flag: the packet holds the whole frame (the window resets its canvas to the size).
pub const FLAG_FULL: u32 = 1;

/// The run's newest frame and which tiles each recent frame changed.
#[derive(Default)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
    pub frame: Option<Frame>,
    /// The oldest seq a window may name and still get a delta (a whole frame was recorded at it,
    /// or older damage was dropped).
    base: u64,
    /// One entry per seq after `base`, oldest first: that frame's changed tiles.
    history: VecDeque<(u64, Vec<bool>)>,
}

/// What a window gets for `since`: the frame and the tiles to send, or the whole frame.
pub struct Plan {
    seq: u64,
    frame: Frame,
    /// `None`: the whole frame.
    tiles: Option<Vec<bool>>,
}

impl Screen {
    /// Record `frame` if its pixels differ from the last one; `next_seq` numbers it. Returns its
    /// seq, or `None` for an unchanged screen.
    pub fn record(&mut self, frame: &Frame, next_seq: impl FnOnce() -> u64) -> Option<u64> {
        let changed = match &self.frame {
            Some(old) if old.width == frame.width && old.height == frame.height => {
                if std::sync::Arc::ptr_eq(&old.rgba, &frame.rgba) {
                    return None;
                }
                let tiles = changed_tiles(old, frame);
                if !tiles.contains(&true) {
                    return None;
                }
                Some(tiles)
            }
            _ => None,
        };
        let seq = next_seq();
        match changed {
            Some(tiles) => {
                self.history.push_back((seq, tiles));
                while self.history.len() > HISTORY {
                    if let Some((dropped, _)) = self.history.pop_front() {
                        self.base = dropped;
                    }
                }
            }
            None => {
                self.history.clear();
                self.base = seq;
            }
        }
        self.width = frame.width;
        self.height = frame.height;
        self.frame = Some(frame.clone());
        Some(seq)
    }

    /// What to send a window that shows `since` (`None`: nothing yet) when the newest is `seq`.
    /// `None` until there is a frame. Cheap: the pixels are shared, not copied.
    pub fn plan(&self, since: Option<u64>, seq: u64) -> Option<Plan> {
        let frame = self.frame.clone()?;
        let tiles = match since {
            Some(since) if since == seq => Some(vec![false; tile_count(&frame)]),
            Some(since) if since >= self.base && since < seq => {
                let mut union = vec![false; tile_count(&frame)];
                for (_, tiles) in self.history.iter().filter(|(at, _)| *at > since) {
                    for (u, t) in union.iter_mut().zip(tiles) {
                        *u |= *t;
                    }
                }
                Some(union)
            }
            _ => None,
        };
        Some(Plan { seq, frame, tiles })
    }
}

impl Plan {
    /// The packet: a little-endian header (seq u64; width, height, rectangle count, flags u32),
    /// then each rectangle as x, y, w, h (u32) and its RGBA rows. Changed tiles next to each other
    /// in a row of tiles go out as one rectangle.
    pub fn packet(&self) -> Vec<u8> {
        let f = &self.frame;
        let rects: Vec<(u32, u32, u32, u32)> = match &self.tiles {
            None => vec![(0, 0, f.width, f.height)],
            Some(tiles) => runs(f, tiles),
        };
        let area: usize = rects.iter().map(|r| r.2 as usize * r.3 as usize).sum();
        let mut out = Vec::with_capacity(PACKET_HEADER + rects.len() * 16 + area * 4);
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&f.width.to_le_bytes());
        out.extend_from_slice(&f.height.to_le_bytes());
        out.extend_from_slice(&(rects.len() as u32).to_le_bytes());
        let flags = if self.tiles.is_none() { FLAG_FULL } else { 0 };
        out.extend_from_slice(&flags.to_le_bytes());
        let stride = f.width as usize * 4;
        for (x, y, w, h) in rects {
            for v in [x, y, w, h] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for row in y as usize..(y + h) as usize {
                let start = row * stride + x as usize * 4;
                out.extend_from_slice(&f.rgba[start..start + w as usize * 4]);
            }
        }
        out
    }
}

fn tiles_across(frame: &Frame) -> usize {
    frame.width.div_ceil(TILE) as usize
}

fn tile_count(frame: &Frame) -> usize {
    tiles_across(frame) * frame.height.div_ceil(TILE) as usize
}

/// Which tiles differ between two frames of the same size. Each pixel row is compared once per
/// tile still unchanged, so a frame that changed everywhere stops comparing early.
fn changed_tiles(old: &Frame, new: &Frame) -> Vec<bool> {
    let across = tiles_across(new);
    let mut tiles = vec![false; tile_count(new)];
    let stride = new.width as usize * 4;
    let tile_bytes = TILE as usize * 4;
    for y in 0..new.height as usize {
        let row = &mut tiles[(y / TILE as usize) * across..][..across];
        let line = y * stride;
        for (tx, changed) in row.iter_mut().enumerate() {
            if *changed {
                continue;
            }
            let start = line + tx * tile_bytes;
            let end = (start + tile_bytes).min(line + stride);
            *changed = old.rgba[start..end] != new.rgba[start..end];
        }
    }
    tiles
}

/// Runs of changed tiles in each row of tiles, as pixel rectangles clipped to the frame.
fn runs(frame: &Frame, tiles: &[bool]) -> Vec<(u32, u32, u32, u32)> {
    let across = tiles_across(frame);
    let mut rects = Vec::new();
    for (ty, row) in tiles.chunks(across).enumerate() {
        let y = ty as u32 * TILE;
        let h = TILE.min(frame.height - y);
        let mut tx = 0;
        while tx < row.len() {
            if !row[tx] {
                tx += 1;
                continue;
            }
            let first = tx;
            while tx < row.len() && row[tx] {
                tx += 1;
            }
            let x = first as u32 * TILE;
            let w = (tx as u32 * TILE).min(frame.width) - x;
            rects.push((x, y, w, h));
        }
    }
    rects
}

#[cfg(test)]
mod tests;
