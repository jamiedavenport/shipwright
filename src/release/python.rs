use super::{Context, Prepared, Result, Selected, command::command, err};
use crate::project::Package;
use std::{fs, path::Path};

pub(super) fn identity(p: &Package) -> Result<(String, String)> {
    let m = toml_file(&p.directory.join("pyproject.toml"))?;
    let project = m.get("project").ok_or("pyproject.toml needs [project]")?;
    Ok((
        project
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or("Python project needs a name")?
            .to_owned(),
        project
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or("Python release needs a static project.version")?
            .to_owned(),
    ))
}

pub(super) fn prepare(cx: &Context<'_>, s: &Selected, skip: bool) -> Result<Prepared> {
    let p = &s.package;
    let mut out = Prepared::default();
    if !skip {
        command(cx, p, "uv", &["build"], false)?;
    }
    let normalized = s.name.replace(['-', '.'], "_").to_lowercase();
    let prefix = format!("{normalized}-{}", cx.version);
    for entry in fs::read_dir(p.directory.join("dist"))
        .map_err(|_| "Missing dist/; run without --skip-build")?
    {
        let path = entry.map_err(err)?.path();
        let name = path.file_name().unwrap().to_string_lossy();
        if (name.starts_with(&format!("{prefix}-")) && name.ends_with(".whl"))
            || name == format!("{prefix}.tar.gz")
            || name == format!("{}-{}.tar.gz", s.name, cx.version)
        {
            out.files.push(path);
        }
    }
    out.files.sort();
    if !out
        .files
        .iter()
        .any(|p| p.extension().is_some_and(|e| e == "whl"))
        || !out
            .files
            .iter()
            .any(|p| p.to_string_lossy().ends_with(".tar.gz"))
    {
        return Err("Missing wheel or sdist for the selected Python name/version in dist/".into());
    }
    Ok(out)
}

pub(super) fn publish(cx: &Context<'_>, s: &Selected, p: &Prepared) -> Result<()> {
    let mut args = vec![
        "publish",
        "--publish-url",
        "https://upload.pypi.org/legacy/",
        "--check-url",
        "https://pypi.org/simple/",
    ];
    // Preserve trusted publishing in Actions without requiring a stored PyPI token.
    if std::env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
        && std::env::var_os("ACTIONS_ID_TOKEN_REQUEST_URL").is_some()
        && std::env::var_os("ACTIONS_ID_TOKEN_REQUEST_TOKEN").is_some()
    {
        args.extend(["--trusted-publishing", "always"]);
    }
    for file in &p.files {
        args.push(file.to_str().ok_or("Non-UTF8 distribution path")?);
    }
    command(cx, &s.package, "uv", &args, true)?;
    Ok(())
}

fn toml_file(path: &Path) -> Result<toml::Value> {
    toml::from_str(&fs::read_to_string(path).map_err(err)?).map_err(err)
}
