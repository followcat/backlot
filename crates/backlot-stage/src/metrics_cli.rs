//! Read-only CLI execution for `backlot metrics` and `backlot digest`.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use backlot_model::event::{Event, EventKind};
use backlot_model::hollywood::HollywoodEvent;
use backlot_model::metrics::{
    DigestInput, MetricsInput, StudioDigestInput, StudioEventsInput, compute_digest,
    compute_metrics,
};

use crate::args::{Common, DigestFormat};
use crate::cli::{Env, Fail};
use crate::metrics_render::{render_digest_md, render_metrics_table};
use crate::store::{self, StoreError};

const DEFAULT_DIGEST_SINCE_MS: u64 = 24 * 3_600_000;

/// Parse `<dur>` (`ms`, `s`, `m`, `h`, `d`, `w` — compound ok, e.g. `1d12h`) into ms.
pub(crate) fn parse_duration_ms(raw: &str) -> Result<u64, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("duration cannot be empty".into());
    }
    if s == "0" {
        return Ok(0);
    }
    let mut rest = s;
    let mut total_ms = 0u64;
    while !rest.is_empty() {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return Err(format!(
                "invalid duration '{raw}' (expected e.g. 24h, 7d, 30m)"
            ));
        }
        let n: u64 = digits
            .parse()
            .map_err(|_| format!("duration overflow in '{raw}'"))?;
        rest = rest.get(digits.len()..).unwrap_or("");
        let unit: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if unit.is_empty() {
            return Err(format!("missing unit in duration '{raw}' (ms|s|m|h|d|w)"));
        }
        rest = rest.get(unit.len()..).unwrap_or("");
        let mult: u64 = match unit.as_str() {
            "ms" => 1,
            "s" => 1_000,
            "m" => 60_000,
            "h" => 3_600_000,
            "d" => 86_400_000,
            "w" => 7 * 86_400_000,
            _ => return Err(format!("unknown duration unit '{unit}' in '{raw}'")),
        };
        let add = n
            .checked_mul(mult)
            .ok_or_else(|| format!("duration overflow in '{raw}'"))?;
        total_ms = total_ms
            .checked_add(add)
            .ok_or_else(|| format!("duration overflow in '{raw}'"))?;
    }
    Ok(total_ms)
}

fn store_err(ctx: &str, e: StoreError<()>) -> Fail {
    let detail = match e {
        StoreError::Io(err) => err.to_string(),
        StoreError::Corrupt(c) => c.to_string(),
        _ => "unexpected ledger store error".into(),
    };
    Fail::refused(format!("{ctx}: {detail}"))
}

fn resolve_target_studios(
    common: &Common,
    env: &Env,
    base: &Path,
    all: bool,
) -> Result<(Vec<String>, PathBuf, bool), Fail> {
    match (all, &common.studio) {
        (false, Some(name)) => {
            let checked = crate::cli::check_studio_name(name)?;
            let resolved = crate::cli::resolve_base(env, common, base, &checked)?;
            Ok((vec![checked], resolved, false))
        }
        _ => {
            let mut names = store::studios(base).map_err(|e| Fail::refused(e.to_string()))?;
            names.sort();
            Ok((names, base.to_path_buf(), true))
        }
    }
}

fn load_studio_events(base: &Path, name: &str) -> Result<Vec<Event>, Fail> {
    let dir = store::studio_dir(base, name);
    let (events, _) =
        store::load_events(&dir).map_err(|e| store_err(&format!("studio '{name}'"), e))?;
    Ok(events)
}

fn load_hollywood_events(base: &Path) -> Result<Vec<HollywoodEvent>, Fail> {
    let hdir = store::hollywood_dir(base);
    match store::load_hevents(&hdir) {
        Ok((events, _)) => Ok(events),
        Err(StoreError::Io(err)) if err.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(other) => Err(store_err("hollywood", other)),
    }
}

fn load_acceptance_texts(sdir: &Path, events: &[Event]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(canon_root) = fs::canonicalize(sdir) else {
        return out;
    };
    for e in events {
        let EventKind::TaskCreated { acceptance, .. } = &e.kind else {
            continue;
        };
        if acceptance.is_empty() || out.contains_key(acceptance) {
            continue;
        }
        let rel = Path::new(acceptance);
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            continue;
        }
        let candidate = sdir.join(rel);
        let Ok(canon_file) = fs::canonicalize(&candidate) else {
            continue;
        };
        if !canon_file.starts_with(&canon_root) {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&canon_file) {
            out.insert(acceptance.clone(), text);
        }
    }
    out
}

/// Execute `backlot metrics [--studio <id> | --all] [--since <dur>] [--json]`.
pub(crate) fn metrics(
    common: &Common,
    env: &Env,
    base: &Path,
    json: bool,
    all: bool,
    since: Option<&str>,
) -> Result<String, Fail> {
    let (names, resolved, scope_all) = resolve_target_studios(common, env, base, all)?;
    let now_ms = crate::live::now_ms();
    let since_ms = match since {
        Some(raw) => {
            let dur = parse_duration_ms(raw)
                .map_err(|m| Fail::usage(format!("{m}\n{}", crate::args::U_METRICS)))?;
            Some(now_ms.saturating_sub(dur))
        }
        None => None,
    };

    let mut studios = Vec::with_capacity(names.len());
    for name in &names {
        studios.push(StudioEventsInput {
            studio: name.clone(),
            events: load_studio_events(&resolved, name)?,
        });
    }
    let hollywood_events = load_hollywood_events(&resolved)?;

    let report = compute_metrics(&MetricsInput {
        studios,
        hollywood_events,
        hollywood_include_unscoped: scope_all,
        since_ms,
        now_ms,
    });

    if json {
        serde_json::to_string_pretty(&report).map_err(|e| Fail::refused(e.to_string()))
    } else {
        Ok(render_metrics_table(&report, since))
    }
}

/// Execute `backlot digest [--studio <id> | --all] [--since <dur>] [--format md|json]`.
pub(crate) fn digest(
    common: &Common,
    env: &Env,
    base: &Path,
    all: bool,
    since: Option<&str>,
    format: DigestFormat,
) -> Result<String, Fail> {
    let (names, resolved, _) = resolve_target_studios(common, env, base, all)?;
    let now_ms = crate::live::now_ms();
    let dur_ms = match since {
        Some(raw) => parse_duration_ms(raw)
            .map_err(|m| Fail::usage(format!("{m}\n{}", crate::args::U_DIGEST)))?,
        None => DEFAULT_DIGEST_SINCE_MS,
    };
    let since_ms = now_ms.saturating_sub(dur_ms);

    let mut studios = Vec::with_capacity(names.len());
    for name in &names {
        let events = load_studio_events(&resolved, name)?;
        let sdir = store::studio_dir(&resolved, name);
        let acceptance_texts = load_acceptance_texts(&sdir, &events);
        studios.push(StudioDigestInput {
            studio: name.clone(),
            events,
            acceptance_texts,
        });
    }
    let hollywood_events = load_hollywood_events(&resolved)?;

    let report = compute_digest(&DigestInput {
        studios,
        hollywood_events,
        since_ms,
        now_ms,
    });

    match format {
        DigestFormat::Json => {
            serde_json::to_string_pretty(&report).map_err(|e| Fail::refused(e.to_string()))
        }
        DigestFormat::Md => Ok(render_digest_md(&report, since.unwrap_or("24h"))),
    }
}
