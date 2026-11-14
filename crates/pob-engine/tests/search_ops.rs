use std::path::PathBuf;
use std::time::Instant;

use pob_engine::{Engine, EngineConfig};
use serde_json::{Value, json};


fn root(var: &str, dir: &str) -> Option<PathBuf> {
    if let Ok(root) = std::env::var(var) {
        return Some(PathBuf::from(root));
    }
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources").join(dir);
    local.join("Launch.lua").is_file().then_some(local)
}

fn boot(var: &str, dir: &str, name: &str) -> Option<Engine> {
    let Some(pob_root) = root(var, dir) else {
        eprintln!("skipping: no PoB data (run pob-sync or set {var})");
        return None;
    };
    let user_dir = std::env::temp_dir().join(format!("pob-engine-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&user_dir);
    Some(Engine::boot(EngineConfig { pob_root, user_dir, cache_dir: None, data_dir: None }).expect("boot"))
}

/// PoE2 fixture build code from POB_SEARCH_FIXTURE: a build with a From Nothing jewel, Voices
/// (granted Sinister sockets) and meta gems. Not committed; it is a ladder player's build.
fn poe2(name: &str) -> Option<Engine> {
    let Some(code) = std::env::var("POB_SEARCH_FIXTURE").ok().and_then(|p| std::fs::read_to_string(p).ok()) else {
        eprintln!("skipping: set POB_SEARCH_FIXTURE to a PoE2 build code file");
        return None;
    };
    let engine = boot("POB_ROOT", "pob", name)?;
    engine.call("search_load", &json!({ "code": code.trim(), "main": "pob" })).expect("search_load");
    Some(engine)
}

fn call(engine: &Engine, method: &str, params: Value) -> Value {
    engine.call(method, &params).unwrap_or_else(|e| panic!("{method}: {e}"))
}

fn without_pruned(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.remove("pruned");
    }
    v
}

fn tree_ids(tree: &Value, key: &str) -> Vec<u64> {
    let mut ids: Vec<u64> = tree[key].as_array().expect(key).iter().map(|v| v.as_u64().unwrap()).collect();
    ids.sort_unstable();
    ids
}

fn path_free(tree: &Value) -> Vec<u64> {
    let mut ids: Vec<u64> = tree["pathFree"].as_object().expect("pathFree").keys().map(|k| k.parse().unwrap()).collect();
    ids.sort_unstable();
    ids
}

/// The first unallocated main-tree node one point away that needs no weapon set and has no path-free source.
fn plain_frontier_node(tree: &Value) -> u64 {
    tree["frontier"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| {
            let id = f["id"].as_u64().unwrap();
            let kind = &tree["nodes"][id.to_string()]["kind"];
            f["dist"] == 1 && f["ws"].is_null() && f["leap"].is_null() && (kind == "small" || kind == "notable")
        })
        .expect("a frontier node")["id"]
        .as_u64()
        .unwrap()
}

#[test]
fn whatif_equals_apply_for_nodes_gems_and_attributes() {
    let Some(engine) = poe2("whatif") else { return };
    let tree = call(&engine, "search_tree", json!({ "max_dist": 1 }));
    let node = plain_frontier_node(&tree);
    let token = call(&engine, "search_snapshot", json!({}))["token"].clone();

    let change = json!({ "add_nodes": [node] });
    let t0 = Instant::now();
    let whatif = call(&engine, "search_whatif", change.clone());
    eprintln!("whatif node (core): {} ms", t0.elapsed().as_millis());
    let applied = call(&engine, "search_apply", change);
    assert_eq!(without_pruned(whatif), without_pruned(applied));
    call(&engine, "search_restore", json!({ "token": token }));

    let skills = call(&engine, "search_skills", json!({}));
    let group = skills["main"]["group"].as_u64().unwrap();
    let gems = skills["groups"][group as usize - 1]["gems"].as_array().unwrap();
    let slot = gems.iter().find(|g| g["support"] == true).expect("a support")["slot"].clone();
    let supports = call(&engine, "search_valid_supports", json!({ "group": group }));
    let candidate = supports["supports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["socketed"] == false && s["conflicts"].as_array().unwrap().is_empty())
        .expect("a legal support")["id"]
        .clone();
    let change = json!({ "gem_changes": [{ "group": group, "slot": slot, "gem": candidate }] });
    let t0 = Instant::now();
    let whatif = call(&engine, "search_whatif", json!({ "gem_changes": change["gem_changes"], "fields": ["dps"] }));
    eprintln!("whatif gem (dps): {} ms", t0.elapsed().as_millis());
    assert!(whatif["dps"]["best"].as_f64().unwrap() > 0.0);
    let whatif = call(&engine, "search_whatif", change.clone());
    let applied = call(&engine, "search_apply", change);
    assert_eq!(whatif, applied);
    call(&engine, "search_restore", json!({ "token": token }));

    let (id, choice) = tree["attributeChoices"].as_object().unwrap().iter().next().expect("an attribute node");
    let other = if choice.as_u64() == Some(1) { 2 } else { 1 };
    let change = json!({ "attr_choices": { id.clone(): other } });
    let whatif = call(&engine, "search_whatif", change.clone());
    let applied = call(&engine, "search_apply", change);
    assert_eq!(whatif, applied);
    let after = call(&engine, "search_tree", json!({ "frontier": false }));
    assert_eq!(after["attributeChoices"][id], json!(other));
}

#[test]
fn snapshot_restores_tree_gems_and_items() {
    let Some(engine) = poe2("snapshot") else { return };
    let base = call(&engine, "search_stats", json!({ "fields": ["core"] }));
    let base_tree = call(&engine, "search_tree", json!({ "max_dist": 1 }));
    let slots = call(&engine, "list_slots", json!({}));
    let token = call(&engine, "search_snapshot", json!({}))["token"].clone();

    let node = plain_frontier_node(&base_tree);
    let main = base["main"]["group"].clone();
    call(&engine, "search_apply", json!({ "add_nodes": [node], "gem_changes": [{ "group": main, "slot": 2, "gem": false }], "fields": false }));
    call(&engine, "unequip_item", json!({ "slot": "Body Armour" }));
    call(&engine, "unequip_item", json!({ "slot": "Amulet" }));
    assert_ne!(call(&engine, "search_stats", json!({ "fields": ["core"] }))["stats"], base["stats"]);

    call(&engine, "search_restore", json!({ "token": token }));
    assert_eq!(call(&engine, "search_stats", json!({ "fields": ["core"] })), base);
    assert_eq!(call(&engine, "list_slots", json!({})), slots);
    let tree = call(&engine, "search_tree", json!({ "frontier": false }));
    assert_eq!(tree_ids(&tree, "allocated"), tree_ids(&base_tree, "allocated"));
    assert_eq!(tree_ids(&tree, "grantedAllocated"), tree_ids(&base_tree, "grantedAllocated"));

    // a restore can be repeated: the snapshot keeps its own copy of the items
    call(&engine, "unequip_item", json!({ "slot": "Body Armour" }));
    call(&engine, "search_restore", json!({ "token": token }));
    assert_eq!(call(&engine, "search_stats", json!({ "fields": ["core"] })), base);
}

#[test]
fn profile_switches_are_reproducible() {
    let Some(engine) = poe2("profile") else { return };
    let fields = json!({ "fields": ["dps", "stats", "maxHit"] });
    let exported = call(&engine, "search_stats", fields.clone());
    let set = call(&engine, "search_set_profile", json!({ "name": "pinnacle_v1" }));
    assert_eq!(set["scoring"]["scenario"], "boss");
    let pinnacle = call(&engine, "search_stats", fields.clone());
    call(&engine, "search_set_profile", json!({ "name": "mapping_v1" }));
    let mapping = call(&engine, "search_stats", fields.clone());
    assert_ne!(pinnacle["dps"], mapping["dps"]);
    call(&engine, "search_set_profile", json!({ "name": "pinnacle_v1" }));
    assert_eq!(call(&engine, "search_stats", fields.clone()), pinnacle);
    call(&engine, "search_set_profile", json!({ "name": "as_exported" }));
    assert_eq!(call(&engine, "search_stats", fields.clone()), exported);

    let custom = json!({ "base": "fresh", "carry_prefixes": ["quest"], "config": { "enemyIsBoss": "Pinnacle" }, "scoring": { "scenario": "mine" } });
    let set = call(&engine, "search_set_profile", json!({ "name": "mine", "profile": custom }));
    assert_eq!(set["scoring"]["scenario"], "mine");
    let mine = call(&engine, "search_stats", fields);
    assert_eq!(mine["dps"], pinnacle["dps"]);
    assert_eq!(mine["profile"], "mine");
}

#[test]
fn path_free_and_granted_nodes_survive_tree_changes() {
    let Some(engine) = poe2("pathfree") else { return };
    let tree = call(&engine, "search_tree", json!({ "max_dist": 1 }));
    let free = path_free(&tree);
    let granted = tree_ids(&tree, "grantedAllocated");
    assert!(!free.is_empty(), "the fixture allocates nodes through From Nothing");
    assert!(!granted.is_empty(), "the fixture has Sinister sockets from Voices");

    let node = plain_frontier_node(&tree);
    let whatif = call(&engine, "search_whatif", json!({ "add_nodes": [node], "fields": ["dps"] }));
    assert!(whatif.get("pruned").is_none(), "pruned {:?}", whatif["pruned"]);

    let applied = call(&engine, "search_apply", json!({ "add_nodes": [node], "fields": ["dps"] }));
    assert!(applied.get("pruned").is_none());
    let after = call(&engine, "search_tree", json!({ "frontier": false }));
    assert_eq!(path_free(&after), free);
    assert_eq!(tree_ids(&after, "grantedAllocated"), granted);
    assert!(tree_ids(&after, "allocated").contains(&node));

    // without its source, a node prunes exactly as PoB's own DeallocNode prunes it
    let source = after["pathFree"].as_object().unwrap().values().next().unwrap()[0].as_u64().unwrap();
    let whatif = call(&engine, "search_whatif", json!({ "remove_nodes": [source], "fields": ["dps"] }));
    let mut pruned: Vec<u64> = whatif["pruned"].as_array().expect("pruned").iter().map(|v| v.as_u64().unwrap()).collect();
    pruned.push(source);
    pruned.sort_unstable();
    call(&engine, "dealloc_node", json!({ "id": source }));
    let dealloc = call(&engine, "search_tree", json!({ "frontier": false }));
    let kept = tree_ids(&dealloc, "allocated");
    let gone: Vec<u64> = tree_ids(&after, "allocated").into_iter().filter(|id| !kept.contains(id)).collect();
    assert_eq!(pruned, gone);
}

#[test]
fn stats_carry_the_corpus_schema_and_marginals() {
    let Some(engine) = poe2("schema") else { return };
    let stats = call(&engine, "search_stats", json!({ "marginals": true }));
    for key in [
        "profile", "mainSkill", "stats", "minionStats", "main", "pobMain", "dps", "offence", "sustain", "movementSpeed",
        "ailmentThreshold", "spirit", "maxHit", "requirements", "keystones", "uniques", "unsupported", "skills", "energy",
        "marginals", "scoring",
    ] {
        assert!(stats.get(key).is_some(), "missing {key}");
    }
    for key in ["CombinedDPS", "TotalDPS", "Life", "TotalEHP", "FireResist", "Str", "MovementSpeedMod", "HitDamageByType"] {
        assert!(stats["stats"].get(key).is_some(), "missing stats.{key}");
    }
    let marginals = stats["marginals"].as_array().unwrap();
    assert!(!marginals.is_empty());
    for m in marginals {
        for key in ["group", "slot", "gem_id", "gemId", "name", "dps_without", "dps_base"] {
            assert!(m.get(key).is_some(), "marginal missing {key}");
        }
    }
    assert!(marginals.iter().any(|m| m["dps_without"] != m["dps_base"]));
    let energy = stats["energy"].as_array().unwrap();
    assert!(!energy.is_empty(), "the fixture has meta gems");
    assert!(energy[0].get("maxEnergy").is_some() && energy[0].get("feeder").is_some());

    let many = call(&engine, "search_whatif_many", json!({ "variants": [{}, { "add_nodes": [999_999_999] }], "fields": ["dps"] }));
    assert_eq!(many[0]["dps"], stats["dps"]);
    assert!(many[1]["err"].is_string());
    assert!(call(&engine, "search_export", json!({}))["code"].as_str().unwrap().len() > 1000);
}

#[test]
fn poe1_stats_and_node_whatif() {
    let Some(engine) = boot("POB1_ROOT", "pob1", "poe1") else { return };
    call(&engine, "new_build", json!({ "name": "search" }));
    let group = call(&engine, "search_skills", json!({}))["groups"].as_array().unwrap().len() + 1;
    let gems = json!([{ "group": group, "slot": 1, "gem": "Fireball" }, { "group": group, "slot": 2, "gem": "Added Fire Damage" }]);
    call(&engine, "search_apply", json!({ "gem_changes": gems, "fields": false }));
    call(&engine, "search_select_main", json!({ "main": group }));
    let tree = call(&engine, "search_tree", json!({ "max_dist": 2 }));
    let path = tree["frontier"][0]["path"].clone();
    call(&engine, "search_apply", json!({ "add_nodes": path, "fields": false }));

    let stats = call(&engine, "search_stats", json!({}));
    assert_eq!(stats["mainSkill"]["id"], "Fireball");
    assert!(stats["dps"]["best"].as_f64().unwrap() > 0.0);
    assert!(stats.get("energy").is_none() && stats.get("spirit").is_none(), "PoE1 has no meta gems or Spirit");

    let tree = call(&engine, "search_tree", json!({ "max_dist": 1 }));
    let node = plain_frontier_node(&tree);
    let whatif = call(&engine, "search_whatif", json!({ "add_nodes": [node] }));
    let applied = call(&engine, "search_apply", json!({ "add_nodes": [node] }));
    assert_eq!(whatif, applied);
    let supports = call(&engine, "search_valid_supports", json!({ "group": group }));
    assert!(!supports["supports"].as_array().unwrap().is_empty());
}
