use super::{Context, Result, check_cancel, err};
use crate::{
    build::{self, Outcome},
    project::Package,
};
use std::{fs::OpenOptions, io::Write, process::Command};

pub(super) fn log(cx: &Context<'_>, p: &Package, message: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(cx.logs.join(format!("{}.log", p.language.name())))
        .map_err(err)?;
    writeln!(file, "{message}").map_err(err)
}
pub(super) fn command(
    cx: &Context<'_>,
    p: &Package,
    program: &str,
    args: &[&str],
    publishing: bool,
) -> Result<Vec<u8>> {
    check_cancel(cx)?;
    log(cx, p, &format!("$ {program} {}", args.join(" ")))?;
    let output = build::capture(
        Command::new(program).args(args).current_dir(&p.directory),
        cx.cancelled,
    )
    .map_err(err)?;
    // npm authentication output bypasses this helper entirely. Sanitize other tool logs.
    if !publishing || matches!(output.outcome, Outcome::Failed(_)) {
        log(cx, p, &redact(&String::from_utf8_lossy(&output.stdout)))?;
        log(cx, p, &redact(&String::from_utf8_lossy(&output.stderr)))?;
    }
    match output.outcome {
        Outcome::Success => Ok(output.stdout),
        Outcome::Cancelled => Err("Cancelled".into()),
        Outcome::Failed(reason) => Err(format!(
            "{program} failed: {reason}{}",
            if publishing {
                match program {
                    "cargo" => {
                        "; check crates.io credentials (cargo login or CARGO_REGISTRY_TOKEN)"
                    }
                    "uv" => "; check PyPI credentials (UV_PUBLISH_TOKEN or a credential provider)",
                    _ => "",
                }
            } else {
                ""
            }
        )),
    }
}
pub(super) fn redact(text: &str) -> String {
    let mut text = text.to_owned();
    for (key, value) in std::env::vars() {
        if value.len() >= 4
            && ["TOKEN", "PASSWORD", "SECRET", "OTP"]
                .iter()
                .any(|word| key.to_ascii_uppercase().contains(word))
        {
            text = text.replace(&value, "[redacted]");
        }
    }
    text.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if [
                "authorization",
                "password",
                "token",
                "otp",
                "authurl",
                "doneurl",
            ]
            .iter()
            .any(|word| lower.contains(word))
            {
                return "[authentication details redacted]".to_owned();
            }
            line.split_whitespace()
                .map(|s| {
                    if s.contains("://") {
                        "[URL redacted]"
                    } else {
                        s
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
