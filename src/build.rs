use crate::project::Package;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum Outcome {
    Success,
    Failed(String),
    Cancelled,
}

pub struct BuildResult {
    pub name: &'static str,
    pub outcome: Outcome,
    pub elapsed: Duration,
    pub log: PathBuf,
}

impl BuildResult {
    pub fn succeeded(&self) -> bool {
        matches!(self.outcome, Outcome::Success)
    }
}

pub fn run(
    packages: &[Package],
    logs: &Path,
    cancelled: &AtomicBool,
    mut completed: impl FnMut(usize, &BuildResult),
) -> Vec<BuildResult> {
    std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        for (index, package) in packages.iter().enumerate() {
            let sender = sender.clone();
            scope.spawn(move || {
                let result = build_package(package, logs, cancelled);
                let _ = sender.send((index, result));
            });
        }
        drop(sender);
        let mut results = Vec::new();
        for (index, result) in receiver {
            completed(index, &result);
            results.push(result);
        }
        results
    })
}

fn build_package(package: &Package, logs: &Path, cancelled: &AtomicBool) -> BuildResult {
    let started = Instant::now();
    let log = logs.join(format!("{}.log", package.language.name()));
    let outcome = match execute(package, &log, cancelled) {
        Ok(outcome) => outcome,
        // TODO: Include execution I/O errors in the package log when it is still writable.
        Err(error) => Outcome::Failed(error.to_string()),
    };
    BuildResult {
        name: package.language.name(),
        outcome,
        elapsed: started.elapsed(),
        log,
    }
}

fn execute(package: &Package, log: &Path, cancelled: &AtomicBool) -> io::Result<Outcome> {
    let mut output = File::create(log)?;
    let (program, args) = package.language.command();
    writeln!(
        output,
        "$ {program} {}\nWorking directory: {}\n",
        args.join(" "),
        package.directory.display()
    )?;
    if cancelled.load(Ordering::Relaxed) {
        return Ok(Outcome::Cancelled);
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&package.directory)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output.try_clone()?);
    // A separate process group lets cancellation also stop compilers launched by a script.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let message = format!("Could not start {program}: {error}");
            writeln!(output, "{message}")?;
            return Ok(Outcome::Failed(message));
        }
    };
    loop {
        if cancelled.load(Ordering::Relaxed) {
            stop(&mut child)?;
            writeln!(output, "\nBuild cancelled")?;
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
