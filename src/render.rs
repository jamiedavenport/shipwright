use crate::build::{BuildResult, Outcome};
use crate::project::Package;
use cliclack::{MultiProgress, ProgressBar, Theme, ThemeState};
use console::Style;
use std::fs::File;
use std::io::{self, IsTerminal, Read, Seek, SeekFrom};
use std::path::Path;

struct ShipwrightTheme;

impl Theme for ShipwrightTheme {
    fn bar_color(&self, state: &ThemeState) -> Style {
        match state {
            ThemeState::Active => Style::new().true_color(242, 84, 45),
            ThemeState::Cancel => Style::new().red(),
            ThemeState::Submit => Style::new().bright().black(),
            ThemeState::Error(_) => Style::new().yellow(),
        }
    }

    fn format_progress_start(&self, template: &str, grouped: bool, last: bool) -> String {
        self.format_progress_with_state(
            &format!("{{spinner:.#f2542d}} {template}"),
            grouped,
            last,
            &ThemeState::Active,
        )
    }
}

pub struct Display {
    progress: Option<MultiProgress>,
    spinners: Vec<ProgressBar>,
}

impl Display {
    pub fn new(packages: &[Package]) -> Self {
        let interactive = io::stdout().is_terminal()
            && io::stderr().is_terminal()
            && std::env::var("TERM").is_ok_and(|term| term != "dumb");
        let progress = interactive.then(|| {
            cliclack::set_theme(ShipwrightTheme);
            cliclack::multi_progress("Shipwright build")
        });
        let mut spinners = Vec::new();
        for package in packages {
            let message = format!("{}: building", package.language.name());
            if let Some(progress) = &progress {
                let spinner = progress.add(cliclack::spinner());
                spinner.start(message);
                spinners.push(spinner);
            } else {
                eprintln!("{message}");
            }
        }
        Self { progress, spinners }
    }

    pub fn complete(&self, index: usize, result: &BuildResult) {
        let status = match result.outcome {
            Outcome::Success => "built",
            Outcome::Failed(_) => "failed",
            Outcome::Cancelled => "cancelled",
        };
        let message = format!(
            "{}: {status} ({:.2}s)",
            result.name,
            result.elapsed.as_secs_f64()
        );
        if let Some(spinner) = self.spinners.get(index) {
            match result.outcome {
                Outcome::Success => spinner.stop(message),
                Outcome::Failed(_) => spinner.error(message),
                Outcome::Cancelled => spinner.cancel(message),
            }
        } else {
            eprintln!("{message}");
        }
    }

    pub fn finish(&self, results: &[BuildResult], logs: &Path) {
        if let Some(progress) = &self.progress {
            if results
                .iter()
                .any(|result| matches!(result.outcome, Outcome::Cancelled))
            {
                progress.cancel();
            } else if results.iter().any(|result| !result.succeeded()) {
                progress.error("Build failed");
            } else {
                progress.stop();
            }
        }
        for result in results {
            if let Outcome::Failed(reason) = &result.outcome {
                eprintln!("\n{} failed: {reason}", result.name);
                match excerpt(&result.log) {
                    Ok(excerpt) if !excerpt.is_empty() => eprintln!("{excerpt}"),
                    Err(error) => eprintln!("Could not read log: {error}"),
                    _ => {}
                }
                eprintln!("Full log: {}", result.log.display());
            }
        }
        eprintln!("Logs: {}", logs.display());
    }
}

fn excerpt(path: &Path) -> io::Result<String> {
    // TODO: Surface compiler diagnostics when trailing build summaries hide the actual error.
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(8192)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = console::strip_ansi_codes(&text);
    let lines: Vec<_> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    Ok(lines[lines.len().saturating_sub(8)..]
        .iter()
        .map(|line| {
            let clean: String = line.chars().filter(|c| !c.is_control()).take(240).collect();
            format!("  {clean}")
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
