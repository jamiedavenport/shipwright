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
            // TODO: Discover Go CLI names and paths instead of hard-coding htomd.
            Self::Go => ("go", &["build", "-o", "bin/htomd", "./cmd/htomd"]),
            Self::Rust => ("cargo", &["build", "--release"]),
        }
    }
}

#[derive(Debug)]
pub struct Package {
    pub language: Language,
    pub directory: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    source: PathBuf,
    #[serde(default)]
    targets: Vec<String>,
}

pub fn discover(start: &Path, selected: Option<&str>) -> Result<Vec<Package>, String> {
    let root = start
        .ancestors()
        .find(|path| path.join("shipwright.toml").is_file())
        .ok_or("No shipwright.toml found in this directory or its parents")?;
    let config_path = root.join("shipwright.toml");
    let contents = std::fs::read_to_string(&config_path)
        .map_err(|error| format!("{}: {error}", config_path.display()))?;
    let config: Config =
        toml::from_str(&contents).map_err(|error| format!("{}: {error}", config_path.display()))?;
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
