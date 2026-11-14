use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};

fn pob_root() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("POB_ROOT") {
        return Some(PathBuf::from(root));
    }
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/pob");
    local.join("Launch.lua").is_file().then_some(local)
}

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    user_dir: PathBuf,
}

impl Server {
    fn start(name: &str, args: &[&str]) -> Option<(Self, Value)> {
        let Some(root) = pob_root() else {
            eprintln!("skipping: no PoB data (run pob-sync or set POB_ROOT)");
            return None;
        };
        let user_dir = std::env::temp_dir().join(format!("pob-engine-serve-{name}-{}", std::process::id()));
        let mut child = Command::new(env!("CARGO_BIN_EXE_pobctl"))
            .arg("--pob-root")
            .arg(root)
            .arg("--user-dir")
            .arg(&user_dir)
            .arg("serve")
            .args(args)
            .env("RUST_LOG", "warn")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn pobctl serve");
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        let mut server = Self { stdin: child.stdin.take(), child, lines, user_dir };
        let ready = server.read();
        assert_eq!(ready["ready"], true, "{ready}");
        Some((server, ready))
    }

    fn read(&mut self) -> Value {
        let line = self.lines.recv_timeout(Duration::from_secs(120)).expect("a reply line");
        serde_json::from_str(&line).expect("reply is JSON")
    }

    fn send(&mut self, req: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{req}").unwrap();
        stdin.flush().unwrap();
    }

    fn ask(&mut self, req: Value) -> Value {
        let id = req["id"].clone();
        self.send(req);
        let reply = self.read();
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    fn ok(&mut self, req: Value) -> Value {
        let reply = self.ask(req);
        assert_eq!(reply["ok"], true, "{reply}");
        reply["r"].clone()
    }

    fn shutdown(mut self) {
        assert_eq!(self.ok(json!({ "id": "bye", "ctl": "shutdown" })), "bye");
        drop(self.stdin.take());
        assert!(self.child.wait().unwrap().success());
        let _ = std::fs::remove_dir_all(&self.user_dir);
    }
}

#[test]
fn serve_answers_requests_and_survives_a_timeout() {
    let Some((mut s, ready)) = Server::start("one", &["--timeout", "1"]) else { return };
    assert!(matches!(ready["game"].as_str(), Some("poe1" | "poe2")), "{ready}");
    assert!(ready["version"].is_string(), "{ready}");
    assert_eq!(ready["workers"], 1);

    assert_eq!(s.ok(json!({ "id": 1, "ctl": "ping" }))["pong"], true);
    let methods = s.ok(json!({ "id": 2, "ctl": "methods" }));
    for m in ["new_build", "get_stats", "version"] {
        assert!(methods.as_array().unwrap().contains(&json!(m)), "{m} missing");
    }
    s.ok(json!({ "id": 3, "m": "new_build", "p": { "name": "serve" } }));
    let stats = s.ok(json!({ "id": 4, "m": "get_stats", "p": { "fields": ["Life"] } }));
    assert!(stats["stats"]["Life"].as_f64().is_some_and(|l| l > 0.0), "{stats}");
    assert_eq!(s.ok(json!({ "id": 5, "lua": "__serve_mark = 1 return 1 + 2" })), 3);

    let err = s.ask(json!({ "id": 6, "m": "no_such_method" }));
    assert_eq!(err["ok"], false);
    assert!(err["err"].as_str().unwrap().contains("no_such_method"), "{err}");
    let err = s.ask(json!({ "id": 7, "nothing": true }));
    assert_eq!(err["ok"], false);

    let timed_out = s.ask(json!({ "id": 8, "lua": "while true do end" }));
    assert_eq!(timed_out["ok"], false, "{timed_out}");
    assert_eq!(timed_out["err"], "timeout");
    assert_eq!(timed_out["restarted"], true);

    // A fresh engine: earlier state is gone, and it answers again.
    assert_eq!(s.ok(json!({ "id": 9, "lua": "return __serve_mark" })), Value::Null);
    s.ok(json!({ "id": 10, "m": "new_build", "p": { "name": "again" } }));
    let stats = s.ok(json!({ "id": 11, "m": "get_stats", "p": { "fields": ["Life"] } }));
    assert!(stats["stats"]["Life"].as_f64().is_some_and(|l| l > 0.0), "{stats}");
    let gc = s.ok(json!({ "id": 12, "ctl": "gc" }));
    assert!(gc["after_mb"].as_f64().is_some(), "{gc}");
    s.shutdown();
}

#[test]
fn serve_routes_to_workers_and_keeps_their_state() {
    let Some((mut s, ready)) = Server::start("pool", &["--workers", "2"]) else { return };
    assert_eq!(ready["workers"], 2);
    s.ok(json!({ "id": "a", "w": 0, "lua": "__serve_tag = 'zero'" }));
    s.ok(json!({ "id": "b", "w": 1, "lua": "__serve_tag = 'one'" }));
    assert_eq!(s.ok(json!({ "id": "c", "w": 0, "lua": "return __serve_tag" })), "zero");
    assert_eq!(s.ok(json!({ "id": "d", "w": 1, "lua": "return __serve_tag" })), "one");

    // Round-robin without "w" reaches both; replies from different workers may arrive in either order.
    s.send(json!({ "id": "e", "lua": "return __serve_tag" }));
    s.send(json!({ "id": "f", "lua": "return __serve_tag" }));
    let mut tags: Vec<String> = (0..2).map(|_| s.read()["r"].as_str().unwrap().to_string()).collect();
    tags.sort();
    assert_eq!(tags, ["one", "zero"]);

    let err = s.ask(json!({ "id": "g", "w": 2, "ctl": "gc" }));
    assert_eq!(err["ok"], false, "{err}");
    s.shutdown();
}
