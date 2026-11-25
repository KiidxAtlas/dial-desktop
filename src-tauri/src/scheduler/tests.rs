use super::*;
use std::sync::mpsc;

fn quick() -> Timing {
    Timing {
        base_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(200),
        stable: Duration::from_secs(5),
        max_failures: 3,
        grace: Duration::from_secs(2),
        poll: Duration::from_millis(20),
    }
}

#[test]
fn backoff_doubles_up_to_the_cap() {
    let t = Timing::default();
    let delays: Vec<u64> = (1..=9).map(|n| backoff(&t, n).as_secs()).collect();
    assert_eq!(delays, [2, 4, 8, 16, 32, 64, 120, 120, 120]);
    assert_eq!(backoff(&t, u32::MAX), t.max_delay);
}

#[test]
fn exits_restart_reset_or_give_up() {
    let t = Timing::default();
    let quick_exit = Duration::from_secs(1);
    let cases = [
        // (failures before, ran, elsewhere, expected)
        (0, quick_exit, false, Next::Restart { failures: 1, delay: Duration::from_secs(2) }),
        (2, quick_exit, false, Next::Restart { failures: 3, delay: Duration::from_secs(8) }),
        // A healthy run starts the count over.
        (5, t.stable, false, Next::Restart { failures: 1, delay: Duration::from_secs(2) }),
    ];
    for (before, ran, elsewhere, expected) in cases {
        assert_eq!(after_exit(&t, before, ran, elsewhere), expected, "{before} {ran:?}");
    }
    assert!(matches!(
        after_exit(&t, t.max_failures, quick_exit, false),
        Next::GiveUp { phase: Phase::Failed, .. }
    ));
    // Another holder of the lease is never retried, however healthy the run was.
    assert!(matches!(
        after_exit(&t, 0, t.stable, true),
        Next::GiveUp { phase: Phase::Elsewhere, .. }
    ));
}

#[test]
fn log_keeps_the_last_lines_trimmed_and_bounded() {
    let mut log = Log::default();
    for i in 0..LOG_LINES + 5 {
        log.push(&format!("line {i}\r\n"));
    }
    let lines = log.lines();
    assert_eq!(lines.len(), LOG_LINES);
    assert_eq!(lines[0], "line 5");
    assert_eq!(lines.last().unwrap(), &format!("line {}", LOG_LINES + 4));
    log.push(&"é".repeat(LINE_CHARS + 10));
    let long = log.lines().pop().unwrap();
    assert_eq!(long.chars().count(), LINE_CHARS + 1);
    assert!(long.ends_with('…'));
}

#[test]
fn background_choice_persists_per_project() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nested/scheduler.json");
    let scheduler = Scheduler::new(Some(file.clone()), |_| {});
    assert_eq!(scheduler.status("/a").background, None);
    scheduler.set_background("/a", true).unwrap();
    scheduler.set_background("/b", false).unwrap();
    let reopened = Scheduler::new(Some(file), |_| {});
    assert_eq!(reopened.status("/a").background, Some(true));
    assert_eq!(reopened.status("/b").background, Some(false));
    assert_eq!(reopened.status("/c").background, None);
}

/// A fake engine: `sh schedule run` runs the script `schedule` in the project folder.
#[cfg(unix)]
fn project(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("schedule"), script).unwrap();
    dir
}

#[cfg(unix)]
fn watched() -> (Scheduler, mpsc::Receiver<Status>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let scheduler = Scheduler::with_timing(None, quick(), move |s| {
        let _ = tx.lock().unwrap().send(s);
    });
    (scheduler, rx)
}

#[cfg(unix)]
fn wait_for(rx: &mpsc::Receiver<Status>, what: impl Fn(&Status) -> bool) -> Status {
    loop {
        let status = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("scheduler event");
        if what(&status) {
            return status;
        }
    }
}

#[cfg(unix)]
#[test]
fn runs_without_a_terminal_and_stops_on_request() {
    let dir = project("trap 'echo bye; exit 0' INT\necho started\nwhile :; do sleep 0.05; done\n");
    let root = dir.path().to_str().unwrap();
    let (scheduler, rx) = watched();
    scheduler.start(root, Path::new("/bin/sh"));
    let running = wait_for(&rx, |s| s.phase == Phase::Running && s.pid.is_some());
    // Starting again while it runs never adds a second scheduler.
    let again = scheduler.start(root, Path::new("/bin/sh"));
    assert_eq!(again.pid, running.pid);
    let stopped = scheduler.stop(root, true);
    assert_eq!(stopped.phase, Phase::Stopped);
    assert_eq!(stopped.pid, None);
    // SIGINT let it finish on its own rather than being killed.
    assert_eq!(stopped.last_exit.as_ref().and_then(|e| e.code), Some(0));
    assert!(stopped.log.iter().any(|l| l == "started"), "{:?}", stopped.log);
}

#[cfg(unix)]
#[test]
fn restarts_with_backoff_then_gives_up() {
    let dir = project("echo crashed >&2\nexit 3\n");
    let root = dir.path().to_str().unwrap();
    let (scheduler, rx) = watched();
    scheduler.start(root, Path::new("/bin/sh"));
    let restarting = wait_for(&rx, |s| s.phase == Phase::Restarting);
    assert_eq!(restarting.last_exit.as_ref().and_then(|e| e.code), Some(3));
    assert!(restarting.retry_at_ms.is_some());
    let failed = wait_for(&rx, |s| s.phase == Phase::Failed);
    assert_eq!(failed.restarts, quick().max_failures);
    assert!(failed.log.iter().filter(|l| *l == "crashed").count() > 3);
}

#[cfg(unix)]
#[test]
fn another_scheduler_is_reported_not_retried() {
    let dir = project(&format!(
        "echo 'Error: another `dial schedule run` is {ELSEWHERE}' >&2\nexit 1\n"
    ));
    let root = dir.path().to_str().unwrap();
    let (scheduler, rx) = watched();
    scheduler.start(root, Path::new("/bin/sh"));
    let status = wait_for(&rx, |s| !matches!(s.phase, Phase::Running));
    assert_eq!(status.phase, Phase::Elsewhere);
    assert_eq!(status.restarts, 0);
}

#[cfg(unix)]
#[test]
fn opening_another_project_stops_the_runner_and_autostarts_by_choice() {
    let script = "trap 'exit 0' INT\nwhile :; do sleep 0.05; done\n";
    let (a, b) = (project(script), project(script));
    let (root_a, root_b) = (a.path().to_str().unwrap(), b.path().to_str().unwrap());
    let (scheduler, rx) = watched();
    let sh = Path::new("/bin/sh");
    scheduler.set_background(root_b, true).unwrap();
    scheduler.start(root_a, sh);
    wait_for(&rx, |s| s.root == root_a && s.phase == Phase::Running && s.pid.is_some());
    scheduler.project_opened(root_b, Some(sh));
    wait_for(&rx, |s| s.root == root_b && s.phase == Phase::Running && s.pid.is_some());
    assert_eq!(scheduler.status(root_a).phase, Phase::Stopped);
    // Without a choice (or in the design preview) nothing starts.
    scheduler.project_opened(root_a, Some(sh));
    assert_eq!(scheduler.status(root_a).phase, Phase::Stopped);
    scheduler.stop_all(true);
    assert_eq!(scheduler.status(root_b).phase, Phase::Stopped);
}
