use super::{
    Context, Result, Selected, err,
    git::{clean, git},
    http::http,
    manifest::identity,
};
use crate::project;
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub(super) fn select<'a>(
    start: &Path,
    selected: Option<&str>,
    dry_run: bool,
    cancelled: &'a AtomicBool,
) -> Result<(Context<'a>, Vec<Selected>)> {
    let (root, config) = project::configuration(start)?;
    let packages = project::discover(start, None)?;
    let version = config
        .version
        .ok_or("shipwright.toml needs a shared version for release")?;
    if version.is_empty()
        || !version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c))
    {
        return Err("Invalid shared version".into());
    }
    let logs = tempfile::Builder::new()
        .prefix("shipwright-release-")
        .tempdir()
        .map_err(err)?
        .keep();
    let mut cx = Context {
        root,
        git_root: PathBuf::new(),
        commit: String::new(),
        version,
        logs,
        cancelled,
        http: http(),
        dry_run,
    };
    cx.git_root = PathBuf::from(git(&cx, &["rev-parse", "--show-toplevel"])?);
    cx.commit = git(&cx, &["rev-parse", "HEAD"])?;
    if !dry_run {
        clean(&cx)?;
    }
    let names = config.release.packages.unwrap_or_else(|| {
        packages
            .iter()
            .map(|p| p.language.name().to_owned())
            .collect()
    });
    for list in [&names, &config.release.binaries] {
        let mut seen = std::collections::HashSet::new();
        for name in list {
            if !seen.insert(name) {
                return Err(format!("Duplicate release selection: {name}"));
            }
            if !packages.iter().any(|p| p.language.name() == name) {
                return Err(format!("Unknown release package: {name}"));
            }
        }
    }
    if config
        .release
        .binaries
        .iter()
        .any(|n| n != "go" && n != "rust")
    {
        return Err("release.binaries supports only go and rust".into());
    }
    if let Some(name) = selected
        && !packages.iter().any(|p| p.language.name() == name)
    {
        return Err(format!("Unknown package: {name}"));
    }
    let mut chosen = Vec::new();
    // Validate every configured manifest even for a single-package invocation.
    for mut package in packages {
        package.directory = package.directory.canonicalize().map_err(err)?;
        if !package.directory.starts_with(&cx.git_root) {
            return Err("Release packages must be inside the Git checkout".into());
        }
        let name = identity(&cx, &package)?;
        let registry = names.iter().any(|n| n == package.language.name());
        let binaries = config
            .release
            .binaries
            .iter()
            .any(|n| n == package.language.name());
        if (registry || binaries) && selected.is_none_or(|s| s == package.language.name()) {
            chosen.push(Selected {
                package,
                name,
                registry,
                binaries,
            });
        }
    }
    if chosen.is_empty() {
        return Err("No release outputs selected".into());
    }
    Ok((cx, chosen))
}
