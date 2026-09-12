use crate::runner::Outcome;
use std::io::{self, Read, Seek};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct Captured {
    pub outcome: Outcome,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

// Anonymous temporary files avoid pipe deadlocks and retaining authentication output.
pub fn capture(command: &mut Command, cancelled: &AtomicBool) -> io::Result<Captured> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    let outcome = wait_command(command, cancelled)?;
    stdout.rewind()?;
    stderr.rewind()?;
    let mut result = Captured {
        outcome,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    stdout.read_to_end(&mut result.stdout)?;
    stderr.read_to_end(&mut result.stderr)?;
    Ok(result)
}

fn wait_command(command: &mut Command, cancelled: &AtomicBool) -> io::Result<Outcome> {
    if cancelled.load(Ordering::Relaxed) {
        return Ok(Outcome::Cancelled);
    }
    // A separate process group lets cancellation also stop compilers launched by a script.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let message = format!("Could not start command: {error}");
            return Ok(Outcome::Failed(message));
        }
    };
    loop {
        if cancelled.load(Ordering::Relaxed) {
            stop(&mut child)?;

            return Ok(Outcome::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(if status.success() {
                    Outcome::Success
                } else {
                    Outcome::Failed(status.to_string())
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = stop(&mut child);
                return Err(error);
            }
        }
    }
}

fn stop(child: &mut Child) -> io::Result<()> {
    #[cfg(unix)]
    {
        use nix::{
            sys::signal::{Signal, killpg},
            unistd::Pid,
        };
        // TODO: Allow a brief graceful shutdown before forcibly killing the process group.
        match killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(error) => return Err(io::Error::from(error)),
        }
    }
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            child.kill()?;
        }
    }
    #[cfg(not(any(unix, windows)))]
    child.kill()?;
    child.wait()?;
    Ok(())
}
