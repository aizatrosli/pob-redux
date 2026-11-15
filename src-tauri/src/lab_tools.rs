//! MCP tools for the build optimiser, meta search and mechanics graph, backed by the exilegen sidecar
//! (`pob_engine::lab`). PoE2 only: exilegen's scoring data (condition registry, energy model,
//! profiles) is PoE2's.

use pob_engine::lab::{JobOptions, LabConfig, SearchOptions};
use serde_json::{json, Map, Value};
use tauri::Manager;

use crate::game::Game;
use crate::tools::{arg_bool, arg_i64, arg_list, arg_str, obj, prop, req_str, JsonObject, ToolContext, ToolDef, ToolError};

pub(crate) const NAMES: &[&str] = &["optimise_build", "build_from_idea", "meta_search_start", "meta_search_status", "meta_search_results", "meta_search_stop", "graph_chains", "chain_build"];

const CHECKPOINT: &str = "before optimise_build";

fn strings() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

fn with_desc(mut v: Value, desc: &str) -> Value {
    v["description"] = json!(desc);
    v
}

fn scoring_props() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(
        "constraints".into(),
        with_desc(strings(), "e.g. one_button, no_cast:strict, no_minions, forbid_gem:X"),
    );
    m
}

pub(crate) fn defs() -> Vec<ToolDef> {
    let def = |name: &'static str, description: &str, schema: Value, read_only: bool, slow: bool| ToolDef {
        name,
        description: description.into(),
        schema,
        output_schema: None,
        read_only,
        destructive: false,
        idempotent: false,
        open_world: false,
        slow,
    };
    let mut optimise = scoring_props();
    optimise.insert("budget".into(), prop("integer", "Evaluations (default 6000)"));
    optimise.insert("steps".into(), prop("string", "auto, or a list of tree, supports, triggers, convert"));
    optimise.insert("apply".into(), prop("boolean", "Open the result"));

    let mut idea = scoring_props();
    for (k, v) in [
        ("class", prop("string", "e.g. Witch")),
        ("ascendancy", prop("string", "e.g. Lich")),
        ("main_skill", prop("string", "Gem name or id")),
        ("uniques", with_desc(strings(), "Uniques to build around")),
        ("supports", strings()),
        ("keystones", strings()),
        ("extra_skills", strings()),
        ("trigger", json!({ "type": "object", "description": "{meta, feeder, spells}, gem ids" })),
        ("weapon", prop("string", "Staff, Bow")),
        ("offhand", prop("string", "Shield, Focus")),
        ("defence", json!({ "type": "string", "enum": ["life", "es", "hybrid", "ci", "mom", "armour", "evasion"] })),
        ("apply", prop("boolean", "Open the result")),
    ] {
        idea.insert(k.into(), v);
    }

    let run = || prop("string", "Run name");
    let mut start = scoring_props();
    start.insert("run".into(), run());
    start.insert("minutes".into(), prop("number", "Default 60"));
    start.insert("workers".into(), prop("integer", "Engines, 1 GB each (default 3)"));

    vec![
        def(
            "optimise_build",
            "Rework the open build's passive tree, supports and triggers with exilegen, scored honestly: locked boss and map configs, conditional gems discounted, meta-gem Energy capping triggers, floors of 75% res, 16k EHP and 4.4k max hit. Returns `start`/`end` (honest and raw DPS, EHP, max hit), `gain`, `changes`, `unverified` (mods PoB cannot model) and `code`. `apply` checkpoints the build (rollback undoes) and opens the result. Takes minutes.",
            obj(Value::Object(optimise), &[]),
            false,
            true,
        ),
        def(
            "build_from_idea",
            "Build an idea from nothing: exilegen picks supports, template rare gear and a tree, scored as optimise_build. Template gear keeps DPS low, so compare ideas with each other. Returns `feasible`, honest DPS, EHP, `notes` and `code`; `apply` checkpoints and opens it. Takes minutes.",
            obj(Value::Object(idea), &["class", "ascendancy", "main_skill"]),
            false,
            true,
        ),
        def(
            "meta_search_start",
            "Start a background search for new archetypes, each idea built as build_from_idea does. Leaves the open build alone. Re-using a run name resumes it.",
            obj(Value::Object(start), &["run"]),
            false,
            false,
        ),
        def(
            "meta_search_status",
            "A meta search's progress: running, ideas built, feasible cells, best find. No `run`: every run.",
            obj(json!({ "run": run() }), &[]),
            true,
            false,
        ),
        def(
            "meta_search_results",
            "Best build per cell of a meta search, best first: class, skill, uniques, trigger, honest DPS, EHP, `verified`, thesis. `codes` adds PoB codes for load_build.",
            obj(
                json!({
                    "run": run(),
                    "top": prop("integer", "Default 10"),
                    "codes": { "type": "boolean" },
                    "include_unverified": prop("boolean", "Default true"),
                }),
                &["run"],
            ),
            true,
            false,
        ),
        def(
            "meta_search_stop",
            "Stop a meta search once its builds in flight end.",
            obj(json!({ "run": run() }), &["run"]),
            false,
            false,
        ),
        def(
            "graph_chains",
            "Synergy chains from the mechanics graph, the Graph view's list: one part makes a state (a charge, an ailment, Rage) and another pays off on it. Best first, each with `id`, `kind`, `main` skill, `parts`, `links`, `thesis`, `score`, `fitted` (expected damage multiple, fitted to PoB runs), `doubts` (links that do not hold up when their text is read again: say so), `online_level`, `farm` (uniques to find) and `spirit_gems` (heralds and auras that fit, with cost and reason). `measured` is what PoB got for the built chain when `scores` names a run. `chain` returns one chain in full: evidence lines, leveling steps, Spirit gems passed over. Changes nothing; about a second. Build one with chain_build.",
            obj(
                json!({
                    "class": prop("string", "e.g. Witch"),
                    "ascendancy": prop("string", "e.g. Lich"),
                    "main_skill": prop("string", "Only chains of this main skill (part of the name)"),
                    "damage_types": with_desc(strings(), "Main skill deals one of: Fire, Cold, Lightning, Physical, Chaos"),
                    "uniques": with_desc(strings(), "Chains must use one of these"),
                    "keystones": with_desc(strings(), "Chains must use one of these"),
                    "kinds": json!({ "type": "array", "items": { "type": "string", "enum": ["self_feed", "enabler", "graft", "bridge"] } }),
                    "online_by": prop("integer", "Only chains complete by this character level"),
                    "level": prop("integer", "Pick Spirit gems for the Spirit held at this level"),
                    "spirit_gear": prop("integer", "Spirit from equipment, with `level`"),
                    "rank_by": json!({ "type": "string", "enum": ["formula", "fitted", "measured"], "description": "Default formula; measured needs `scores`" }),
                    "scores": prop("string", "Chain scoring run directory"),
                    "chain": prop("string", "One chain id, in full"),
                    "top": prop("integer", "Default 10"),
                }),
                &[],
            ),
            true,
            false,
        ),
        def(
            "chain_build",
            "Build one graph_chains chain from nothing with exilegen, scored as optimise_build. Returns `code`, honest DPS and EHP, the main skill's supports and the chain's `parts` with `included`. `level` builds what a character of that level has. `exclude_supports` keeps support gems out and reports what replaced each. `spirit_gems` adds heralds and auras: `auto` for the chain's own picks or a list of names; `spirit` then reports the Spirit reserved and the DPS with and without them. `apply` checkpoints and opens the result. Takes minutes.",
            obj(
                json!({
                    "chain": prop("string", "Chain id from graph_chains"),
                    "level": prop("integer", "Character level (default: end level)"),
                    "exclude_supports": with_desc(strings(), "Support gem names"),
                    "spirit_gems": with_desc(strings(), "[\"auto\"] or gem names"),
                    "spirit_gear": prop("integer", "Spirit from equipment"),
                    "apply": prop("boolean", "Open the result"),
                }),
                &["chain"],
            ),
            false,
            true,
        ),
    ]
}

fn lab(ctx: &ToolContext) -> Result<LabConfig, ToolError> {
    if ctx.game() != Game::Poe2 {
        return Err(ToolError::Failed("the optimiser and meta search are PoE2 only".into()));
    }
    let state = ctx.app.state::<crate::AppState>();
    let dir = ctx.app.path().app_data_dir().map_err(|e| ToolError::Failed(e.to_string()))?.join("lab");
    LabConfig::discover(&state.pob_root(), &dir, None).map_err(ToolError::Failed)
}

/// The Graph view's requirements for one graph_chains call (exilegen `graphx.Requirements`).
fn graph_requirements(args: &JsonObject) -> Result<Value, ToolError> {
    let mut req = Map::new();
    for (arg, field) in [("class", "cls"), ("ascendancy", "ascendancy"), ("rank_by", "rank_by")] {
        if let Some(v) = arg_str(args, arg).filter(|v| !v.trim().is_empty()) {
            req.insert(field.into(), json!(v));
        }
    }
    for (arg, field) in [("damage_types", "damage_types"), ("uniques", "uniques"), ("keystones", "keystones"), ("kinds", "chain_kinds")] {
        let v = str_list(args, arg);
        if !v.is_empty() {
            req.insert(field.into(), json!(v));
        }
    }
    if let Some(v) = u64_arg(args, "online_by")? {
        req.insert("online_by".into(), json!(v));
    }
    if let Some(level) = u64_arg(args, "level")? {
        req.insert("spirit_source".into(), json!("level"));
        req.insert("spirit_level".into(), json!(level.clamp(1, 100)));
        req.insert("spirit_gear".into(), json!(u64_arg(args, "spirit_gear")?.unwrap_or(0)));
    }
    Ok(Value::Object(req))
}

/// Arguments of `exilegen.lab graph`. `requirements` is the path of the requirements file.
fn graph_args(args: &JsonObject, requirements: &str) -> Result<Vec<String>, ToolError> {
    let mut out = vec!["graph".to_string(), "--requirements".into(), format!("@{requirements}")];
    let scores = arg_str(args, "scores").or_else(|| std::env::var("POB_LAB_CHAIN_SCORES").ok());
    for (flag, v) in [("--scores", scores), ("--main", arg_str(args, "main_skill")), ("--chain", arg_str(args, "chain"))] {
        if let Some(v) = v.filter(|v| !v.trim().is_empty()) {
            out.extend([flag.to_string(), v]);
        }
    }
    out.extend(["--top".to_string(), u64_arg(args, "top")?.unwrap_or(10).clamp(1, 100).to_string()]);
    Ok(out)
}

fn str_list(args: &JsonObject, key: &str) -> Vec<String> {
    arg_list(args, key).into_iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

fn u64_arg(args: &JsonObject, key: &str) -> Result<Option<u64>, ToolError> {
    Ok(arg_i64(args, key)?.map(|v| v.max(0) as u64))
}

fn job(args: &JsonObject) -> Result<JobOptions, ToolError> {
    Ok(JobOptions {
        constraints: str_list(args, "constraints"),
        profiles: Vec::new(),
        budget: u64_arg(args, "budget")?,
        seed: None,
        steps: arg_str(args, "steps"),
        timeout: Some(std::time::Duration::from_secs(1800)),
    })
}

/// Facts of one side (start or end) of an optimise result, without the long flag list.
fn side(v: &Value) -> Value {
    let mut out = json!({
        "objective": v["objective"], "feasible": v["feasible"], "verified": v["verified"],
        "dps_honest": v["dps_honest"], "dps_raw": v["dps_raw"], "ehp": v["ehp"],
        "binding_profile": v["binding_profile"], "violations": v["violations"], "below_target": v["below_target"],
    });
    if let Some(profiles) = v["profiles"].as_object() {
        let per: Map<String, Value> = profiles
            .iter()
            .map(|(k, p)| {
                (k.clone(), json!({
                    "dps_honest": p["dps_honest"], "dps_raw": p["dps_raw"], "ehp": p["ehp"], "max_hit": p["max_hit_min"],
                    "movement_speed": p["movement_speed"], "spirit_unreserved": p["spirit_unreserved"], "main_skill": p["main_skill"],
                }))
            })
            .collect();
        out["profiles"] = Value::Object(per);
    }
    out
}

fn unverified(flags: &Value) -> Value {
    let mut seen: Vec<String> = flags
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|f| f.contains("unverified"))
                .map(|f| f.split_once(':').map_or(f, |(_, rest)| rest).to_string())
                .collect()
        })
        .unwrap_or_default();
    seen.sort();
    seen.dedup();
    json!(seen)
}

fn compact_optimise(r: &Value) -> Value {
    let steps: Vec<Value> = r["steps"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| json!({
                    "step": s["name"], "improved": s["improved"], "start_objective": s["start_objective"],
                    "end_objective": s["end_objective"], "changes": s["changes"], "evals": s["evals"],
                }))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "constraints": r["constraints"], "profiles": r["profiles"],
        "start": side(&r["start"]), "end": side(&r["end"]), "gain": r["gain"],
        "changes": r["changes"], "steps": steps, "unverified": unverified(&r["end"]["flags"]),
        "evals": r["evals"], "seconds": r["seconds"], "code": r["code"],
    })
}

/// Checkpoint the open build and open `code`. Returns the checkpoint label.
fn open(ctx: &ToolContext, code: &str) -> Result<Value, ToolError> {
    ctx.call("checkpoint", json!({ "label": CHECKPOINT }))?;
    ctx.call("load_build_code", json!({ "code": code }))?;
    Ok(json!(CHECKPOINT))
}

/// Runs one lab tool. Returns the result and whether the open build changed.
pub(crate) fn run(ctx: &ToolContext, name: &str, args: &JsonObject) -> Result<(Value, bool), ToolError> {
    let lab = lab(ctx)?;
    match name {
        "optimise_build" => {
            let code = ctx.call("save_build_code", Value::Null)?;
            let code = code.as_str().or_else(|| code["code"].as_str()).ok_or_else(|| ToolError::Failed("could not export the open build".into()))?.to_string();
            let r = lab.optimise(&code, &job(args)?).map_err(ToolError::Failed)?;
            let mut out = compact_optimise(&r);
            let apply = arg_bool(args, "apply") == Some(true);
            if apply {
                if let Some(code) = r["code"].as_str() {
                    out["checkpoint"] = open(ctx, code)?;
                }
            }
            out["applied"] = json!(apply);
            Ok((out, apply))
        }
        "build_from_idea" => {
            let mut sk = json!({
                "cls": req_str(args, "class")?,
                "ascendancy": req_str(args, "ascendancy")?,
                "main_skill": req_str(args, "main_skill")?,
                "source": "user",
            });
            for k in ["uniques", "supports", "keystones", "extra_skills"] {
                let v = str_list(args, k);
                if !v.is_empty() {
                    sk[k] = json!(v);
                }
            }
            for k in ["weapon", "offhand", "defence"] {
                if let Some(v) = arg_str(args, k) {
                    sk[k] = json!(v);
                }
            }
            if let Some(t) = args.get("trigger").filter(|t| t.is_object()) {
                sk["trigger"] = t.clone();
            }
            let r = lab.synth(&sk, &job(args)?).map_err(ToolError::Failed)?;
            let s = &r["score"];
            let mut out = json!({
                "ok": r["ok"], "error": r["error"], "feasible": s["feasible"], "verified": s["verified"],
                "objective": s["objective"], "dps_honest": s["dps_honest"], "dps_raw": s["dps_raw"], "ehp": s["ehp"],
                "violations": s["violations"], "below_target": s["below_target"], "unverified": unverified(&s["flags"]),
                "notes": r["notes"], "evals": r["evals"], "seconds": r["seconds"], "code": r["code"],
            });
            let apply = arg_bool(args, "apply") == Some(true) && r["ok"] == true;
            if apply {
                if let Some(code) = r["code"].as_str() {
                    out["checkpoint"] = open(ctx, code)?;
                }
            }
            out["applied"] = json!(apply);
            Ok((out, apply))
        }
        "meta_search_start" => {
            let opts = SearchOptions {
                constraints: str_list(args, "constraints"),
                profiles: Vec::new(),
                workers: u64_arg(args, "workers")?,
                minutes: args.get("minutes").and_then(Value::as_f64),
                budget: None,
                max_jobs: None,
                seed: None,
            };
            Ok((lab.search_start(&req_str(args, "run")?, &opts).map_err(ToolError::Failed)?, false))
        }
        "meta_search_status" => {
            let r = match arg_str(args, "run") {
                Some(run) => lab.search_status(&run),
                None => lab.search_list(),
            };
            Ok((r.map_err(ToolError::Failed)?, false))
        }
        "meta_search_results" => {
            let top = u64_arg(args, "top")?.unwrap_or(10).clamp(1, 100);
            let r = lab
                .search_results(&req_str(args, "run")?, top, arg_bool(args, "codes") == Some(true), arg_bool(args, "include_unverified") != Some(false))
                .map_err(ToolError::Failed)?;
            Ok((r, false))
        }
        "meta_search_stop" => Ok((lab.search_stop(&req_str(args, "run")?).map_err(ToolError::Failed)?, false)),
        "graph_chains" => {
            let dir = lab.state_dir.join("jobs");
            std::fs::create_dir_all(&dir).map_err(|e| ToolError::Failed(e.to_string()))?;
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
            let file = dir.join(format!("graph-{}-{nanos}.json", std::process::id()));
            std::fs::write(&file, graph_requirements(args)?.to_string()).map_err(|e| ToolError::Failed(e.to_string()))?;
            let r = graph_args(args, &file.to_string_lossy()).and_then(|a| lab.run(&a, Some(std::time::Duration::from_secs(120))).map_err(ToolError::Failed));
            let _ = std::fs::remove_file(&file);
            Ok((r?, false))
        }
        "chain_build" => {
            let gems = str_list(args, "spirit_gems");
            let gems: Vec<String> = if gems.iter().any(|g| g.eq_ignore_ascii_case("none")) { Vec::new() } else { gems };
            let level = u64_arg(args, "level")?.map(|v| v as u32);
            let a = crate::lab_chain::chain_args(&req_str(args, "chain")?, level, &str_list(args, "exclude_supports"), &gems, u64_arg(args, "spirit_gear")?.unwrap_or(0) as u32);
            let mut out = lab.run(&a, Some(std::time::Duration::from_secs(1800))).map_err(ToolError::Failed)?;
            let apply = arg_bool(args, "apply") == Some(true);
            let code = out["code"].as_str().map(str::to_string);
            let opened = match code {
                Some(code) if apply => {
                    out["checkpoint"] = open(ctx, &code)?;
                    true
                }
                _ => false,
            };
            out["applied"] = json!(opened);
            Ok((out, opened))
        }
        other => Err(ToolError::Invalid(format!("unknown tool {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::{compact_optimise, defs, graph_args, graph_requirements, unverified, NAMES};
    use serde_json::json;

    #[test]
    fn graph_call_maps_to_requirements_and_flags() {
        let args = json!({
            "class": "Witch", "ascendancy": "Lich", "kinds": ["bridge"], "uniques": ["Indigon"], "level": 45, "spirit_gear": 30,
            "main_skill": "Ember Fusillade", "top": 500, "rank_by": "fitted",
        });
        let args = args.as_object().unwrap();
        assert_eq!(
            graph_requirements(args).unwrap(),
            json!({
                "cls": "Witch", "ascendancy": "Lich", "chain_kinds": ["bridge"], "uniques": ["Indigon"], "rank_by": "fitted",
                "spirit_source": "level", "spirit_level": 45, "spirit_gear": 30,
            })
        );
        let flags = graph_args(args, "req.json").unwrap();
        assert_eq!(flags[..3], ["graph", "--requirements", "@req.json"]);
        assert!(flags.windows(2).any(|w| w == ["--main", "Ember Fusillade"]));
        assert_eq!(flags[flags.len() - 2..], ["--top", "100"]);
        // nothing asked: no restrictions, the fixed Spirit budget
        let none = json!({});
        assert_eq!(graph_requirements(none.as_object().unwrap()).unwrap(), json!({}));
    }

    #[test]
    fn names_match_defs() {
        let names: Vec<&str> = defs().iter().map(|d| d.name).collect();
        assert_eq!(names, NAMES);
    }

    #[test]
    fn optimise_result_is_compacted() {
        let side = json!({
            "objective": 18.6, "feasible": true, "verified": false, "dps_honest": 2.4e6, "dps_raw": 4.3e6, "ehp": 15326.0,
            "flags": ["pinnacle_v1:unverified:energy_unmodelled:Spellslinger", "mapping_v1:unverified:energy_unmodelled:Spellslinger", "pinnacle_v1:condition_flag:gem.arc"],
            "profiles": { "pinnacle_v1": { "dps_honest": 2.4e6, "max_hit_min": 6904.0, "main_skill": "Orb of Storms", "terms": {} } },
        });
        let r = json!({
            "start": side, "end": side, "gain": 0.0, "changes": ["supports: +Zerphi's Infamy"], "code": "eN...",
            "steps": [{ "name": "supports", "improved": false, "notes": ["long"], "changes": [] }], "history": [1, 2, 3],
        });
        let c = compact_optimise(&r);
        assert_eq!(c["end"]["profiles"]["pinnacle_v1"]["max_hit"], 6904.0);
        assert!(c["end"]["profiles"]["pinnacle_v1"].get("terms").is_none());
        assert_eq!(c["steps"][0]["step"], "supports");
        assert!(c["steps"][0].get("notes").is_none() && c.get("history").is_none());
        assert_eq!(c["unverified"], json!(["unverified:energy_unmodelled:Spellslinger"]));
        assert_eq!(unverified(&json!(null)), json!([]));
    }
}
