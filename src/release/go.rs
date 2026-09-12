use super::{
    Context, Prepared, Result, Selected, archive::archive, command::redact, err, git::push_tag,
};
use crate::{build, project::Package};
use std::fs;

pub(super) fn identity(cx: &Context<'_>, p: &Package) -> Result<(String, String)> {
    let text = fs::read_to_string(p.directory.join("go.mod")).map_err(err)?;
    let name = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("module "))
        .ok_or("go.mod needs a module directive")?
        .trim()
        .trim_matches('"')
        .to_owned();
    Ok((name, cx.version.clone()))
}

pub(super) fn prepare(cx: &Context<'_>, s: &Selected, skip: bool) -> Result<Prepared> {
    let p = &s.package;
    let mut out = Prepared::default();
    if s.binaries {
        let files =
            build::go_binaries(p, cx.cancelled, !skip).map_err(|e| redact(&e.to_string()))?;
        if files.is_empty() {
            return Err("No Go main packages found".into());
        }
        out.archive = Some(archive(cx, p, &p.directory.join("bin"), &files)?);
    }
    Ok(out)
}

pub(super) fn publish(cx: &Context<'_>, s: &Selected) -> Result<()> {
    push_tag(cx, &go_tag(cx, &s.package)?)
}

pub(super) fn go_tag(cx: &Context<'_>, p: &Package) -> Result<String> {
    let relative = p
        .directory
        .strip_prefix(&cx.git_root)
        .map_err(err)?
        .to_str()
        .ok_or("Non-UTF8 Go directory")?
        .replace('\\', "/");
    Ok(if relative.is_empty() {
        format!("v{}", cx.version)
    } else {
        format!("{relative}/v{}", cx.version)
    })
}
