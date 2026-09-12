use crate::{process::capture, project::Package, runner::Outcome};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

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
