use super::{Context, Result, err, go, npm, python, rust};
use crate::project::{Language, Package};
use serde_json::Value;
use std::{fs, path::Path};

pub(super) fn json_file(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).map_err(err)?).map_err(err)
}
pub(super) fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing {key}"))
}
pub(super) fn identity(cx: &Context<'_>, p: &Package) -> Result<String> {
    let (name, version) = match p.language {
        Language::Python => python::identity(p)?,
        Language::TypeScript => npm::identity(p)?,
        Language::Rust => rust::identity(cx, p)?,
        Language::Go => go::identity(cx, p)?,
    };
    if version != cx.version {
        return Err(format!(
            "{} version {version} differs from shared version {}",
            p.language.name(),
            cx.version
        ));
    }
    Ok(name)
}
