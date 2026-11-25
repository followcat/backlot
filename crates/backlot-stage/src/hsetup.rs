//! `backlot hollywood setup` (R22, D14): writes the `[hollywood]`
//! table into the user's `backlot/config.toml` — the file the client
//! reads at start and the stage re-reads every tick — and keeps
//! everything else in it byte for byte. Every bad input is refused
//! before the file is touched: an unsupported or uninstalled runner
//! is the dry run's own message, a bad effort or `--key` is refused
//! the way the config's own rules refuse it.

use std::path::Path;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::config::{self, HollywoodCfg};

#[path = "hsetup_merge.rs"]
mod merge;
use merge::{hollywood_span, table, update_section};

/// A `--key` sanity check — the client does the real binding parse.
/// Here a direct binding needs a modifier (`alt+y`, not `y`) and a
/// `prefix+` chord needs a tail.
fn check_key(key: &str) -> Result<(), Fail> {
    let k = key.trim();
    if k.is_empty() {
        return Err(Fail::usage("--key is empty"));
    }
    match k.strip_prefix("prefix+") {
        Some(tail) if tail.trim().is_empty() => {
            Err(Fail::usage("--key 'prefix+' wants a key after it"))
        }
        Some(_) => Ok(()),
        None if k.contains('+') => Ok(()),
        None => Err(Fail::usage(format!(
            "--key '{k}' is a bare key — want a binding like alt+y or prefix+<key>"
        ))),
    }
}

/// Validate Hollywood's runner, model, effort and launch binary —
/// shared between `hollywood setup`, `propose setup` and `Setup` card
/// execution so bad configs refuse at birth.
pub(crate) fn validate_hollywood_target(
    env: &Env,
    runner: &str,
    model: &str,
    effort: Option<&str>,
) -> Result<std::path::PathBuf, Fail> {
    if runner.trim().is_empty() {
        return Err(Fail::usage("--runner is empty"));
    }
    if !crate::runners::supported(runner) {
        return Err(Fail::refused(format!(
            "runner '{runner}' is not supported (supported: {})",
            backlot_model::recast::SUPPORTED_RUNNERS.join(", ")
        )));
    }
    if model.trim().is_empty() {
        return Err(Fail::usage("--model is empty"));
    }
    let actor = backlot_model::ActorIdentity {
        runner: runner.to_string(),
        runner_version: None,
        model: model.to_string(),
        effort: effort.map(str::to_string),
        permission_profile: "standard".into(),
    };
    backlot_model::recast::check_model(&actor).map_err(|e| Fail::refused(e.to_string()))?;
    config::effort_rule(runner, effort).map_err(Fail::refused)?;
    let path = config::config_path(&env.vars);
    // A malformed file refuses before anything writes — the refusal
    // names the parse position only, never the file's text.
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let _: toml::Value = toml::from_str(&old).map_err(|e| match e.span() {
        Some(span) => Fail::refused(format!(
            "config.toml: malformed at {}",
            config::line_col(&old, span.start)
        )),
        None => Fail::refused("config.toml: malformed".to_string()),
    })?;
    // The runner must launch — under the launch PATH, like the
    // dry-run contract. One detection's login-shell or known-dir
    // hit earns the `[runner_env]` write first, then the shared
    // resolver answers under the PATH the launch actually gets.
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let scope = crate::runners::detect::scope(env);
    let named = std::collections::BTreeSet::from([runner.to_string()]);
    crate::wizard_paths::ensure_runner_paths(
        base,
        &named,
        env,
        &mut |r| crate::runners::detect::find(r, &scope),
        &mut std::io::stderr(),
    )?;
    let config_text = std::fs::read_to_string(&path).unwrap_or_default();
    crate::runners::resolve::launch_binary(runner, &config_text, &scope).map_err(Fail::refused)?;
    Ok(path)
}

/// Validate and write `[hollywood]` atomically while preserving
/// `start` and every other key/comment/section in `config.toml`.
pub(crate) fn write_config(env: &Env, cfg: &HollywoodCfg) -> Result<std::path::PathBuf, Fail> {
    if let Some(k) = cfg.key.as_deref() {
        check_key(k)?;
    }
    let path = validate_hollywood_target(env, &cfg.runner, &cfg.model, cfg.effort.as_deref())?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let new = match hollywood_span(&old) {
        Some((a, b)) => {
            let section = update_section(&old[a..b], cfg);
            format!("{}{section}{}", &old[..a], &old[b..])
        }
        None => {
            let section = table(cfg);
            let mut new = old.clone();
            if !new.is_empty() && !new.ends_with('\n') {
                new.push('\n');
            }
            if !new.is_empty() {
                new.push('\n');
            }
            new.push_str(&section);
            new
        }
    };
    if new != old {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| Fail::refused(format!("{}: {e}", dir.display())))?;
        }
        // Through a sibling temp file: a torn write leaves the old
        // config, never a half table.
        write_atomic(&path, &new)?;
    }
    Ok(path)
}

/// `hollywood setup --runner <r> --model <m> [--effort <e>] [--key
/// <k>]` — the user's, never a role terminal's: the table steers the
/// overseer itself (D17a's gate's shape).
pub(crate) fn hsetup(_common: &Common, env: &Env, cfg: &HollywoodCfg) -> Result<String, Fail> {
    let (_, path) = crate::verbs_hwood::hollywood_setup(env, cfg)?;
    Ok(format!(
        "[hollywood] written to {} — a running stage sees it on its next tick; restart the TUI for the layer",
        path.display()
    ))
}

/// Write `text` over `path` through a sibling temp file + rename,
/// keeping the old file's mode when it has one.
fn write_atomic(path: &Path, text: &str) -> Result<(), Fail> {
    let tmp = path.with_extension("toml.setup-tmp");
    std::fs::write(&tmp, text).map_err(|e| Fail::refused(format!("{}: {e}", tmp.display())))?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Fail::refused(format!("{}: {e}", path.display()))
    })
}
