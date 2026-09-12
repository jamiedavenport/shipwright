use super::{
    Context, Event, Prepared, Result, Selected, check_cancel,
    command::command,
    err,
    http::{encode, exists},
    manifest::{field, json_file},
};
use crate::{
    build::{self, Outcome},
    project::Package,
};
use serde_json::Value;
use std::{
    io::IsTerminal,
    path::Path,
    process::Command,
    sync::{atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

pub(super) fn identity(p: &Package) -> Result<(String, String)> {
    let m = json_file(&p.directory.join("package.json"))?;
    Ok((field(&m, "name")?.into(), field(&m, "version")?.into()))
}

pub(super) fn prepare(cx: &Context<'_>, s: &Selected, skip: bool) -> Result<Prepared> {
    let p = &s.package;
    let mut out = Prepared::default();
    if s.registry {
        out.already_published = exists(
            cx,
            &format!(
                "https://registry.npmjs.org/{}/{}",
                encode(&s.name),
                encode(&cx.version)
            ),
        )?;
    }
    if !skip {
        let (program, args) = p.language.command();
        command(cx, p, program, args, false)?;
    }
    let data = command(cx, p, "npm", &["pack", "--json", "--ignore-scripts"], false)?;
    let packed: Value = serde_json::from_slice(&data).map_err(err)?;
    let pack = packed.get(0).ok_or("npm pack returned no tarball")?;
    if field(pack, "name")? != s.name || field(pack, "version")? != cx.version {
        return Err("npm tarball identity differs from manifest".into());
    }
    let file = field(pack, "filename")?;
    if Path::new(file).components().count() != 1 {
        return Err("Unexpected npm tarball path".into());
    }
    let path = p.directory.join(file);
    if !path.is_file() {
        return Err("npm pack did not produce its tarball".into());
    }
    validate_npm_entries(p, pack)?;
    out.files.push(path);
    Ok(out)
}

// npm pack does not reject missing compiled entry points, even with scripts disabled.
fn validate_npm_entries(p: &Package, packed: &Value) -> Result<()> {
    fn paths(value: &Value, output: &mut Vec<String>) {
        match value {
            Value::String(s) => output.push(s.trim_start_matches("./").to_owned()),
            Value::Object(map) => {
                for value in map.values() {
                    paths(value, output);
                }
            }
            Value::Array(values) => {
                for value in values {
                    paths(value, output);
                }
            }
            _ => {}
        }
    }
    let manifest = json_file(&p.directory.join("package.json"))?;
    let mut required = Vec::new();
    for key in ["main", "module", "types", "typings", "bin", "exports"] {
        paths(&manifest[key], &mut required);
    }
    let files = packed["files"]
        .as_array()
        .ok_or("npm pack did not list tarball contents")?;
    for path in required {
        let found = files.iter().filter_map(|f| f["path"].as_str()).any(|file| {
            if let Some((prefix, suffix)) = path.split_once('*') {
                file.starts_with(prefix) && file.ends_with(suffix)
            } else {
                file == path
            }
        });
        if !found {
            return Err(format!(
                "npm tarball is missing declared entry point {path}; build the package and check its files/exports"
            ));
        }
    }
    Ok(())
}

pub(super) fn publish(
    cx: &Context<'_>,
    p: &Package,
    tarball: &Path,
    events: &mpsc::Sender<Event>,
) -> Result<()> {
    let mut otp: Option<String> = None;
    for attempt in 0..2 {
        check_cancel(cx)?;
        let mut cmd = Command::new("npm");
        cmd.current_dir(&p.directory).args([
            "publish",
            tarball.to_str().ok_or("Non-UTF8 tarball path")?,
            "--json",
            "--ignore-scripts",
            "--registry=https://registry.npmjs.org/",
            "--access=public",
            "--logs-max=0",
        ]);
        // Prevent npm's own browser/terminal auth and debug logs; handle structured errors here.
        cmd.env("CI", "true")
            .env("npm_config_browser", "false")
            .env("npm_config_logs_max", "0");
        if let Some(otp) = &otp {
            cmd.env("npm_config_otp", otp);
        }
        let output = build::capture(&mut cmd, cx.cancelled).map_err(err)?;
        if matches!(output.outcome, Outcome::Success) {
            return Ok(());
        }
        check_cancel(cx)?;
        let data: Value = serde_json::from_slice(&output.stdout).or_else(|_| serde_json::from_slice(&output.stderr)).map_err(|_| "npm publish failed without a structured error; check npm credentials and package configuration")?;
        let error = data.get("error").unwrap_or(&data);
        let code = error["code"].as_str().unwrap_or("unknown");
        let challenge = code == "EOTP"
            || (code == "E401"
                && (error["summary"]
                    .as_str()
                    .is_some_and(|s| s.contains("one-time password"))
                    || (error["authUrl"].is_string() && error["doneUrl"].is_string())));
        if attempt == 0 && challenge {
            let (tx, rx) = mpsc::channel();
            events
                .send(Event::Authenticate(error.clone(), tx))
                .map_err(err)?;
            otp = Some(rx.recv().map_err(err)??);
        } else {
            let safe_code: String = code
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .take(32)
                .collect();
            return Err(format!(
                "npm publish failed ({safe_code}); {}",
                if challenge {
                    "authentication retry failed"
                } else {
                    "check package configuration and npm credentials (npm login or a CI token)"
                }
            ));
        }
    }
    Err("npm authentication failed".into())
}
pub(super) fn authenticate(cx: &Context<'_>, challenge: &Value) -> Result<String> {
    check_cancel(cx)?;
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err("npm requires interactive authentication; rerun in a terminal or configure a CI publishing token".into());
    }
    match (challenge["authUrl"].as_str(), challenge["doneUrl"].as_str()) {
        (Some(auth), Some(done)) => {
            // Public npm only. Never forward registry credentials to the session endpoint.
            if !auth.starts_with("https://www.npmjs.com/")
                && !auth.starts_with("https://npmjs.com/")
            {
                return Err("npm returned an unsupported authentication URL".into());
            }
            if !done.starts_with("https://registry.npmjs.org/")
                || auth.chars().any(char::is_control)
                || done.chars().any(char::is_control)
            {
                return Err("npm returned an invalid authentication URL".into());
            }
            eprintln!("Open this URL to authenticate with npm:\n{auth}");
            let deadline = Instant::now() + Duration::from_secs(300);
            loop {
                check_cancel(cx)?;
                if Instant::now() >= deadline {
                    return Err("npm browser authentication timed out".into());
                }
                let mut response = cx
                    .http
                    .get(done)
                    .header("Cache-Control", "no-cache")
                    .call()
                    .map_err(|_| "npm authentication polling failed")?;
                match response.status().as_u16() {
                    200 => {
                        let value: Value = response
                            .body_mut()
                            .read_json()
                            .map_err(|_| "Invalid npm authentication response")?;
                        return value["token"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .ok_or("Missing npm authentication token".into());
                    }
                    202 => {
                        let retry = response
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(1)
                            .clamp(1, 30);
                        let until = (Instant::now() + Duration::from_secs(retry)).min(deadline);
                        while Instant::now() < until {
                            check_cancel(cx)?;
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                    status => {
                        return Err(format!("npm authentication polling failed (HTTP {status})"));
                    }
                }
            }
        }
        (None, None) => {
            let otp = cliclack::password("npm one-time password")
                .mask('•')
                .interact()
                .map_err(|_| {
                    cx.cancelled.store(true, Ordering::Relaxed);
                    "npm authentication cancelled"
                })?;
            check_cancel(cx)?;
            if otp.trim().is_empty() {
                Err("Empty npm one-time password".into())
            } else {
                Ok(otp)
            }
        }
        _ => Err("npm returned an incomplete browser authentication challenge".into()),
    }
}
