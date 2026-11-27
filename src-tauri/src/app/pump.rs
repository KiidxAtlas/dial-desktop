//! Leading-edge, trailing-coalesced frame scheduler for the Desktop UI pump.
//!
//! When the pump has been idle for at least `frame` (16 ms), the very next wakeup publishes
//! immediately (0 ms synthetic delay for discrete clicks and RPC replies). Rapid bursts while
//! streaming coalesce to at most one publish per `frame`, never dropping the trailing update.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn pump_loop(
    wake: &Receiver<()>,
    frame: Duration,
    tick: Duration,
    mut publish: impl FnMut(),
) {
    let mut last_publish: Option<Instant> = None;
    loop {
        match wake.recv_timeout(tick) {
            Ok(()) => {
                while wake.try_recv().is_ok() {}
                if let Some(prev) = last_publish {
                    let elapsed = prev.elapsed();
                    if elapsed < frame {
                        thread::sleep(frame - elapsed);
                        while wake.try_recv().is_ok() {}
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        publish();
        last_publish = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, mpsc};

    #[test]
    fn leading_edge_publishes_immediately_and_bursts_coalesce() {
        let (tx, rx) = mpsc::channel();
        let times: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&times);
        let frame = Duration::from_millis(60);
        let tick = Duration::from_secs(5);
        let handle = thread::spawn(move || {
            pump_loop(&rx, frame, tick, || {
                recorded.lock().unwrap().push(Instant::now());
            });
        });

        // First wake from idle should publish immediately (leading edge), not after `frame`.
        let t0 = Instant::now();
        tx.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_millis(200);
        while times.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        let first_latency = times.lock().unwrap()[0].saturating_duration_since(t0);
        assert!(
            first_latency < Duration::from_millis(45),
            "leading-edge wake should not wait a full frame ({frame:?}), took {first_latency:?}"
        );

        // Rapid burst within `frame` coalesces into a single trailing publish.
        for _ in 0..10 {
            tx.send(()).unwrap();
        }
        thread::sleep(Duration::from_millis(120));
        drop(tx);
        handle.join().unwrap();

        let published = times.lock().unwrap();
        assert_eq!(published.len(), 2, "burst should coalesce to 1 trailing publish, got {}", published.len());
        let gap = published[1].saturating_duration_since(published[0]);
        assert!(
            gap >= Duration::from_millis(50),
            "trailing publish should respect frame spacing, gap was {gap:?}"
        );
    }
}
