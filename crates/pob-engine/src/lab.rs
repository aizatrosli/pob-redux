//! Build optimiser and meta-search through the exilegen sidecar.
//!
//! [exilegen](https://github.com/aizatrosli/exilegen) holds the search (support, tree and trigger
//! optimisers, the from-scratch builder and its MAP-Elites archive) and the honest scoring layer
//! (condition registry, meta-gem energy, locked profiles, floors). It runs as a Python process that
//! drives `pobctl serve` and its `search_*` methods, so every number is still PoB's own. This module
//! finds the pieces, runs `python -m exilegen.lab` jobs (one JSON object out) and manages long
//! searches in run directories. A native port can replace the sidecar behind the same functions.
//!
//! Discovery (first match wins):
//! - Python: `POB_LAB_PYTHON`, else `python3` (`python` on Windows).
//! - exilegen checkout (the directory holding the `exilegen/` package): `POB_LAB_EXILEGEN`, else
//!   `exilegen/` or `../exilegen/` next to the executable or the working directory.
//! - pobctl: the explicit path, `POB_LAB_POBCTL`, `EXG_POBCTL`, else `pobctl` next to the executable.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// What exilegen's lab protocol version this client speaks.
pub const LAB_VERSION: u64 = 1;

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

#[derive(Debug, Clone)]
pub struct LabConfig {
    pub python: PathBuf,
    /// The exilegen checkout: the directory that holds the `exilegen/` package.
    pub exilegen: PathBuf,
    pub pobctl: PathBuf,
    /// PoB-PoE2 data root (Launch.lua), as for the engine.
    pub pob_root: PathBuf,
    /// Lab state: engine user dir and cache, job scratch files, and `runs/` for meta searches.
    pub state_dir: PathBuf,
}

/// Options shared by optimise and synth jobs.
#[derive(Debug, Clone, Default)]
pub struct JobOptions {
    /// exilegen constraints, e.g. `one_button`, `no_cast:strict`, `forbid_gem:X`.
    pub constraints: Vec<String>,
    /// Scoring profiles (default: `pinnacle_v1` + `mapping_v1`).
    pub profiles: Vec<String>,
    /// Engine evaluations (default 6000).
    pub budget: Option<u64>,
    pub seed: Option<u64>,
    /// optimise only: `auto` or a comma list of `convert,tree,tree_scratch,supports,triggers`.
    pub steps: Option<String>,
    /// Wall-clock limit for the job; the process is killed past it.
    pub timeout: Option<Duration>,
}

/// Options for a meta search (`python -m exilegen build`).
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub constraints: Vec<String>,
    pub profiles: Vec<String>,
    /// Engines (default: exilegen's min(3, cores - 1)).
    pub workers: Option<u64>,
    /// Wall-clock budget of this session (default 60).
    pub minutes: Option<f64>,
    /// Engine evaluations per skeleton (default 6000).
    pub budget: Option<u64>,
    pub max_jobs: Option<u64>,
    pub seed: Option<u64>,
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn is_exilegen(dir: &Path) -> bool {
    dir.join("exilegen").join("lab.py").is_file()
}

impl LabConfig {
    /// Find Python, exilegen and pobctl. The error says which piece is missing and how to point at it.
    pub fn discover(pob_root: &Path, state_dir: &Path, pobctl: Option<PathBuf>) -> Result<Self, String> {
        let python = env_path("POB_LAB_PYTHON").unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));

        let mut roots: Vec<PathBuf> = env_path("POB_LAB_EXILEGEN").into_iter().collect();
        for base in [exe_dir(), std::env::current_dir().ok()].into_iter().flatten() {
            roots.push(base.join("exilegen"));
            if let Some(parent) = base.parent() {
                roots.push(parent.join("exilegen"));
            }
        }
        let exilegen = roots.into_iter().find(|d| is_exilegen(d)).ok_or(
            "exilegen not found: set POB_LAB_EXILEGEN to an exilegen checkout (the folder holding the exilegen/ package)",
        )?;

        let pobctl = pobctl
            .into_iter()
            .chain(env_path("POB_LAB_POBCTL"))
            .chain(env_path("EXG_POBCTL"))
            .chain(exe_dir().map(|d| d.join(format!("pobctl{EXE}"))))
            .find(|p| p.is_file())
            .ok_or("pobctl not found: build it (cargo build --release -p pob-engine --bin pobctl) or set POB_LAB_POBCTL")?;

        Ok(Self { python, exilegen, pobctl, pob_root: pob_root.to_path_buf(), state_dir: state_dir.to_path_buf() })
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.state_dir.join("runs")
    }

    fn command(&self, module: &str) -> Command {
        let mut cmd = Command::new(&self.python);
        cmd.arg("-m").arg(module).current_dir(&self.exilegen);
        let mut pythonpath = std::ffi::OsString::from(&self.exilegen);
        if let Some(old) = std::env::var_os("PYTHONPATH").filter(|v| !v.is_empty()) {
            pythonpath.push(if cfg!(windows) { ";" } else { ":" });
            pythonpath.push(old);
        }
        cmd.env("PYTHONPATH", pythonpath)
            .env("PYTHONUNBUFFERED", "1")
            .env("EXG_BACKEND", "pobctl")
            .env("EXG_POBCTL", &self.pobctl)
            .env("EXG_POB_ROOT", &self.pob_root)
            .env("EXG_STATE", self.state_dir.join("engine"));
        cmd
    }

    /// Run `python -m exilegen.lab <args>` and return its JSON object. An `{"error"}` reply, a crash
    /// or a timeout is an `Err` carrying the reason (and the tail of stderr for a crash).
    pub fn run(&self, args: &[String], timeout: Option<Duration>) -> Result<Value, String> {
        let jobs = self.state_dir.join("jobs");
        std::fs::create_dir_all(&jobs).map_err(|e| format!("lab state dir {}: {e}", jobs.display()))?;
        let log_path = jobs.join(format!("{}.log", stamp()));
        let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
        let mut child = self
            .command("exilegen.lab")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .map_err(|e| format!("start {}: {e}", self.python.display()))?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stdout.read_to_string(&mut s);
            s
        });
        let status = wait(&mut child, timeout);
        let out = reader.join().unwrap_or_default();
        let stderr_tail = || {
            let s = std::fs::read_to_string(&log_path).unwrap_or_default();
            let lines: Vec<&str> = s.lines().rev().take(12).collect();
            lines.into_iter().rev().collect::<Vec<_>>().join("\n")
        };
        let status = match status {
            Some(s) => s,
            None => return Err(format!("timed out after {}s (log: {})", timeout.unwrap_or_default().as_secs(), log_path.display())),
        };
        let reply = out.lines().rev().find_map(|l| serde_json::from_str::<Value>(l.trim()).ok().filter(Value::is_object));
        let _ = if status.success() { std::fs::remove_file(&log_path) } else { Ok(()) };
        match reply {
            Some(v) if v.get("error").is_some_and(|e| !e.is_null()) => Err(v["error"].as_str().unwrap_or("error").to_string()),
            Some(v) => Ok(v),
            None => Err(format!("exilegen exited with {status} and no result:\n{}", stderr_tail())),
        }
    }

    /// Lab protocol version, backend and engine paths, as exilegen sees them.
    pub fn version(&self) -> Result<Value, String> {
        let v = self.run(&["version".into()], Some(Duration::from_secs(60)))?;
        if v["lab"].as_u64() != Some(LAB_VERSION) {
            return Err(format!("exilegen lab protocol {} (expected {LAB_VERSION}): update exilegen", v["lab"]));
        }
        Ok(v)
    }

    /// Optimise a build (share code): tree, supports and triggers under honest scoring.
    /// Returns exilegen's result: `start`, `end`, `gain`, `changes`, `steps`, `explain`, `code`, `evals`.
    pub fn optimise(&self, code: &str, opts: &JobOptions) -> Result<Value, String> {
        let file = self.scratch_file("code", code.trim())?;
        let mut args = vec!["optimise".into(), "--file".into(), file.to_string_lossy().into_owned()];
        if let Some(steps) = &opts.steps {
            args.extend(["--steps".into(), steps.clone()]);
        }
        push_scoring(&mut args, opts);
        let out = self.run(&args, opts.timeout);
        let _ = std::fs::remove_file(&file);
        out
    }

    /// Build a skeleton (class, ascendancy, main skill, uniques, trigger, ...) from nothing.
    /// Returns `ok`, `code`, `score`, `summary`, `notes`.
    pub fn synth(&self, skeleton: &Value, opts: &JobOptions) -> Result<Value, String> {
        let file = self.scratch_file("json", &skeleton.to_string())?;
        let mut args = vec!["synth".into(), "--skeleton".into(), format!("@{}", file.display())];
        push_scoring(&mut args, opts);
        let out = self.run(&args, opts.timeout);
        let _ = std::fs::remove_file(&file);
        out
    }

    fn scratch_file(&self, ext: &str, text: &str) -> Result<PathBuf, String> {
        let dir = self.state_dir.join("jobs");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let p = dir.join(format!("{}.{ext}", stamp()));
        std::fs::write(&p, text).map_err(|e| e.to_string())?;
        Ok(p)
    }

    fn run_dir(&self, run: &str) -> Result<PathBuf, String> {
        if run.is_empty() || run.len() > 64 || !run.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(format!("bad run name {run:?}: use letters, digits, '-' and '_'"));
        }
        Ok(self.runs_dir().join(run))
    }

    /// Start a meta search in `runs/<run>` in the background. Re-using a run name resumes it.
    pub fn search_start(&self, run: &str, opts: &SearchOptions) -> Result<Value, String> {
        let dir = self.run_dir(run)?;
        if let Ok(st) = self.search_status(run) {
            if st["running"] == true {
                return Err(format!("run {run} is already running"));
            }
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(dir.join("STOP"));
        let mut args: Vec<String> = vec!["build".into(), "--run-dir".into(), dir.to_string_lossy().into_owned(), "-v".into()];
        for c in &opts.constraints {
            args.extend(["--constraint".into(), c.clone()]);
        }
        for p in &opts.profiles {
            args.extend(["--profile".into(), p.clone()]);
        }
        let mut num = |flag: &str, v: Option<String>| {
            if let Some(v) = v {
                args.extend([flag.to_string(), v]);
            }
        };
        num("--workers", opts.workers.map(|v| v.to_string()));
        num("--minutes", opts.minutes.map(|v| v.to_string()));
        num("--budget", opts.budget.map(|v| v.to_string()));
        num("--max-jobs", opts.max_jobs.map(|v| v.to_string()));
        num("--seed", opts.seed.map(|v| v.to_string()));
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("lab.log")).map_err(|e| e.to_string())?;
        let err = log.try_clone().map_err(|e| e.to_string())?;
        let mut child = self
            .command("exilegen")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(err)
            .spawn()
            .map_err(|e| format!("start {}: {e}", self.python.display()))?;
        let pid = child.id();
        let meta = json!({ "run": run, "pid": pid, "started": now_secs(), "args": args });
        std::fs::write(dir.join("lab.json"), serde_json::to_string_pretty(&meta).unwrap_or_default()).map_err(|e| e.to_string())?;
        // Reap the child when it exits, so a finished search is not reported as running.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(json!({ "run": run, "run_dir": dir, "pid": pid, "started": true }))
    }

    /// Progress of a run: `running`, `stopping`, and exilegen's `progress.json` (jobs, cells, QD score, best).
    pub fn search_status(&self, run: &str) -> Result<Value, String> {
        let dir = self.run_dir(run)?;
        if !dir.is_dir() {
            return Err(format!("no run named {run}"));
        }
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
        let meta = read("lab.json").unwrap_or(Value::Null);
        let progress = read("progress.json").unwrap_or(Value::Null);
        let running = meta["pid"].as_u64().is_some_and(|p| pid_alive(p as u32)) && progress["final"] != true;
        let log_tail = std::fs::read_to_string(dir.join("lab.log"))
            .map(|s| s.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        Ok(json!({
            "run": run,
            "run_dir": dir,
            "running": running,
            "stopping": dir.join("STOP").exists(),
            "started": meta["started"],
            "progress": progress,
            "report": dir.join("report.md").is_file().then(|| dir.join("report.md")),
            "log_tail": log_tail,
        }))
    }

    /// The best build of each archive cell, best first. `codes` adds each elite's PoB code.
    pub fn search_results(&self, run: &str, top: u64, codes: bool, include_unverified: bool) -> Result<Value, String> {
        let dir = self.run_dir(run)?;
        if !dir.join("archive.jsonl").is_file() {
            return Err(format!("run {run} has no results yet"));
        }
        let mut args: Vec<String> = vec!["results".into(), "--run-dir".into(), dir.to_string_lossy().into_owned(), "--top".into(), top.to_string()];
        args.extend(["--tiers".into(), if include_unverified { "0,1" } else { "0" }.into()]);
        if codes {
            args.push("--codes".into());
        }
        self.run(&args, Some(Duration::from_secs(300)))
    }

    /// Ask a run to stop: it finishes the skeletons in flight, writes its report and exits.
    pub fn search_stop(&self, run: &str) -> Result<Value, String> {
        let dir = self.run_dir(run)?;
        if !dir.is_dir() {
            return Err(format!("no run named {run}"));
        }
        std::fs::write(dir.join("STOP"), b"").map_err(|e| e.to_string())?;
        Ok(json!({ "run": run, "stopping": true }))
    }

    /// Every run with its status, newest first.
    pub fn search_list(&self) -> Result<Value, String> {
        let mut runs: Vec<(u64, Value)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.runs_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Ok(st) = self.search_status(&name) {
                    let started = st["started"].as_f64().unwrap_or(0.0) as u64;
                    let p = &st["progress"];
                    runs.push((started, json!({
                        "run": name, "running": st["running"], "started": st["started"], "jobs": p["jobs"],
                        "cells_feasible": p["cells_feasible"], "cells_verified": p["cells_verified"], "best": p["best"],
                    })));
                }
            }
        }
        runs.sort_by_key(|r| std::cmp::Reverse(r.0));
        Ok(json!({ "runs": runs.into_iter().map(|(_, v)| v).collect::<Vec<_>>() }))
    }
}

fn push_scoring(args: &mut Vec<String>, opts: &JobOptions) {
    for c in &opts.constraints {
        args.extend(["--constraint".into(), c.clone()]);
    }
    for p in &opts.profiles {
        args.extend(["--profile".into(), p.clone()]);
    }
    if let Some(b) = opts.budget {
        args.extend(["--budget".into(), b.to_string()]);
    }
    if let Some(s) = opts.seed {
        args.extend(["--seed".into(), s.to_string()]);
    }
}

fn wait(child: &mut Child, timeout: Option<Duration>) -> Option<std::process::ExitStatus> {
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) => {}
            Err(_) => return None,
        }
        if timeout.is_some_and(|t| t0.elapsed() > t) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn stamp() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}-{:09}-{}", d.as_secs(), d.subsec_nanos(), std::process::id())
}

fn pid_alive(pid: u32) -> bool {
    if cfg!(windows) {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    } else {
        Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(dir: &Path) -> LabConfig {
        LabConfig { python: "python3".into(), exilegen: dir.into(), pobctl: dir.join("pobctl"), pob_root: dir.into(), state_dir: dir.join("state") }
    }

    #[test]
    fn run_names_are_plain() {
        let c = cfg(&std::env::temp_dir());
        assert!(c.run_dir("one_button-2").is_ok());
        for bad in ["", "../x", "a/b", "a b", &"x".repeat(65)] {
            assert!(c.run_dir(bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn stop_and_status_use_the_run_dir() {
        let dir = std::env::temp_dir().join(format!("pob-lab-test-{}", stamp()));
        let c = cfg(&dir);
        assert!(c.search_status("r1").is_err());
        std::fs::create_dir_all(c.runs_dir().join("r1")).unwrap();
        std::fs::write(c.runs_dir().join("r1").join("progress.json"), r#"{"jobs": 3, "final": false}"#).unwrap();
        let st = c.search_status("r1").unwrap();
        assert_eq!(st["running"], false);
        assert_eq!(st["progress"]["jobs"], 3);
        c.search_stop("r1").unwrap();
        assert_eq!(c.search_status("r1").unwrap()["stopping"], true);
        assert_eq!(c.search_list().unwrap()["runs"][0]["run"], "r1");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn current_process_is_alive() {
        assert!(pid_alive(std::process::id()));
    }
}
