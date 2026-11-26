//! Bounded capture for Desktop's noninteractive commands. Descendants may inherit either output
//! pipe: drain available bytes while the direct child runs, but never wait for EOF.

use std::io::{self, Read};
#[cfg(unix)]
use std::os::fd::AsRawFd as PipeHandle;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle as PipeHandle;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const MAX_OUTPUT: u64 = 8 * 1024 * 1024;

pub(super) fn run_detailed(command: &mut Command, timeout: Duration) -> Result<Vec<u8>, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe between fork and exec. Isolate even interactive
        // login shells from the caller's controlling terminal and process group.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
    let output = run_configured(
        command.stderr(Stdio::null()),
        timeout,
        MAX_OUTPUT,
        |error| format!("could not start: {error}"),
    )?;
    if !output.status.success() {
        return Err(format!("exited with {}", output.status));
    }
    Ok(output.stdout)
}

pub(crate) fn run_captured(
    command: &mut Command,
    timeout: Duration,
    max_output: u64,
    spawn_error: impl FnOnce(io::Error) -> String,
) -> Result<Output, String> {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(command, 0);
    run_configured(
        command.stderr(Stdio::piped()),
        timeout,
        max_output,
        spawn_error,
    )
}

/// The caller sets stderr and, on Unix, puts the child in its own process group/session.
fn run_configured(
    command: &mut Command,
    timeout: Duration,
    max_output: u64,
    spawn_error: impl FnOnce(io::Error) -> String,
) -> Result<Output, String> {
    let mut child = super::without_console(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(spawn_error)?;
    let result = capture(&mut child, timeout, max_output);
    if result.is_err() && matches!(child.try_wait(), Ok(None)) {
        #[cfg(unix)]
        {
            // SAFETY: the child leads its own process group and its PID cannot be recycled
            // before we reap it. Never signal this group after reaping the leader.
            unsafe { libc::killpg(child.id() as libc::pid_t, libc::SIGKILL) };
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn capture(child: &mut Child, timeout: Duration, limit: u64) -> Result<Output, String> {
    let start = Instant::now();
    let mut stdout = child.stdout.take().ok_or("has no output")?;
    let mut stderr = child.stderr.take();
    let mut out = Vec::new();
    let mut err = Vec::new();
    loop {
        if start.elapsed() >= timeout {
            return Err(format!("timed out after {} s", timeout.as_secs()));
        }
        let status = child
            .try_wait()
            .map_err(|error| format!("could not be waited on: {error}"))?;
        // Sole reader. The snapshot taken after exit includes all the direct child's writes.
        let read_error = |e| format!("output could not be read: {e}");
        let ready = drain(&mut stdout, &mut out, limit).map_err(read_error)?;
        let err_ready = stderr
            .as_mut()
            .map_or(Ok(0), |p| drain(p, &mut err, limit))
            .map_err(read_error)?;
        if let Some(status) = status {
            return Ok(Output {
                status,
                stdout: out,
                stderr: err,
            });
        }
        if ready == 0 && err_ready == 0 {
            std::thread::sleep(
                Duration::from_millis(5).min(timeout.saturating_sub(start.elapsed())),
            );
        }
    }
}

fn drain(pipe: &mut (impl Read + PipeHandle), buf: &mut Vec<u8>, limit: u64) -> io::Result<u64> {
    let ready = available(pipe)?;
    if ready > limit.saturating_sub(buf.len() as u64) {
        return Err(io::Error::other(format!("output exceeded {limit} bytes")));
    }
    pipe.take(ready).read_to_end(buf)?;
    Ok(ready)
}

#[cfg(unix)]
pub(crate) fn available(pipe: &impl PipeHandle) -> io::Result<u64> {
    let mut count: libc::c_int = 0;
    // SAFETY: the pipe remains open for the call; FIONREAD writes one int to count.
    if unsafe { libc::ioctl(pipe.as_raw_fd(), libc::FIONREAD, &mut count) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(count as u64)
}

#[cfg(windows)]
pub(crate) fn available(pipe: &impl PipeHandle) -> io::Result<u64> {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            pipe: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }
    let mut count = 0;
    // SAFETY: PeekNamedPipe supports anonymous pipes. We request only the available byte count.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            null_mut(),
            0,
            null_mut(),
            &mut count,
            null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::BrokenPipe {
            return Err(error);
        }
    }
    Ok(u64::from(count))
}
