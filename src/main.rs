use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::path::{Path, PathBuf};

use cargo_metadata::Message;
use clap::{Parser, ValueEnum};
use log::*;
use serde::Serialize;

#[derive(Parser)]
struct Flags {
    /// Path to the rust workspace relative to the repository root
    #[clap(long)]
    workspace: Option<PathBuf>,
    /// Phabricator API token
    #[clap(long, env = "PHAB_TOKEN")]
    token: String,
    /// Build status
    #[clap(long)]
    status: Status,
    /// Build PHID (PHID-...)
    build_phid: String,
    /// Path to 'cargo clippy --message-format=json' output
    #[clap(long)]
    clippy_json: Option<PathBuf>,
    /// Path to 'cargo check --message-format=json' output
    #[clap(long, conflicts_with = "clippy_json")]
    check_json: Option<PathBuf>,
    /// Path to 'cargo nextest' stderr output
    #[clap(long)]
    nextest_stderr: Option<PathBuf>,
}

#[derive(Debug, Copy, Clone, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum Status {
    Abort,
    Fail,
    Pass,
    Pause,
    Restart,
    Resume,
    Work,
}

#[derive(Debug, Serialize)]
struct Params {
    #[serde(rename = "buildTargetPHID")]
    build: String,
    // receiver: String,
    #[serde(rename = "type")]
    status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<Vec<UnitResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lint: Option<Vec<LintResult>>,
    #[serde(rename = "__conduit__")]
    auth: Auth, // output: OutputFormat,
}
#[derive(Debug, Serialize)]
struct Auth {
    token: String,
}

#[derive(Debug, Serialize)]
struct UnitResult {
    name: String,
    result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    engine: Option<String>,
    #[serde(rename = "duration", skip_serializing_if = "Option::is_none")]
    duration_s: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}
impl UnitResult {
    fn from_nextest(path: &Path) -> anyhow::Result<impl Iterator<Item = Self>> {
        let mut results = HashMap::<(String, String), UnitResult>::new();
        let data = std::fs::read_to_string(path)?;
        let regex = regex::Regex::new(r"([A-Z]+) \[\s*((?:\d|\.)+)s\] (.*?) (.*?)$")?;
        for line in data.lines() {
            let Some(captures) = regex.captures(line) else {
                continue;
            };
            let name = captures.get(4).unwrap().as_str().to_string();
            let namespace = captures.get(3).unwrap().as_str().to_string();
            results.insert(
                (namespace.clone(), name.clone()),
                UnitResult {
                    name,
                    result: captures.get(1).unwrap().as_str().to_lowercase(),
                    duration_s: Some(captures.get(2).unwrap().as_str().parse()?),
                    namespace: Some(namespace),
                    engine: Some("cargo-nextest".into()),
                    coverage: None,
                    path: None,
                    details: None,
                    format: None,
                },
            );
        }
        Ok(results.into_values())
    }
}
#[derive(Debug, Eq, PartialEq, Serialize, Hash)]
struct LintResult {
    name: String,
    code: String,
    severity: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(rename = "char", skip_serializing_if = "Option::is_none")]
    position: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}
impl LintResult {
    fn from_clippy(path: &Path, workspace: &Path) -> anyhow::Result<HashSet<Self>> {
        let mut results = HashSet::new();
        let json = std::fs::read(path)?;
        for msg in Message::parse_stream(json.as_slice()) {
            if let Message::CompilerMessage(msg) = msg? {
                let diag = msg.message;
                let Some(code) = &diag.code else {
                    continue;
                };
                let code = code.code.clone();
                let span = &diag.spans[0];

                let res = LintResult {
                    name: if code.contains("clippy") {
                        "cargo-clippy".into()
                    } else {
                        "cargo-check".into()
                    },
                    code,
                    severity: format!("{:?}", diag.level),
                    path: workspace
                        .join(&span.file_name)
                        .to_string_lossy()
                        .to_string(),
                    line: Some(span.line_start),
                    position: None,
                    description: Some(diag.message),
                };
                results.insert(res);
            }
        }
        Ok(results)
    }
}
fn main_impl() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Flags::parse();
    let workspace = args.workspace.unwrap_or_default();
    let mut lints: Vec<LintResult> = vec![];
    match (args.clippy_json, args.check_json) {
        (Some(path), None) | (None, Some(path)) => {
            match LintResult::from_clippy(&path, &workspace) {
                Ok(res) => lints.extend(res),
                Err(e) => {
                    warn!("Failed to parse clippy/check lints: {:?}", e);
                }
            }
        }
        _ => {}
    }
    let mut units: Vec<UnitResult> = vec![];
    if let Some(path) = args.nextest_stderr {
        match UnitResult::from_nextest(&path) {
            Ok(res) => units.extend(res),
            Err(e) => {
                warn!("Failed to parse nextest results: {:?}", e);
            }
        }
    }
    units.sort_by(|a, b| {
        b.duration_s
            .unwrap_or_default()
            .partial_cmp(&a.duration_s.unwrap_or_default())
            .unwrap()
    });
    let output = Params {
        build: args.build_phid,
        status: args.status,
        unit: Some(units),
        lint: Some(lints),
        auth: Auth { token: args.token },
    };
    print!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn main() {
    if let Err(e) = main_impl() {
        error!("{}", e);
        std::process::exit(2);
    }
}
