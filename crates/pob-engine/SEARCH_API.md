# Search methods

`lua/search.lua` adds `search_*` methods to the bridge for programs that score many small changes to one
build. It is a port of exilegen's `exg.lua`; the stats record, change format and profiles are the same, so
a client switches by renaming ops. Both games are served: on PoE1 the PoE2-only parts (meta-gem `energy`,
`spirit`) are absent.

Every stat comes from `calcs.initEnv(build, "CALCULATOR", override)` + `calcs.perform`, the pipeline behind
PoB's `calcFunc` and node power, so `search_stats`, `search_whatif` and `search_apply` agree exactly. PoB's
main output (sidebar, `get_stats`) is refreshed only by `search_export` or the bridge's `refresh`.

A build opened by any bridge method starts a session with the exported main skill (`"pob"`) and the
`as_exported` profile; `search_load` defaults to `main: "auto"`.

## exg op -> bridge method

| exg op | bridge method | notes |
|---|---|---|
| `load` | `search_load` | `{code \| xml, name?, profile?, main?, gc?}`; returns the summary (plus `game`) |
| `summary` | `search_summary` | |
| `profiles` | `search_profiles` | |
| `set_profile` | `search_set_profile` | `{name, profile?, main?}`; `profile` may be a profile table that defines `name` |
| `select_main` | `search_select_main` | |
| `stats` | `search_stats` | |
| `skills` | `search_skills` | |
| `valid_supports` | `search_valid_supports` | `attrCost` is 0 on PoE1 |
| `tree` | `search_tree` | adds `pathFree`, `grantedAllocated`, `leap` on frontier entries and nodes |
| `whatif` | `search_whatif` | |
| `whatif_many` | `search_whatif_many` | |
| `apply` | `search_apply` | |
| `snapshot` | `search_snapshot` | `{items?}`: items are kept too unless `items: false` |
| `restore` | `search_restore` | |
| `drop_snapshot` | `search_drop_snapshot` | |
| `export` / `export_xml` | `search_export` / `search_export_xml` | |
| `lua`, `gc`, `ping` | `pobctl eval`, `gc`, `ping` | existing bridge methods |

## Differences from exg

- **Gem removal:** JSON `null` reaches Lua as nil, so remove a slot with `"gem": false` (or `"remove": true`)
  instead of `"gem": null`.
- **Path-free nodes (exilegen known issue #1, fixed):** an allocated node that PoB lists as reachable without
  a path (`node.intuitiveLeapLikesAffecting`: Intuitive Leap-like and From Nothing jewel sockets,
  `AllocateFromNodeRadius` keystones) is kept while one of its sources is kept, on what-if and apply alike.
  Item-granted nodes (PoE2 "Allocates N Sinister Jewel sockets") are never pruned and are handed back to PoB's
  `SetGrantedPassiveNodes` after an apply or restore. Removing a source prunes exactly what PoB's own
  `DeallocNode` prunes. A source added in the same change is not yet seen as a source.
- **`search_tree`:** `pathFree {id: [source ids]}` for allocated nodes, `grantedAllocated [ids]`,
  `jewelSockets[].granted` / `.pathFree`, and `leap [source ids]` on frontier entries and node facts.
  `allocated` no longer includes item-granted sockets.
- **Item snapshots:** `search_snapshot` keeps PoB's own items undo state (every item, slot and jewel socket
  selection, item sets), so gear changed with any bridge method (`equip_item_raw`, `unequip_item`,
  `item_db_equip`, ...) is undone by `search_restore` without an export and reload. A restore can be
  repeated.
- **Profiles:** shipped in `lua/search_profiles.json` (same three profiles). Fresh profiles also carry
  `bandit` and `pantheon` inputs (PoE1 character state). Their config sets are titled `search_<name>`.
- **Main selection** moves PoB's marker through the bridge's calcs-selection helpers, so the Calcs tab follows.

## Change (whatif / whatif_many / apply)

```
{ "add_nodes":    [id | {"id": id, "ws": 1|2}],
  "remove_nodes": [id],
  "attr_choices": {"<id>": 1|2|3},
  "gem_changes":  [{"group": g, "slot": s, "gem"?: <gem id | skill id | name> | false, "level"?, "quality"?, "enabled"?, "label"?}],
  "group_enabled": {"<g>": true|false} }
```

Rules as exg's API.md: tree connectivity as `PassiveSpec:BuildAllDependsAndPaths` (`pruned` lists dropped ids),
a node without `ws` takes the first mode that connects it, a new group is `#groups+1`, in an item- or node-granted
group slot 1 (the granted skill) cannot change but later slots take supports unless the group is `noSupports`, removals run highest slot first.

## Stats record

`fields`: `"core"` = `mainSkill stats minionStats main dps offence sustain movementSpeed ailmentThreshold
spirit maxHit requirements keystones uniques`; `"all"` adds `pobMain unsupported skills energy`; `marginals`
only when asked (`fields` or `marginals: true`). `stats` uses corpus-eval's `STAT_KEYS` names plus the exg
extensions. Field contents are as in exilegen's `engine/API.md`.

## Measured (release, PoE2 fixture build, one core)

| op | ms (two runs; the box was shared) |
|---|---|
| `search_whatif` node path, `fields: ["dps"]` | 16–20 |
| `search_whatif` node, core | 18–28 |
| `search_whatif` gem, `fields: ["dps"]` | 15 |
| `search_whatif_many`, per variant | 16–23 |
| `search_stats` core / all | 19 / 110–150 |
| `search_tree` (full frontier) | 40–46 |
| `search_apply` | 100–120 |
| `search_restore` | 70–130; about 150 with items |

Tests: `tests/search_ops.rs` (`POB_ROOT` for PoE2, `POB1_ROOT` for PoE1; each skips when its root is missing).
