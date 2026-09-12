use crate::{
    build,
    operation::Operation,
    process::capture,
    project::{Language, Package},
};
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
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

pub struct PackageResult {
    pub name: &'static str,
    pub outcome: Outcome,
    pub elapsed: Duration,
    pub log: PathBuf,
}

impl PackageResult {
    pub fn succeeded(&self) -> bool {
        matches!(self.outcome, Outcome::Success)
    }
}

pub fn run(
    packages: &[Package],
    operation: Operation,
    logs: &Path,
    cancelled: &AtomicBool,
    mut completed: impl FnMut(usize, &PackageResult),
) -> Vec<PackageResult> {
    std::thread::scope(|scope| {
        let (sender, receiver) = mpsc::channel();
        for (index, package) in packages.iter().enumerate() {
            let sender = sender.clone();
            scope.spawn(move || {
                let result = run_package(package, operation, logs, cancelled);
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

fn run_package(
    package: &Package,
    operation: Operation,
    logs: &Path,
    cancelled: &AtomicBool,
) -> PackageResult {
    let started = Instant::now();
    let log = logs.join(format!("{}.log", package.language.name()));
    let outcome = match execute(package, operation, &log, cancelled) {
        Ok(outcome) => outcome,
        // TODO: Include execution I/O errors in the package log when it is still writable.
        Err(error) => Outcome::Failed(error.to_string()),
    };
    let outcome = if cancelled.load(Ordering::Relaxed) {
        Outcome::Cancelled
    } else {
        outcome
    };
    PackageResult {
        name: package.language.name(),
        outcome,
        elapsed: started.elapsed(),
        log,
    }
}

fn execute(
    package: &Package,
    operation: Operation,
    log: &Path,
    cancelled: &AtomicBool,
) -> io::Result<Outcome> {
    let mut output = File::create(log)?;
    let (program, args) = operation.command(package.language);
    writeln!(
        output,
        "$ {program} {}\nWorking directory: {}\n",
        args.join(" "),
        package.directory.display()
    )?;
    if cancelled.load(Ordering::Relaxed) {
        return Ok(Outcome::Cancelled);
    }
    if package.language == Language::Go && operation == Operation::Build {
        let files = match build::go_binaries(package, cancelled, true) {
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
    if package.language == Language::Go
        && operation == (Operation::Format { fix: false })
        && matches!(result.outcome, Outcome::Success)
        && !result.stdout.is_empty()
    {
        return Ok(Outcome::Failed(
            "Files need formatting; run shipwright format go --fix".into(),
        ));
    }
    Ok(result.outcome)
}
