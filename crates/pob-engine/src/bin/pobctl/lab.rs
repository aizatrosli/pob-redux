//! `pobctl optimise`, `pobctl synth` and `pobctl meta-search`: the exilegen sidecar from the command line
//! (see `pob_engine::lab`). Lab state lives in `$POB_LAB_DIR`, else `<user dir>/lab`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use pob_engine::lab::{JobOptions, LabConfig, SearchOptions};
use serde_json::Value;

#[derive(Args)]
pub struct Scoring {
    /// exilegen constraint, e.g. one_button, no_cast:strict, forbid_gem:X (repeatable).
    #[arg(long = "constraint")]
    constraints: Vec<String>,
    /// Scoring profile (repeatable; default pinnacle_v1 + mapping_v1).
    #[arg(long = "profile")]
    profiles: Vec<String>,
    /// Engine evaluations (default 6000).
    #[arg(long)]
    budget: Option<u64>,
    #[arg(long)]
    seed: Option<u64>,
    /// Seconds before the job is killed; 0 for no limit.
    #[arg(long, default_value_t = 1800)]
    timeout: u64,
}

impl Scoring {
    fn job(&self, steps: Option<String>) -> JobOptions {
        JobOptions {
            constraints: self.constraints.clone(),
            profiles: self.profiles.clone(),
            budget: self.budget,
            seed: self.seed,
            steps,
            timeout: (self.timeout > 0).then(|| Duration::from_secs(self.timeout)),
        }
    }
}

#[derive(Subcommand)]
pub enum MetaSearch {
    /// Start (or resume) a search in the background.
    Start {
        /// Run name (letters, digits, '-', '_'); re-use it to resume.
        run: String,
        #[arg(long = "constraint")]
        constraints: Vec<String>,
        #[arg(long = "profile")]
        profiles: Vec<String>,
        /// Engines (default min(3, cores - 1)).
        #[arg(long)]
        workers: Option<u64>,
        /// Wall-clock budget in minutes (default 60).
        #[arg(long)]
        minutes: Option<f64>,
        /// Engine evaluations per skeleton (default 6000).
        #[arg(long)]
        budget: Option<u64>,
        #[arg(long)]
        max_jobs: Option<u64>,
        #[arg(long)]
        seed: Option<u64>,
    },
    /// Progress of a run.
    Status { run: String },
    /// Best build of each archive cell, best first.
    Results {
        run: String,
        #[arg(long, default_value_t = 20)]
        top: u64,
        /// Include each elite's PoB code.
        #[arg(long)]
        codes: bool,
        /// Also list feasible builds with mods PoB cannot model.
        #[arg(long)]
        unverified: bool,
    },
    /// Ask a run to stop after the skeletons in flight.
    Stop { run: String },
    /// Every run, newest first.
    List,
}

fn config(pob_root: &Path, user_dir: &Path) -> Result<LabConfig, String> {
    let state = std::env::var_os("POB_LAB_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| user_dir.join("lab"));
    LabConfig::discover(pob_root, &state, std::env::current_exe().ok())
}

fn read_code(file: &Path) -> Result<String, String> {
    let text = if file == Path::new("-") {
        std::io::read_to_string(std::io::stdin()).map_err(|e| e.to_string())?
    } else {
        std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?
    };
    let text = text.trim();
    if text.starts_with('<') {
        return Err("give a PoB share code (pobctl call export_build prints one for an XML build)".into());
    }
    Ok(text.to_string())
}

pub fn optimise(pob_root: &Path, user_dir: &Path, file: &Path, steps: Option<String>, scoring: &Scoring) -> Result<Value, String> {
    let lab = config(pob_root, user_dir)?;
    lab.optimise(&read_code(file)?, &scoring.job(steps))
}

pub fn synth(pob_root: &Path, user_dir: &Path, skeleton: &str, scoring: &Scoring) -> Result<Value, String> {
    let lab = config(pob_root, user_dir)?;
    let text = match skeleton.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
        None => skeleton.to_string(),
    };
    let sk: Value = serde_json::from_str(&text).map_err(|e| format!("skeleton JSON: {e}"))?;
    lab.synth(&sk, &scoring.job(None))
}

pub fn meta_search(pob_root: &Path, user_dir: &Path, cmd: &MetaSearch) -> Result<Value, String> {
    let lab = config(pob_root, user_dir)?;
    match cmd {
        MetaSearch::Start { run, constraints, profiles, workers, minutes, budget, max_jobs, seed } => lab.search_start(
            run,
            &SearchOptions {
                constraints: constraints.clone(),
                profiles: profiles.clone(),
                workers: *workers,
                minutes: *minutes,
                budget: *budget,
                max_jobs: *max_jobs,
                seed: *seed,
            },
        ),
        MetaSearch::Status { run } => lab.search_status(run),
        MetaSearch::Results { run, top, codes, unverified } => lab.search_results(run, *top, *codes, *unverified),
        MetaSearch::Stop { run } => lab.search_stop(run),
        MetaSearch::List => lab.search_list(),
    }
}
