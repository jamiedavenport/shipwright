use super::{
    Context, Prepared, Result, Selected,
    archive::archive,
    command::command,
    err,
    http::{encode, exists},
    manifest::field,
};
use crate::project::Package;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn identity(cx: &Context<'_>, p: &Package) -> Result<(String, String)> {
    let m = cargo_metadata(cx, p)?;
    let m = cargo_package(&m, p)?;
    Ok((field(m, "name")?.into(), field(m, "version")?.into()))
}

pub(super) fn prepare(cx: &Context<'_>, s: &Selected, skip: bool) -> Result<Prepared> {
    let p = &s.package;
    let mut out = Prepared::default();
    if s.registry {
        out.already_published = exists(
            cx,
            &format!(
                "https://crates.io/api/v1/crates/{}/{}",
                encode(&s.name),
                encode(&cx.version)
            ),
        )?;
        let mut args = vec![
            "publish",
            "--locked",
            "--registry",
            "crates-io",
            "--dry-run",
        ];
        if cx.dry_run {
            args.push("--allow-dirty");
        }
        if skip {
            args.push("--no-verify");
        }
        command(cx, p, "cargo", &args, false)?;
    }
    if s.binaries {
        let metadata = cargo_metadata(cx, p)?;
        let package = cargo_package(&metadata, p)?;
        let targets = package["targets"]
            .as_array()
            .ok_or("Missing Cargo targets")?;
        let names = targets
            .iter()
            .filter(|t| {
                t["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|v| v == "bin"))
            })
            .map(|t| field(t, "name").map(str::to_owned))
            .collect::<Result<Vec<_>>>()?;
        if names.is_empty() {
            return Err("No Rust binary targets found".into());
        }
        // Explicit host target prevents a configured cross target being mislabeled.
        let rustc = command(cx, p, "rustc", &["-vV"], false)?;
        let rustc = String::from_utf8_lossy(&rustc);
        let host = rustc
            .lines()
            .find_map(|l| l.strip_prefix("host: "))
            .ok_or("rustc did not report its host")?;
        if !skip {
            command(
                cx,
                p,
                "cargo",
                &["build", "--release", "--locked", "--bins", "--target", host],
                false,
            )?;
        }
        let dir = PathBuf::from(field(&metadata, "target_directory")?)
            .join(host)
            .join("release");
        let files = names
            .iter()
            .map(|n| dir.join(format!("{n}{}", std::env::consts::EXE_SUFFIX)))
            .collect::<Vec<_>>();
        out.archive = Some(archive(cx, p, &dir, &files)?);
    }
    Ok(out)
}

pub(super) fn publish(cx: &Context<'_>, s: &Selected) -> Result<()> {
    command(
        cx,
        &s.package,
        "cargo",
        &[
            "publish",
            "--locked",
            "--registry",
            "crates-io",
            "--no-verify",
        ],
        true,
    )?;
    Ok(())
}

fn cargo_metadata(cx: &Context<'_>, p: &Package) -> Result<Value> {
    serde_json::from_slice(&command(
        cx,
        p,
        "cargo",
        &["metadata", "--format-version=1", "--no-deps", "--locked"],
        false,
    )?)
    .map_err(err)
}
fn cargo_package<'a>(m: &'a Value, p: &Package) -> Result<&'a Value> {
    let manifest = p.directory.join("Cargo.toml").canonicalize().map_err(err)?;
    m["packages"]
        .as_array()
        .ok_or("Invalid Cargo metadata")?
        .iter()
        .find(|v| {
            v["manifest_path"]
                .as_str()
                .is_some_and(|s| Path::new(s) == manifest)
        })
        .ok_or("Rust release requires a package manifest, not a virtual workspace".into())
}
