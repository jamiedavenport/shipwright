mod archive;
mod command;
mod git;
mod github;
mod go;
mod http;
mod manifest;
mod npm;
mod packages;
mod python;
mod rust;
mod selection;

use crate::{
    project::{Language, Package},
    render::Display,
    runner::{Outcome, PackageResult},
};
use git::{check_tag, git, push_tag};
use github::Github;
use go::go_tag;
use http::encode;
use npm::authenticate;
use packages::{prepare, publish};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Instant,
};

type Result<T> = std::result::Result<T, String>;

struct Selected {
    package: Package,
    name: String,
    registry: bool,
    binaries: bool,
}
#[derive(Default)]
struct Prepared {
    files: Vec<PathBuf>,
    archive: Option<PathBuf>,
    already_published: bool,
}
struct Context<'a> {
    root: PathBuf,
    git_root: PathBuf,
    commit: String,
    version: String,
    logs: PathBuf,
    cancelled: &'a AtomicBool,
    http: ureq::Agent,
    dry_run: bool,
}
enum Event {
    Complete(usize, PackageResult),
    Authenticate(Value, mpsc::Sender<Result<String>>),
}

pub fn run(
    start: &Path,
    selected: Option<&str>,
    dry_run: bool,
    skip_build: bool,
    cancelled: &AtomicBool,
) -> Result<u8> {
    let (cx, chosen) = selection::select(start, selected, dry_run, cancelled)?;
    let github = if chosen.iter().any(|s| s.binaries) {
        Some(Github::new(&cx, dry_run)?)
    } else {
        None
    };
    let validate_tags = || -> Result<()> {
        for s in &chosen {
            if s.registry && s.package.language == Language::Go {
                check_tag(&cx, &go_tag(&cx, &s.package)?)?;
            }
        }
        if github.is_some() {
            check_tag(&cx, &format!("v{}", cx.version))?;
        }
        Ok(())
    };
    if !dry_run {
        validate_tags()?;
    }
    let display_packages: Vec<_> = chosen.iter().map(|s| s.package.clone()).collect();
    let display = Display::phase(
        &display_packages,
        "Shipwright release",
        "preparing",
        "prepared",
    );
    let (prepared, results) = std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        for (index, selection) in chosen.iter().enumerate() {
            let tx = tx.clone();
            let cx = &cx;
            scope.spawn(move || {
                let started = Instant::now();
                let result = prepare(cx, selection, skip_build);
                let status = result_status(
                    cx,
                    selection,
                    started,
                    result.as_ref().map(|_| ()).map_err(Clone::clone),
                );
                let _ = tx.send((index, result, status));
            });
        }
        drop(tx);
        let mut prepared: Vec<Option<Prepared>> = (0..chosen.len()).map(|_| None).collect();
        let mut results = Vec::new();
        for (index, result, status) in rx {
            display.complete(index, &status);
            prepared[index] = result.ok();
            results.push(status);
        }
        (prepared, results)
    });
    display.finish(&results, &cx.logs);
    if results.iter().any(|r| !r.succeeded()) {
        return Ok(exit_status(cancelled));
    }
    if dry_run {
        validate_tags()?;
        if let Some(github) = &github {
            let (status, release) = github.request(
                &cx,
                "GET",
                &format!("releases/tags/v{}", encode(&cx.version)),
                None,
            )?;
            if status == 200 {
                for p in prepared.iter().flatten() {
                    if let Some(archive) = &p.archive {
                        github.upload(&cx, &release, archive, true)?;
                    }
                }
            }
        }
        for (s, p) in chosen.iter().zip(&prepared) {
            let p = p.as_ref().unwrap();
            eprintln!(
                "{}: {}{}",
                s.package.language.name(),
                if s.registry {
                    if p.already_published {
                        "already published (source equality not checked)"
                    } else {
                        "would publish package"
                    }
                } else {
                    "binary only"
                },
                if p.archive.is_some() {
                    "; would upload GitHub archive"
                } else {
                    ""
                }
            );
        }
        eprintln!("Dry run complete; no remote changes.");
        return Ok(0);
    }
    if !git(&cx, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        return Err("Tracked files changed during preparation".into());
    }
    if git(&cx, &["rev-parse", "HEAD"])? != cx.commit {
        return Err("HEAD changed during preparation".into());
    }
    let mut display = Display::phase(
        &display_packages,
        "Shipwright release",
        "publishing",
        "released",
    );
    let results = std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        for (index, s) in chosen.iter().enumerate() {
            let tx = tx.clone();
            let cx = &cx;
            let p = prepared[index].as_ref().unwrap();
            scope.spawn(move || {
                let started = Instant::now();
                let result = publish(cx, s, p, &tx);
                let _ = tx.send(Event::Complete(
                    index,
                    result_status(cx, s, started, result),
                ));
            });
        }
        drop(tx);
        let mut completed = Vec::new();
        for event in rx {
            match event {
                Event::Complete(index, result) => {
                    display.complete(index, &result);
                    completed.push((index, result));
                }
                Event::Authenticate(challenge, response) => {
                    display.suspend();
                    let answer = authenticate(&cx, &challenge);
                    let _ = response.send(answer);
                    display = Display::phase(
                        &display_packages,
                        "Shipwright release",
                        "publishing",
                        "released",
                    );
                    for (index, result) in &completed {
                        display.complete(*index, result);
                    }
                }
            }
        }
        completed.into_iter().map(|(_, r)| r).collect::<Vec<_>>()
    });
    display.finish(&results, &cx.logs);
    let mut failed = results.iter().any(|r| !r.succeeded());
    // Keep successful registry publications even when a different publisher fails.
    if let Some(github) = github {
        let result = (|| {
            let tag = format!("v{}", cx.version);
            push_tag(&cx, &tag)?;
            let release = github.release(&cx, &tag)?;
            for p in prepared.iter().flatten() {
                if let Some(archive) = &p.archive {
                    github.upload(&cx, &release, archive, false)?;
                }
            }
            if !failed {
                github.finish(&cx, &release)?;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            eprintln!("GitHub: {error}");
            failed = true;
        }
    }
    if failed {
        eprintln!(
            "Release incomplete; successful publications are preserved. Rerun after fixing the failure."
        );
        Ok(exit_status(cancelled))
    } else {
        Ok(0)
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn exit_status(cancelled: &AtomicBool) -> u8 {
    if cancelled.load(Ordering::Relaxed) {
        130
    } else {
        1
    }
}
fn check_cancel(cx: &Context<'_>) -> Result<()> {
    if cx.cancelled.load(Ordering::Relaxed) {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}
fn result_status(
    cx: &Context<'_>,
    s: &Selected,
    started: Instant,
    result: Result<()>,
) -> PackageResult {
    PackageResult {
        name: s.package.language.name(),
        outcome: if cx.cancelled.load(Ordering::Relaxed) {
            Outcome::Cancelled
        } else {
            match result {
                Ok(()) => Outcome::Success,
                Err(e) => Outcome::Failed(e),
            }
        },
        elapsed: started.elapsed(),
        log: cx.logs.join(format!("{}.log", s.package.language.name())),
    }
}
