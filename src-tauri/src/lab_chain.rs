//! The Graph view's "Create build": builds one chain with exilegen (`python -m exilegen.lab chain`)
//! through the lab client. PoE2 only, as the rest of the lab.

use std::time::Duration;

use pob_engine::lab::LabConfig;
use serde::Serialize;
use serde_json::Value;
use tauri::{Manager, State};

use crate::game::Game;
use crate::AppState;

/// Why a chain build did not produce a result. `setup` means Python, exilegen or pobctl was not
/// found, so the view can say where to configure them.
#[derive(Serialize)]
pub(crate) struct ChainBuildError {
    kind: &'static str,
    message: String,
}

fn failed(message: String) -> ChainBuildError {
    // the lab client reports a missing interpreter as "start <python>: ..."
    let setup = message.starts_with("start ") || message.contains("No module named");
    ChainBuildError { kind: if setup { "setup" } else { "failed" }, message }
}

fn setup(message: String) -> ChainBuildError {
    ChainBuildError { kind: "setup", message }
}

/// Arguments of `exilegen.lab chain` for one request. No `level` builds at the generator's end level.
/// Spirit gems are added exactly as named, with `spirit_gear` Spirit assumed from equipment.
fn chain_args(chain: &str, level: Option<u32>, exclude_supports: &[String], spirit_gems: &[String], spirit_gear: u32) -> Vec<String> {
    let mut args = vec!["chain".to_string(), "--chain".to_string(), chain.to_string()];
    if let Some(level) = level {
        args.push("--level".into());
        args.push(level.clamp(1, 100).to_string());
    }
    for name in exclude_supports {
        args.push("--exclude-support".into());
        args.push(name.clone());
    }
    if !spirit_gems.is_empty() {
        args.push("--spirit-gems".into());
        args.push(spirit_gems.join(","));
        if spirit_gear > 0 {
            args.push("--spirit-gear".into());
            args.push(spirit_gear.to_string());
        }
    }
    // with exclusions, also build the unrestricted chain so the result says what replaced each one;
    // with Spirit gems, also build the chain without them so the result says what they add
    if !exclude_supports.is_empty() || !spirit_gems.is_empty() {
        args.push("--compare".into());
    }
    args
}

/// Build one chain at a character level without the excluded support gems, with the named Spirit
/// gems when any are given. Takes about a minute.
#[tauri::command]
pub(crate) async fn lab_chain_build(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    chain: String,
    level: Option<u32>,
    exclude_supports: Option<Vec<String>>,
    spirit_gems: Option<Vec<String>>,
    spirit_gear: Option<u32>,
) -> Result<Value, ChainBuildError> {
    if state.game() != Game::Poe2 {
        return Err(failed("chain builds are PoE2 only".into()));
    }
    let dir = app.path().app_data_dir().map_err(|e| failed(e.to_string()))?.join("lab");
    let lab = LabConfig::discover(&state.pob_root(), &dir, None).map_err(setup)?;
    let args = chain_args(&chain, level, &exclude_supports.unwrap_or_default(), &spirit_gems.unwrap_or_default(), spirit_gear.unwrap_or(0));
    tauri::async_runtime::spawn_blocking(move || lab.run(&args, Some(Duration::from_secs(1800))))
        .await
        .map_err(|e| failed(e.to_string()))?
        .map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::{chain_args, failed};

    #[test]
    fn args_follow_the_lab_command() {
        assert_eq!(chain_args("ed0b", None, &[], &[], 0), ["chain", "--chain", "ed0b"]);
        assert_eq!(
            chain_args("ed0b", Some(45), &["Rakiata's Flow".into(), "Extraction".into()], &[], 0),
            ["chain", "--chain", "ed0b", "--level", "45", "--exclude-support", "Rakiata's Flow", "--exclude-support", "Extraction", "--compare"]
        );
    }

    #[test]
    fn spirit_gems_are_named_and_compared() {
        assert_eq!(
            chain_args("ed0b", Some(33), &[], &["Herald of Ice".into(), "Herald of Ash".into()], 100),
            ["chain", "--chain", "ed0b", "--level", "33", "--spirit-gems", "Herald of Ice,Herald of Ash", "--spirit-gear", "100", "--compare"]
        );
        assert_eq!(
            chain_args("ed0b", None, &[], &["Herald of Ice".into()], 0),
            ["chain", "--chain", "ed0b", "--spirit-gems", "Herald of Ice", "--compare"]
        );
        // Spirit from equipment only matters to a build that holds Spirit gems
        assert_eq!(chain_args("ed0b", None, &[], &[], 100), ["chain", "--chain", "ed0b"]);
    }

    #[test]
    fn a_missing_interpreter_is_a_setup_error() {
        assert_eq!(failed("start python: program not found".into()).kind, "setup");
        assert_eq!(failed("chain not found: ed0b".into()).kind, "failed");
    }
}
