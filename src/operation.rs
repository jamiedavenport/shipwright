use crate::project::Language;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Build,
    Test,
    Lint { fix: bool },
    Format { fix: bool },
}

impl Operation {
    pub fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Test => "test",
            Self::Lint { .. } => "lint",
            Self::Format { .. } => "format",
        }
    }

    pub fn progress(self) -> (&'static str, &'static str) {
        match self {
            Self::Build => ("building", "built"),
            Self::Test => ("testing", "passed"),
            Self::Lint { fix: false } | Self::Format { fix: false } => ("checking", "passed"),
            Self::Lint { fix: true } | Self::Format { fix: true } => ("fixing", "finished"),
        }
    }

    pub fn command(self, language: Language) -> (&'static str, &'static [&'static str]) {
        use Language::*;
        match (self, language) {
            (Self::Build, Python) => ("uv", &["build"]),
            (Self::Build, TypeScript) => ("bun", &["run", "build"]),
            (Self::Build, Go) => ("go", &["build", "./..."]),
            (Self::Build, Rust) => ("cargo", &["build", "--release"]),
            (Self::Test, Python) => ("uv", &["run", "pytest"]),
            (Self::Test, TypeScript) => ("bun", &["run", "test"]),
            (Self::Test, Go) => ("go", &["test", "./..."]),
            (Self::Test, Rust) => ("cargo", &["test"]),
            (Self::Lint { fix: false }, Python) => ("uv", &["run", "ruff", "check", "."]),
            (Self::Lint { fix: true }, Python) => ("uv", &["run", "ruff", "check", "--fix", "."]),
            (Self::Lint { fix: false }, TypeScript) => ("bun", &["run", "lint"]),
            (Self::Lint { fix: true }, TypeScript) => ("bun", &["run", "lint", "--fix"]),
            (Self::Lint { fix: false }, Go) => ("go", &["vet", "./..."]),
            (Self::Lint { fix: true }, Go) => ("go", &["vet", "-fix", "./..."]),
            (Self::Lint { fix: false }, Rust) => (
                "cargo",
                &["clippy", "--all-targets", "--", "-D", "warnings"],
            ),
            (Self::Lint { fix: true }, Rust) => (
                "cargo",
                &[
                    "clippy",
                    "--fix",
                    "--allow-dirty",
                    "--allow-staged",
                    "--",
                    "-D",
                    "warnings",
                ],
            ),
            (Self::Format { fix: false }, Python) => {
                ("uv", &["run", "ruff", "format", "--check", "."])
            }
            (Self::Format { fix: true }, Python) => ("uv", &["run", "ruff", "format", "."]),
            (Self::Format { fix: false }, TypeScript) => ("bun", &["run", "format:check"]),
            (Self::Format { fix: true }, TypeScript) => ("bun", &["run", "format"]),
            (Self::Format { fix: false }, Go) => ("gofmt", &["-l", "."]),
            (Self::Format { fix: true }, Go) => ("gofmt", &["-w", "."]),
            (Self::Format { fix: false }, Rust) => ("cargo", &["fmt", "--check"]),
            (Self::Format { fix: true }, Rust) => ("cargo", &["fmt"]),
        }
    }
}
