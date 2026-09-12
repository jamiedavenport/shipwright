use super::{
    Context, Event, Prepared, Result, Selected, check_cancel, command::log, go, npm, python, rust,
};
use crate::project::Language;
use std::sync::mpsc;

pub(super) fn prepare(cx: &Context<'_>, s: &Selected, skip: bool) -> Result<Prepared> {
    let p = &s.package;
    log(cx, p, "Preparing release")?;
    let out = match p.language {
        Language::Python => python::prepare(cx, s, skip)?,
        Language::TypeScript => npm::prepare(cx, s, skip)?,
        Language::Rust => rust::prepare(cx, s, skip)?,
        Language::Go => go::prepare(cx, s, skip)?,
    };
    if out.already_published {
        log(
            cx,
            p,
            "already published (local source equality not checked)",
        )?;
    }
    Ok(out)
}

pub(super) fn publish(
    cx: &Context<'_>,
    s: &Selected,
    p: &Prepared,
    events: &mpsc::Sender<Event>,
) -> Result<()> {
    check_cancel(cx)?;
    if !s.registry {
        return Ok(());
    }
    if p.already_published {
        eprintln!(
            "{}: already published (local source equality not checked)",
            s.package.language.name()
        );
        return Ok(());
    }
    match s.package.language {
        Language::Python => python::publish(cx, s, p)?,
        Language::TypeScript => npm::publish(cx, &s.package, &p.files[0], events)?,
        Language::Rust => rust::publish(cx, s)?,
        Language::Go => go::publish(cx, s)?,
    }
    log(cx, &s.package, "Published successfully")
}
