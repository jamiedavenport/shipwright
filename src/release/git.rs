use super::{Context, Result, check_cancel, err};
use crate::{process, runner::Outcome};
use std::process::Command;

pub(super) fn git(cx: &Context<'_>, args: &[&str]) -> Result<String> {
    check_cancel(cx)?;
    let output = process::capture(
        Command::new("git")
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(&cx.root),
        cx.cancelled,
    )
    .map_err(err)?;
    match output.outcome {
        Outcome::Success => Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()),
        _ => Err(format!(
            "git {} failed; check origin, credentials and connectivity",
            args[0]
        )),
    }
}
pub(super) fn clean(cx: &Context<'_>) -> Result<()> {
    if !git(cx, &["status", "--porcelain", "--untracked-files=all"])?.is_empty() {
        return Err("Release requires a clean checkout (including untracked files)".into());
    }
    Ok(())
}

pub(super) fn check_tag(cx: &Context<'_>, tag: &str) -> Result<bool> {
    git(cx, &["check-ref-format", &format!("refs/tags/{tag}")])?;
    let local = git(cx, &["tag", "--list", tag])?;
    if !local.is_empty()
        && git(cx, &["rev-parse", &format!("refs/tags/{tag}^{{commit}}")])? != cx.commit
    {
        return Err(format!("Local tag {tag} points to another commit"));
    }
    let refs = git(
        cx,
        &[
            "ls-remote",
            "--tags",
            "origin",
            &format!("refs/tags/{tag}"),
            &format!("refs/tags/{tag}^{{}}"),
        ],
    )?;
    let direct = refs.lines().find_map(|l| {
        l.split_once('\t')
            .filter(|(_, r)| *r == format!("refs/tags/{tag}"))
            .map(|(c, _)| c)
    });
    let peeled = refs.lines().find_map(|l| {
        l.split_once('\t')
            .filter(|(_, r)| r.ends_with("^{}"))
            .map(|(c, _)| c)
    });
    if let Some(commit) = peeled.or(direct) {
        if commit != cx.commit {
            return Err(format!("Remote tag {tag} points to another commit"));
        }
        Ok(true)
    } else {
        Ok(false)
    }
}
pub(super) fn push_tag(cx: &Context<'_>, tag: &str) -> Result<()> {
    if check_tag(cx, tag)? {
        eprintln!("{tag}: already published");
        return Ok(());
    }
    if git(cx, &["tag", "--list", tag])?.is_empty() {
        git(cx, &["tag", tag, &cx.commit])?;
    }
    git(
        cx,
        &[
            "push",
            "origin",
            &format!("refs/tags/{tag}:refs/tags/{tag}"),
        ],
    )?;
    Ok(())
}
