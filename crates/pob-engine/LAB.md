# Build lab: optimiser and meta search

`pob_engine::lab` runs [exilegen](https://github.com/aizatrosli/exilegen) as a sidecar. exilegen holds the
search: tree, support and trigger optimisers, a from-scratch builder, and a MAP-Elites archive of build
styles. It also holds the honest scoring layer:

- locked Pinnacle-boss and map-pack configs, with the score taken as the worse of the two;
- a condition registry that discounts gems whose value assumes a state, such as Hexblast without a curse;
- meta-gem Energy, which caps trigger rates;
- floors taken from the ladder.

exilegen drives `pobctl serve` and its `search_*` methods (SEARCH_API.md), so every number is still PoB's.
PoE2 only.

## Setup

1. Build pobctl: `cargo build --release -p pob-engine --bin pobctl`.
2. Clone exilegen and point `POB_LAB_EXILEGEN` at the checkout. Its only runtime dependencies are Python 3.10+ and PyYAML.
3. Optionally set `POB_LAB_PYTHON` (default `python3`, or `python` on Windows) and `POB_LAB_POBCTL`. Without `POB_LAB_POBCTL`, the lab uses the `pobctl` next to the running executable. In a dev build that is `target/<profile>/`, beside the app.

Lab state goes in `$POB_LAB_DIR` for pobctl, else `<user dir>/lab`. For the app it goes in `<app data>/lab`. It holds the
engine's user dir, job scratch files and `runs/<name>/` for meta searches.

## Command line

```sh
pobctl --pob-root <dir> optimise build.code --constraint one_button          # JSON: start, end, gain, changes, code
pobctl --pob-root <dir> synth '{"cls":"Witch","ascendancy":"Lich","main_skill":"Spark","uniques":["Liminal Coil"]}'
pobctl --pob-root <dir> meta-search start overnight --minutes 480 --workers 3 --constraint one_button
pobctl --pob-root <dir> meta-search status overnight
pobctl --pob-root <dir> meta-search results overnight --top 20 --codes
pobctl --pob-root <dir> meta-search stop overnight        # finishes the builds in flight, writes report.md
pobctl --pob-root <dir> meta-search list
```

A search runs detached, so it outlives the command that started it and any cloud session limit.
`meta-search start` with the same name resumes it. `runs/<name>/report.md` and `builds.csv` are the human
report; `lab.log` is its log.

## MCP tools (PoE2)

| tool | does |
|---|---|
| `optimise_build` | Reworks the open build's tree, supports and triggers. `apply` checkpoints first (`before optimise_build`), so `rollback` undoes it |
| `build_from_idea` | Builds class + ascendancy + main skill (+ uniques, trigger, keystones, weapon, defence) from nothing |
| `meta_search_start` / `_status` / `_results` / `_stop` | The meta search above. Results carry PoB codes for `load_build` |

Jobs take minutes, so the tools are marked slow. A job is killed after 30 minutes.

## Protocol

The client runs `python -m exilegen.lab <command>` with `PYTHONPATH`, `EXG_BACKEND=pobctl`, `EXG_POBCTL` and
`EXG_POB_ROOT` set. Each command prints one JSON object on stdout; an `{"error"}` object or a non-zero exit is a
failure, and stderr is kept in `jobs/<stamp>.log`. Meta searches are `python -m exilegen build --run-dir`, stopped
by a `STOP` file and read through `progress.json`, `archive.jsonl` and `lab.json` (pid, args). The client checks
`exilegen.lab version` against `LAB_VERSION`.
