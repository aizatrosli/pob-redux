//! `pobctl serve`: JSON-lines requests on stdin routed to engine worker processes.
//! Workers are processes rather than threads because a Lua call that overruns
//! cannot be interrupted (JIT-compiled loops skip debug hooks); the process is killed instead.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use pob_engine::{Engine, EngineConfig, Error};
use serde_json::{json, Value};

/// Above this Lua heap a build load is preceded by a full collection instead of a step.
const GC_FULL_MB: f64 = 600.0;

fn emit(v: impl std::fmt::Display) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn fail(id: &Value, err: &str) {
    emit(json!({ "id": id, "ok": false, "err": err }));
}

struct Job {
    line: String,
    id: Value,
    timeout: Duration,
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Proc {
    fn spawn(cfg: &EngineConfig) -> Result<(Self, Value), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(exe);
        cmd.arg("--pob-root").arg(&cfg.pob_root).arg("--user-dir").arg(&cfg.user_dir);
        if let Some(dir) = &cfg.cache_dir {
            cmd.arg("--cache-dir").arg(dir);
        }
        let mut child = cmd
            .arg("serve-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn worker: {e}"))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut proc = Self { child, stdin, lines };
        let ready = proc.lines.recv().ok().and_then(|l| serde_json::from_str::<Value>(&l).ok()).unwrap_or(Value::Null);
        if ready["ready"] != true {
            proc.kill();
            return Err(ready["err"].as_str().unwrap_or("worker exited during boot").to_string());
        }
        Ok((proc, ready))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn run_worker(w: usize, cfg: EngineConfig, jobs: Receiver<Job>, ready: Sender<Result<Value, String>>) {
    let mut proc = match Proc::spawn(&cfg) {
        Ok((p, r)) => {
            let _ = ready.send(Ok(r));
            Some(p)
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    for job in jobs {
        if proc.is_none() {
            match Proc::spawn(&cfg) {
                Ok((p, _)) => proc = Some(p),
                Err(e) => {
                    fail(&job.id, &format!("worker {w} failed to boot: {e}"));
                    continue;
                }
            }
        }
        let p = proc.as_mut().expect("spawned above");
        let sent = writeln!(p.stdin, "{}", job.line).and_then(|_| p.stdin.flush());
        let got = match sent {
            Ok(()) => p.lines.recv_timeout(job.timeout),
            Err(_) => Err(RecvTimeoutError::Disconnected),
        };
        match got {
            Ok(line) => emit(line),
            Err(e) => {
                let err = if e == RecvTimeoutError::Timeout { "timeout" } else { "worker exited" };
                log::warn!("serve: worker {w}: {err}; restarting it");
                p.kill();
                emit(json!({ "id": job.id, "ok": false, "err": err, "restarted": true, "w": w }));
                proc = Proc::spawn(&cfg).map_err(|e| log::error!("serve: worker {w} failed to boot: {e}")).ok().map(|(p, _)| p);
            }
        }
    }
    if let Some(p) = proc {
        drop(p.stdin);
        let mut child = p.child;
        let _ = child.wait();
    }
}

fn limit(secs: f64) -> Duration {
    if secs <= 0.0 {
        return Duration::MAX;
    }
    Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX)
}

pub fn serve(cfg: &EngineConfig, workers: usize, timeout: f64) -> i32 {
    let n = workers.max(1);
    let t0 = Instant::now();
    let (ready_tx, ready_rx) = mpsc::channel();
    let mut queues = Vec::with_capacity(n);
    let mut threads = Vec::with_capacity(n);
    for w in 0..n {
        let (tx, rx) = mpsc::channel::<Job>();
        let (cfg, ready_tx) = (cfg.clone(), ready_tx.clone());
        queues.push(tx);
        threads.push(std::thread::spawn(move || run_worker(w, cfg, rx, ready_tx)));
    }
    drop(ready_tx);
    let mut version = Value::Null;
    for r in ready_rx.iter().take(n) {
        match r {
            Ok(r) => version = r["version"].clone(),
            Err(e) => {
                emit(json!({ "ready": false, "err": e }));
                return 1;
            }
        }
    }
    emit(json!({
        "ready": true,
        "version": version["pobVersion"],
        "game": version["game"],
        "workers": n,
        "boot_ms": t0.elapsed().as_millis() as u64,
    }));

    let mut next = 0usize;
    let mut bye = None;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v @ Value::Object(_)) => v,
            Ok(_) => {
                fail(&Value::Null, "request must be a JSON object");
                continue;
            }
            Err(e) => {
                fail(&Value::Null, &format!("bad json: {e}"));
                continue;
            }
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        match req.get("ctl").map(|c| c.as_str()) {
            Some(Some("ping")) => {
                emit(json!({ "id": id, "ok": true, "r": { "pong": true, "workers": n } }));
                continue;
            }
            Some(Some("shutdown")) => {
                bye = Some(id);
                break;
            }
            Some(Some("gc" | "methods")) => {}
            Some(c) => {
                fail(&id, &format!("unknown ctl {}", c.unwrap_or("(not a string)")));
                continue;
            }
            None if !req["m"].is_string() && !req["lua"].is_string() => {
                fail(&id, "request needs \"m\", \"lua\" or \"ctl\"");
                continue;
            }
            None => {}
        }
        let w = match req.get("w").filter(|w| !w.is_null()) {
            None => {
                next += 1;
                (next - 1) % n
            }
            Some(w) => match w.as_u64() {
                Some(w) if (w as usize) < n => w as usize,
                _ => {
                    fail(&id, &format!("\"w\" must be a worker index below {n}"));
                    continue;
                }
            },
        };
        let timeout = limit(req.get("timeout").and_then(Value::as_f64).unwrap_or(timeout));
        let _ = queues[w].send(Job { line, id, timeout });
    }
    drop(queues);
    for t in threads {
        let _ = t.join();
    }
    if let Some(id) = bye {
        emit(json!({ "id": id, "ok": true, "r": "bye" }));
    }
    0
}

fn request(engine: &Engine, req: &Value) -> Result<Value, Error> {
    if let Some(m) = req["m"].as_str() {
        if m == "new_build" || m.starts_with("load_build") {
            // Free the previous build before PoB builds the next one.
            engine.eval(&format!("if collectgarbage('count') / 1024 > {GC_FULL_MB} then collectgarbage('collect') else collectgarbage('step', 0) end"))?;
        }
        return engine.call(m, req.get("p").unwrap_or(&Value::Null));
    }
    if let Some(code) = req["lua"].as_str() {
        return engine.eval(code);
    }
    match req["ctl"].as_str() {
        Some("methods") => engine.method_names().map(|m| json!(m)),
        Some("gc") => engine.eval(
            "local before = collectgarbage('count') collectgarbage('collect') return { before_mb = before / 1024, after_mb = collectgarbage('count') / 1024 }",
        ),
        other => Err(Error::Other(format!("unknown request {other:?}"))),
    }
}

fn worker_loop(cfg: EngineConfig) -> i32 {
    let engine = match Engine::boot(cfg) {
        Ok(e) => e,
        Err(e) => {
            emit(json!({ "ready": false, "err": e.to_string() }));
            return 1;
        }
    };
    // stdout carries the protocol; Lua's own writes go to stderr.
    let _ = engine.eval("io.output(io.stderr)");
    let version = engine.call("version", &Value::Null).unwrap_or(Value::Null);
    emit(json!({ "ready": true, "boot_ms": engine.boot_ms as u64, "version": version }));
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let req: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let t = Instant::now();
        let res = request(&engine, &req);
        let ms = (t.elapsed().as_secs_f64() * 1e6).round() / 1e3;
        match res {
            Ok(r) => emit(json!({ "id": id, "ok": true, "r": r, "ms": ms })),
            Err(e) => emit(json!({ "id": id, "ok": false, "err": e.to_string(), "ms": ms })),
        }
    }
    0
}

/// The worker side of `serve`, on a thread with the engine's usual 64 MB stack.
pub fn worker(cfg: EngineConfig) -> i32 {
    std::thread::Builder::new()
        .name("pob-engine".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || worker_loop(cfg))
        .expect("spawn pob-engine thread")
        .join()
        .unwrap_or(4)
}
