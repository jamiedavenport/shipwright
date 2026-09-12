use crate::project::Package;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
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
    let outcome = if cancelled.load(Ordering::Relaxed) {
        Outcome::Cancelled
    } else {
        outcome
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
    if package.language == crate::project::Language::Go {
        let files = match go_binaries(package, cancelled, true) {
            Ok(files) => files,
            Err(error) => {
                writeln!(output, "{error}")?;
                return Ok(Outcome::Failed(error.to_string()));
            }
        };
        if cancelled.load(Ordering::Relaxed) {
            return Ok(Outcome::Cancelled);
        }
        if !files.is_empty() {
            return Ok(Outcome::Success);
        }
        // Library-only modules still receive the ordinary build check below.
    }
    let mut command = Command::new(program);
    command.args(args).current_dir(&package.directory);
    let result = capture(&mut command, cancelled)?;
    output.write_all(&result.stdout)?;
    output.write_all(&result.stderr)?;
    Ok(result.outcome)
}

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

pub fn go_binaries(
    package: &Package,
    cancelled: &AtomicBool,
    compile: bool,
) -> io::Result<Vec<PathBuf>> {
    let mut list = Command::new("go");
    list.args([
        "list",
        "-f",
        "{{if eq .Name \"main\"}}{{.ImportPath}}\t{{.Target}}{{end}}",
        "./...",
    ])
    .current_dir(&package.directory);
    // Match the host archive label, even when the caller configured cross compilation.
    let go_os = match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    };
    let go_arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        arch => arch,
    };
    list.env("GOOS", go_os).env("GOARCH", go_arch);
    let output = capture(&mut list, cancelled)?;
    if !matches!(output.outcome, Outcome::Success) {
        return Err(io::Error::other(format!(
            "go list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let mut files = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
    {
        let (import, target) = line
            .split_once('\t')
            .ok_or_else(|| io::Error::other("Invalid go list output"))?;
        let name = Path::new(target)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| import.rsplit('/').next().unwrap());
        let name = if cfg!(windows) && !name.ends_with(".exe") {
            format!("{name}.exe")
        } else {
            name.to_owned()
        };
        let path = package.directory.join("bin").join(name);
        if files.contains(&path) {
            return Err(io::Error::other(
                "Go main packages have colliding executable names",
            ));
        }
        if compile {
            std::fs::create_dir_all(package.directory.join("bin"))?;
            let output = capture(
                Command::new("go")
                    .args(["build", "-o"])
                    .arg(&path)
                    .arg(import)
                    .env("GOOS", go_os)
                    .env("GOARCH", go_arch)
                    .current_dir(&package.directory),
                cancelled,
            )?;
            if !matches!(output.outcome, Outcome::Success) {
                return Err(io::Error::other(format!(
                    "go build failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
        }
        files.push(path);
    }
    Ok(files)
}
