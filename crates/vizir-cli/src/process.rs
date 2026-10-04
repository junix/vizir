//! Bounded external-renderer execution. No background readers or global signal handlers.
use std::fmt;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const RENDER_TIMEOUT: Duration = Duration::from_secs(30);
const DIAGNOSTIC_LIMIT: usize = 64 * 1024;
const PIPE_GRACE: Duration = Duration::from_millis(250);
const CLEANUP_GRACE: Duration = Duration::from_millis(250);
const POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Default, Debug)]
struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
}

impl Capture {
    fn append(&mut self, bytes: &[u8]) {
        let retained = bytes.len().min(DIAGNOSTIC_LIMIT - self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..retained]);
        self.truncated |= retained < bytes.len();
    }

    fn text(&self) -> String {
        let mut text = String::from_utf8_lossy(&self.bytes).trim().to_owned();
        if self.truncated {
            text.push_str("\n[diagnostics truncated after 65536 bytes]");
        }
        text
    }
}

#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) status: ExitStatus,
    stdout: Capture,
    stderr: Capture,
}

impl Output {
    pub(crate) fn diagnostics(&self) -> String {
        diagnostics(&self.stdout, &self.stderr)
    }
}

fn diagnostics(stdout: &Capture, stderr: &Capture) -> String {
    let mut parts = Vec::new();
    if !stderr.bytes.is_empty() {
        parts.push(format!("stderr: {}", stderr.text()));
    }
    if !stdout.bytes.is_empty() {
        parts.push(format!("stdout: {}", stdout.text()));
    }
    parts.join("\n")
}

#[derive(Debug)]
pub(crate) struct Failure {
    message: String,
    pub(crate) missing: bool,
    stdout: Capture,
    stderr: Capture,
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)?;
        let diagnostics = diagnostics(&self.stdout, &self.stderr);
        if !diagnostics.is_empty() {
            write!(formatter, "\n{diagnostics}")?;
        }
        Ok(())
    }
}

/// The deadline covers child completion AND pipe EOF, not just `try_wait`.
/// On failure only this direct child is killed; descendants are never signaled.
/// Cleanup and post-exit pipe draining are bounded, including inherited pipes.
pub(crate) fn run(command: &mut Command, timeout: Duration) -> Result<Output, Failure> {
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Failure {
            message: format!("could not start: {error}"),
            missing: error.kind() == io::ErrorKind::NotFound,
            stdout: Capture::default(),
            stderr: Capture::default(),
        })?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let mut out = Capture::default();
    let mut err = Capture::default();
    let mut status = None;
    let mut exited = None;
    let result = (|| -> Result<ExitStatus, String> {
        // Only our read endpoints are made nonblocking; the child's write
        // endpoints retain their normal blocking semantics.
        prepare(stdout.as_ref().expect("piped stdout"))
            .and_then(|()| prepare(stderr.as_ref().expect("piped stderr")))
            .map_err(|error| format!("could not prepare diagnostic capture: {error}"))?;
        loop {
            if started.elapsed() >= timeout {
                return Err(format!("timed out after {} ms", timeout.as_millis()));
            }
            // Bounded work per stream prevents a continuous flood from starving
            // stderr, cancellation, or the deadline check.
            drain(&mut stdout, &mut out)
                .and_then(|()| drain(&mut stderr, &mut err))
                .map_err(|error| format!("could not capture diagnostics: {error}"))?;
            if status.is_none() {
                status = child
                    .try_wait()
                    .map_err(|error| format!("could not wait: {error}"))?;
                if status.is_some() {
                    exited = Some(Instant::now());
                }
            }
            if started.elapsed() >= timeout {
                return Err(format!("timed out after {} ms", timeout.as_millis()));
            }
            if let Some(status) = status {
                if stdout.is_none() && stderr.is_none() {
                    return Ok(status);
                }
                if exited.is_some_and(|time| time.elapsed() >= PIPE_GRACE) {
                    return Err("diagnostic pipes remained open 250 ms after child exit".to_owned());
                }
            }
            std::thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
        }
    })();
    match result {
        Ok(status) => Ok(Output {
            status,
            stdout: out,
            stderr: err,
        }),
        Err(mut message) => {
            // Close capture endpoints even if a descendant still owns a write
            // endpoint. No reader thread remains blocked or gets detached.
            drop(stdout);
            drop(stderr);
            if status.is_none()
                && let Some(note) = cancel(&mut child)
            {
                message.push_str(&format!("; {note}"));
            }
            Err(Failure {
                message,
                missing: false,
                stdout: out,
                stderr: err,
            })
        }
    }
}

fn cancel(child: &mut Child) -> Option<String> {
    match child.try_wait() {
        Ok(Some(_)) => return None,
        Ok(None) => {}
        Err(error) => return Some(format!("could not inspect child during cleanup: {error}")),
    }
    if let Err(error) = child.kill() {
        return Some(format!("could not cancel direct child: {error}"));
    }
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return None,
            Ok(None) if started.elapsed() < CLEANUP_GRACE => std::thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                return Some("direct child not reaped within 250 ms of cancellation".to_owned());
            }
            Err(error) => return Some(format!("could not reap direct child: {error}")),
        }
    }
}

#[cfg(unix)]
trait Pipe: Read + std::os::fd::AsFd {}
#[cfg(unix)]
impl<T: Read + std::os::fd::AsFd> Pipe for T {}
#[cfg(windows)]
trait Pipe: Read + std::os::windows::io::AsRawHandle {}
#[cfg(windows)]
impl<T: Read + std::os::windows::io::AsRawHandle> Pipe for T {}
#[cfg(not(any(unix, windows)))]
trait Pipe: Read {}
#[cfg(not(any(unix, windows)))]
impl<T: Read> Pipe for T {}

#[cfg(unix)]
fn prepare(pipe: &impl Pipe) -> io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(pipe)?;
    rustix::fs::fcntl_setfl(pipe, flags | rustix::fs::OFlags::NONBLOCK)?;
    Ok(())
}
#[cfg(windows)]
fn prepare(_pipe: &impl Pipe) -> io::Result<()> {
    Ok(())
}
#[cfg(not(any(unix, windows)))]
fn prepare(_pipe: &impl Pipe) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "bounded renderer capture needs Unix or Windows",
    ))
}

fn drain(pipe: &mut Option<impl Pipe>, capture: &mut Capture) -> io::Result<()> {
    let Some(reader) = pipe.as_mut() else {
        return Ok(());
    };
    let mut buffer = [0; 8192];
    for _ in 0..8 {
        match read_available(reader, &mut buffer) {
            Ok(0) => {
                *pipe = None;
                return Ok(());
            }
            Ok(count) => capture.append(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn read_available(pipe: &mut impl Pipe, buffer: &mut [u8]) -> io::Result<usize> {
    pipe.read(buffer)
}

#[cfg(windows)]
fn read_available(pipe: &mut impl Pipe, buffer: &mut [u8]) -> io::Result<usize> {
    use windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    let mut available = 0;
    // SAFETY: this owned ChildStdout/ChildStderr supplies a live pipe handle;
    // only this function reads it. Peek writes into the valid `available`
    // pointer; the unused output pointers are null as permitted by the API.
    let result = unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
            Ok(0)
        } else {
            Err(error)
        };
    }
    if available == 0 {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    // This is the only reader, so the bytes Peek observed cannot be consumed
    // elsewhere; asking for no more than that count cannot wait for more data.
    let count = buffer.len().min(available as usize);
    pipe.read(&mut buffer[..count])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture_command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "process::tests::renderer_fixture",
            "--nocapture",
        ]);
        command.env("VIZIR_PROCESS_TEST_MODE", mode);
        command
    }

    #[test]
    #[ignore = "subprocess fixture, invoked only by bounded-runner tests"]
    fn renderer_fixture() {
        match std::env::var("VIZIR_PROCESS_TEST_MODE").unwrap().as_str() {
            "success" => {
                println!("renderer version 1");
                eprintln!("renderer warning");
            }
            "stdin" => {
                let mut bytes = Vec::new();
                std::io::stdin().read_to_end(&mut bytes).unwrap();
                assert!(bytes.is_empty());
            }
            "hang" => std::thread::sleep(Duration::from_secs(10)),
            "flood" | "infinite-flood" => {
                let infinite =
                    std::env::var("VIZIR_PROCESS_TEST_MODE").unwrap() == "infinite-flood";
                for _ in 0..256 {
                    std::io::stdout().write_all(&[b'o'; 8192]).unwrap();
                    std::io::stderr().write_all(&[b'e'; 8192]).unwrap();
                }
                // A failsafe also bounds this fixture if deadline handling
                // regresses; it is continuously noisy until then.
                let started = Instant::now();
                while infinite && started.elapsed() < Duration::from_secs(5) {
                    std::io::stdout().write_all(&[b'o'; 8192]).unwrap();
                    std::io::stderr().write_all(&[b'e'; 8192]).unwrap();
                }
            }
            "descendant" => {
                // The direct child must exit first to exercise inherited pipe
                // ownership. This short-lived descendant self-exits below.
                #[allow(clippy::zombie_processes)]
                let _descendant = fixture_command("hold-pipes").spawn().unwrap();
                // This controlled descendant self-exits shortly after the
                // capture grace. It intentionally inherits both pipe handles.
            }
            "hold-pipes" => std::thread::sleep(Duration::from_millis(800)),
            "failed" => {
                eprintln!("controlled failure");
                std::process::exit(17);
            }
            mode => panic!("unknown renderer fixture {mode}"),
        }
    }

    #[test]
    fn captures_both_streams_and_preserves_success() {
        let output = run(&mut fixture_command("success"), Duration::from_secs(3)).unwrap();
        assert!(output.status.success());
        assert!(output.diagnostics().contains("renderer version 1"));
        assert!(output.diagnostics().contains("renderer warning"));
    }

    #[test]
    fn child_stdin_is_closed() {
        let output = run(&mut fixture_command("stdin"), Duration::from_secs(3)).unwrap();
        assert!(output.status.success(), "{}", output.diagnostics());
    }

    #[test]
    fn missing_binary_is_identified_without_waiting() {
        let directory = tempfile::tempdir().unwrap();
        let failure = run(
            &mut Command::new(directory.path().join("missing")),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(failure.missing);
        assert!(failure.to_string().contains("could not start"));
    }

    #[test]
    fn failed_status_and_diagnostics_are_preserved() {
        let output = run(&mut fixture_command("failed"), Duration::from_secs(3)).unwrap();
        assert_eq!(output.status.code(), Some(17));
        assert!(output.diagnostics().contains("controlled failure"));
    }

    #[test]
    fn finite_flood_is_drained_with_bounded_retention() {
        let output = run(&mut fixture_command("flood"), Duration::from_secs(5)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.bytes.len(), DIAGNOSTIC_LIMIT);
        assert_eq!(output.stderr.bytes.len(), DIAGNOSTIC_LIMIT);
        assert!(output.stdout.truncated && output.stderr.truncated);
        assert!(output.diagnostics().contains("diagnostics truncated"));
        assert!(output.diagnostics().len() <= 2 * DIAGNOSTIC_LIMIT + 200);
    }

    #[test]
    fn hung_and_continuously_noisy_children_cannot_starve_deadline() {
        for mode in ["hang", "infinite-flood"] {
            let started = Instant::now();
            let failure = run(&mut fixture_command(mode), Duration::from_millis(120)).unwrap_err();
            assert!(failure.to_string().contains("timed out after 120 ms"));
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{mode} exceeded cleanup bound"
            );
            assert!(failure.stdout.bytes.len() <= DIAGNOSTIC_LIMIT);
            assert!(failure.stderr.bytes.len() <= DIAGNOSTIC_LIMIT);
        }
    }

    #[test]
    fn direct_child_is_reaped_after_cancellation() {
        let mut child = fixture_command("hang")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(cancel(&mut child).is_none());
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn descendant_held_pipes_fail_instead_of_hanging_or_reporting_success() {
        let started = Instant::now();
        let failure = run(&mut fixture_command("descendant"), Duration::from_secs(3)).unwrap_err();
        assert!(
            failure.to_string().contains("pipes remained open"),
            "{failure}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn overall_deadline_also_bounds_post_exit_capture() {
        let started = Instant::now();
        let failure = run(
            &mut fixture_command("descendant"),
            Duration::from_millis(120),
        )
        .unwrap_err();
        assert!(
            failure.to_string().contains("timed out after 120 ms"),
            "{failure}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn closed_pipes_do_not_hide_a_hung_child() {
        let failure = run(
            Command::new("/bin/sh").args(["-c", "exec 1>&- 2>&-; exec /bin/sleep 5"]),
            Duration::from_millis(120),
        )
        .unwrap_err();
        assert!(failure.to_string().contains("timed out after 120 ms"));
    }
}
