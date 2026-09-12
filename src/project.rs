use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Python,
    TypeScript,
    Go,
    Rust,
}

impl Language {
    const ALL: [Self; 4] = [Self::Python, Self::TypeScript, Self::Go, Self::Rust];

    pub fn name(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::Go => "go",
            Self::Rust => "rust",
        }
    }

    fn manifest(self) -> &'static str {
        // TODO: Manifest detection.
        match self {
            Self::Python => "pyproject.toml",
            Self::TypeScript => "package.json",
            Self::Go => "go.mod",
            Self::Rust => "Cargo.toml",
        }
    }

    pub fn command(self) -> (&'static str, &'static [&'static str]) {
        // TODO: Command detection.
        match self {
            Self::Python => ("uv", &["build"]),
            Self::TypeScript => ("bun", &["run", "build"]),
            Self::Go => ("go", &["build", "./..."]),
            Self::Rust => ("cargo", &["build", "--release"]),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Package {
    pub language: Language,
    pub directory: PathBuf,
}

#[derive(Deserialize)]
pub struct Config {
    pub version: Option<String>,
    #[serde(default)]
    pub release: ReleaseConfig,
    source: PathBuf,
    #[serde(default)]
    targets: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseConfig {
    pub packages: Option<Vec<String>>,
    #[serde(default)]
    pub binaries: Vec<String>,
}

pub fn configuration(start: &Path) -> Result<(PathBuf, Config), String> {
    let root = start
        .ancestors()
        .find(|path| path.join("shipwright.toml").is_file())
        .ok_or("No shipwright.toml found in this directory or its parents")?;
    let path = root.join("shipwright.toml");
    let contents = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let config = toml::from_str(&contents).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((root.to_owned(), config))
}

pub fn discover(start: &Path, selected: Option<&str>) -> Result<Vec<Package>, String> {
    let (root, config) = configuration(start)?;
    let source = root.join(&config.source);
    if !source.exists() {
        return Err(format!("Source path does not exist: {}", source.display()));
    }
    let mut packages = Vec::new();
    for directory in source.ancestors() {
        // TODO: Report ambiguous manifests instead of choosing the first language match.
        if let Some(language) = Language::ALL
            .into_iter()
            .find(|language| directory.join(language.manifest()).is_file())
        {
            packages.push(Package {
                language,
                directory: directory.to_owned(),
            });
            break;
        }
    }
    if packages.is_empty() {
        return Err(format!(
            "No supported package manifest found above {}",
            source.display()
        ));
    }
    for target in config.targets {
        let language = Language::ALL
            .into_iter()
            .find(|language| language.name() == target)
            .ok_or_else(|| format!("Unsupported target: {target}"))?;
        if packages.iter().any(|package| package.language == language) {
            return Err(format!("Duplicate package: {target}"));
        }
        packages.push(Package {
            language,
            directory: root.join(target),
        });
    }
    if let Some(selected) = selected {
        if !packages
            .iter()
            .any(|package| package.language.name() == selected)
        {
            let names = packages
                .iter()
                .map(|package| package.language.name())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "Unknown package '{selected}'; available packages: {names}"
            ));
        }
        packages.retain(|package| package.language.name() == selected);
    }
    for package in &packages {
        let manifest = package.directory.join(package.language.manifest());
        if !manifest.is_file() {
            return Err(format!("Missing package manifest: {}", manifest.display()));
        }
    }
    Ok(packages)
}
