//! Usage & cost fields (P4a): the runner-reported `Usage` a
//! `task.delivered` carries, the `UsageTotals` the state, records
//! and summary aggregate into, and their compact display spellings.
//! All fields optional; all sums strictly factual — never a score
//! (D8).

use serde::{Deserialize, Serialize};

/// The runner-reported token/cost figures one `task.delivered`
/// carries (P4a). Every field is optional and the whole struct is:
/// a delivery without usage writes no `usage` key at all, and old
/// ledgers load with none. Costs are integer micro-dollars
/// (USD × 1_000_000) so `Eq`/`Ord` and JSON stay exact.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_micros_usd: Option<u64>,
}

impl Usage {
    /// Nothing reported — the event keeps the field absent.
    pub fn is_empty(&self) -> bool {
        self == &Usage::default()
    }
}

/// Delivery usage summed factually — on the studio state, an actor
/// record and the summary document. Counts only; never a score (D8).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTotals {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cost_micros_usd: u64,
}

impl UsageTotals {
    /// True while every counter is zero — callers show nothing.
    pub fn is_empty(&self) -> bool {
        *self == UsageTotals::default()
    }

    /// Fold one delivery's reported figures in; absent fields add 0.
    pub fn add(&mut self, usage: &Usage) {
        self.input_tokens = self
            .input_tokens
            .saturating_add(usage.input_tokens.unwrap_or(0));
        self.output_tokens = self
            .output_tokens
            .saturating_add(usage.output_tokens.unwrap_or(0));
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(usage.cache_read_tokens.unwrap_or(0));
        self.cost_micros_usd = self
            .cost_micros_usd
            .saturating_add(usage.cost_micros_usd.unwrap_or(0));
    }

    /// `950`, `12.4k`, `999k`, `1.2M` — one decimal under `k`,
    /// whole thousands at six figures and up.
    pub fn tokens(n: u64) -> String {
        if n >= 1_000_000 {
            format!("{:.1}M", n as f64 / 1_000_000.0)
        } else if n >= 100_000 {
            format!("{}k", n / 1_000)
        } else if n >= 1_000 {
            format!("{:.1}k", n as f64 / 1_000.0)
        } else {
            n.to_string()
        }
    }

    /// `$5`, `$0.08`, `$1.2345` — micro-dollars, fraction trimmed.
    pub fn cost(micros: u64) -> String {
        let (whole, frac) = (micros / 1_000_000, micros % 1_000_000);
        if frac == 0 {
            return format!("${whole}");
        }
        let mut frac = format!("{frac:06}");
        while frac.ends_with('0') {
            frac.pop();
        }
        format!("${whole}.{frac}")
    }

    /// `12.4k in / 3.1k out / 2.0k cache · $0.08` — segments appear
    /// only for counters that moved; `is_empty` gives an empty line.
    pub fn compact(&self) -> String {
        let mut parts = Vec::new();
        if self.input_tokens > 0 {
            parts.push(format!("{} in", Self::tokens(self.input_tokens)));
        }
        if self.output_tokens > 0 {
            parts.push(format!("{} out", Self::tokens(self.output_tokens)));
        }
        if self.cache_read_tokens > 0 {
            parts.push(format!("{} cache", Self::tokens(self.cache_read_tokens)));
        }
        let mut s = parts.join(" / ");
        if self.cost_micros_usd > 0 {
            if !s.is_empty() {
                s.push_str(" · ");
            }
            s.push_str(&Self::cost(self.cost_micros_usd));
        }
        s
    }

    /// The dock's tighter spelling: `12.4k/3.1k/2.0kc tok · $0.08`.
    pub fn compact_dock(&self) -> String {
        let mut toks = Vec::new();
        if self.input_tokens > 0 {
            toks.push(Self::tokens(self.input_tokens));
        }
        if self.output_tokens > 0 {
            toks.push(Self::tokens(self.output_tokens));
        }
        if self.cache_read_tokens > 0 {
            toks.push(format!("{}c", Self::tokens(self.cache_read_tokens)));
        }
        let mut s = match toks.len() {
            0 => String::new(),
            _ => format!("{} tok", toks.join("/")),
        };
        if self.cost_micros_usd > 0 {
            if !s.is_empty() {
                s.push_str(" · ");
            }
            s.push_str(&Self::cost(self.cost_micros_usd));
        }
        s
    }
}
