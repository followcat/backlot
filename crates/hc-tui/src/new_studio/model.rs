//! The New Studio dialog's data model — machine rows, the `repos
//! --json` and `runners --json` documents, the studio-id rules and
//! the generated `cast.toml`. Pure parsing/validation lives here so
//! the step draws and the App wiring stay thin.

use crate::chrome::text_editor::TextEditor;

/// An async answer's state — pending, ok, or the message the
/// machine sent back.
#[derive(Debug, Clone)]
pub(crate) enum Probe<T> {
    Pending,
    Done(T),
    Failed(String),
}

impl<T> Probe<T> {
    pub(crate) fn is_pending(&self) -> bool {
        matches!(self, Probe::Pending)
    }
}

/// The four wizard steps, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    MachineRepo,
    IdTitle,
    Cast,
    CheckStart,
}

impl Step {
    /// `[1/4] Machine & Repo` — the title row's step name.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Step::MachineRepo => "Machine & Repo",
            Step::IdTitle => "ID & Title",
            Step::Cast => "Cast",
            Step::CheckStart => "Check & Start",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Step::MachineRepo => 1,
            Step::IdTitle => 2,
            Step::Cast => 3,
            Step::CheckStart => 4,
        }
    }
}

/// Which half of step 1 the keys act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step1Pane {
    Machines,
    Repos,
}

/// One machine row: this core (`None`) or an attached endpoint.
#[derive(Debug, Clone)]
pub(crate) struct MachineRow {
    /// `None` is Local — `run_output`'s local branch.
    pub name: Option<String>,
    /// The row label — `Local`, or the endpoint's name.
    pub label: String,
    /// What the core last reported — attached machines only; Local
    /// is always `Online`.
    pub state: MachineState,
    /// `backlot --version` on the machine: the first line, or why it
    /// could not run.
    pub backlot: Probe<String>,
}

/// Machine reachability as `machine.list` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MachineState {
    Online,
    Offline(String),
}

impl MachineRow {
    /// The machine is usable: online and `backlot --version` answered.
    pub(crate) fn usable(&self) -> bool {
        self.state == MachineState::Online && matches!(self.backlot, Probe::Done(_))
    }
}

/// A `repos --json` entry's `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepoKind {
    GitReady,
    GitEmpty,
    Dir,
}

/// One listed directory from `backlot repos --json`.
#[derive(Debug, Clone)]
pub(crate) struct RepoEntry {
    pub name: String,
    pub path: String,
    pub kind: RepoKind,
    pub in_use_studio: Option<String>,
    pub branch: Option<String>,
}

/// The repository browser's listing state for the selected machine.
#[derive(Debug)]
pub(crate) struct Repos {
    /// The breadcrumb's absolute path on the target machine — `None`
    /// until the first listing answers (that call asks for `$HOME`).
    pub dir: Option<String>,
    /// `$HOME` on the target, learned from the first listing's `cwd`
    /// — ascending past it is refused client-side.
    pub home: Option<String>,
    pub listing: Probe<Vec<RepoEntry>>,
    /// Bumped on every request; a stale answer never lands.
    pub epoch: u64,
    /// `/`'s text and whether the field owns keys.
    pub filter: TextEditor,
    pub filtering: bool,
    pub sel: usize,
    pub scroll: usize,
}

impl Default for Repos {
    fn default() -> Self {
        Repos {
            dir: None,
            home: None,
            listing: Probe::Pending,
            epoch: 0,
            filter: TextEditor::new("", false),
            filtering: false,
            sel: 0,
            scroll: 0,
        }
    }
}

impl Repos {
    /// Entries surviving the filter — case-insensitive substring on
    /// the name, like the picker's own `filter`.
    pub(crate) fn visible(&self) -> Vec<&RepoEntry> {
        let entries = match &self.listing {
            Probe::Done(entries) => entries,
            _ => return Vec::new(),
        };
        let needle = self.filter.as_str().trim().to_lowercase();
        entries
            .iter()
            .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle))
            .collect()
    }
}

/// The `repos --json` document: `cwd` plus the classified entries.
/// A document that does not parse is the machine's answer verbatim.
pub(crate) fn parse_repos(text: &str) -> Result<(String, Vec<RepoEntry>), String> {
    #[derive(serde::Deserialize)]
    struct Doc {
        cwd: String,
        #[serde(default)]
        entries: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        name: String,
        path: String,
        kind: String,
        #[serde(default)]
        in_use_studio: Option<String>,
        #[serde(default)]
        branch: Option<String>,
    }
    let doc: Doc = serde_json::from_str(text).map_err(|e| format!("repos: bad document: {e}"))?;
    let entries = doc
        .entries
        .into_iter()
        .map(|e| RepoEntry {
            name: e.name,
            path: e.path,
            kind: match e.kind.as_str() {
                "git_ready" => RepoKind::GitReady,
                "git_empty" => RepoKind::GitEmpty,
                _ => RepoKind::Dir,
            },
            in_use_studio: e.in_use_studio,
            branch: e.branch,
        })
        .collect();
    Ok((doc.cwd, entries))
}

/// A `runners --json` row — the detection verdict plus the models
/// and efforts the cast table cycles.
#[derive(Debug, Clone)]
pub(crate) struct RunnerRow {
    pub runner: String,
    /// `found` | `not working` | `missing`.
    pub status: String,
    pub version: Option<String>,
    pub models: Vec<String>,
    pub efforts: Vec<String>,
}

impl RunnerRow {
    /// The runner can seat a role — detection answered.
    pub(crate) fn found(&self) -> bool {
        self.status == "found"
    }
}

/// The `runners --json` document.
pub(crate) fn parse_runners(text: &str) -> Result<Vec<RunnerRow>, String> {
    #[derive(serde::Deserialize)]
    struct Row {
        runner: String,
        status: String,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        models: Vec<String>,
        #[serde(default)]
        efforts: Vec<String>,
    }
    let rows: Vec<Row> =
        serde_json::from_str(text).map_err(|e| format!("runners: bad document: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|r| RunnerRow {
            runner: r.runner,
            status: r.status,
            version: r.version,
            models: r.models,
            efforts: r.efforts,
        })
        .collect())
}

/// A repo name → a studio id: lowercase, the `[a-z0-9_-]` alphabet,
/// a leading non-alnum run collapsed to nothing, length-capped —
/// the same derivation the wizard's repo-answer default applies.
pub(crate) fn studio_id(repo_name: &str) -> String {
    let mut out = String::new();
    for ch in repo_name.chars() {
        let ch = ch.to_ascii_lowercase();
        match ch {
            'a'..='z' | '0'..='9' | '-' | '_' => out.push(ch),
            _ => out.push('-'),
        }
    }
    let out = out.trim_start_matches(['-', '_']).to_string();
    out.chars().take(16).collect()
}

/// `title`'s prefill: the repo name humanized — separators become
/// spaces so `rate-limit-tokens` reads `rate limit tokens`.
pub(crate) fn title_prefill(repo_name: &str) -> String {
    repo_name.replace(['-', '_'], " ")
}

/// Live studio-id validation against the pattern and the machine's
/// existing studios — `Err(text)` is shown inline, `Ok(())` passes.
pub(crate) fn id_error(id: &str, existing: &[String]) -> Result<(), String> {
    if id.is_empty() {
        return Err("studio id is empty".into());
    }
    if id.len() > 16 {
        return Err("studio id is longer than 16 chars".into());
    }
    if !id
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return Err("must start with [a-z0-9]".into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err("want [a-z0-9][a-z0-9_-]*".into());
    }
    if existing.iter().any(|s| s == id) {
        return Err(format!("'{id}' already exists on this machine"));
    }
    Ok(())
}

/// One cast-table row: a fixed role or an actor candidate.
#[derive(Debug, Clone)]
pub(crate) struct CastRow {
    /// The ledger key: `producer`, `director`, `writer`, `actor-a`…
    pub key: String,
    /// Whether the row is a `[[candidates]]` entry — `x` removes it.
    pub candidate: bool,
    /// Index into the runners list.
    pub runner: usize,
    /// Index into the runner's `models`, 0 when it has none.
    pub model: usize,
    /// Index into the runner's `efforts`, 0 when it has none.
    pub effort: usize,
}

/// The cast table: producer, director, writer plus candidates the
/// user added with `a`.
#[derive(Debug)]
pub(crate) struct CastTable {
    pub rows: Vec<CastRow>,
    /// Selected row.
    pub sel: usize,
    /// Selected column: 0 runner, 1 model, 2 effort.
    pub col: usize,
    /// `Ctrl+E`'s hand-edited cast.toml — `Some` means the table was
    /// replaced by the user's file and the summary shows it.
    pub custom: Option<String>,
    /// The edited file's parse error, when it has one.
    pub custom_error: Option<String>,
}

impl CastTable {
    /// The draft: one row per fixed role, no candidates yet — all on
    /// the first `found` runner with its first model/effort.
    pub(crate) fn draft(runners: &[RunnerRow]) -> CastTable {
        let seat = runners.iter().position(|r| r.found()).unwrap_or(0);
        let row = |key: &str, candidate: bool| CastRow {
            key: key.to_string(),
            candidate,
            runner: seat,
            model: 0,
            effort: 0,
        };
        CastTable {
            rows: vec![
                row("producer", false),
                row("director", false),
                row("writer", false),
                row("actor-a", true),
                row("actor-b", true),
            ],
            sel: 0,
            col: 0,
            custom: None,
            custom_error: None,
        }
    }

    /// Cycle the selected cell's value among the runner's options;
    /// `delta` is ±1. The runner cell's change reseats model and
    /// effort to that runner's defaults.
    pub(crate) fn cycle(&mut self, runners: &[RunnerRow], delta: isize) {
        let Some(row) = self.rows.get_mut(self.sel) else {
            return;
        };
        if runners.is_empty() {
            return;
        }
        match self.col {
            0 => {
                row.runner = cycle(row.runner, runners.len(), delta);
                row.model = 0;
                row.effort = 0;
            }
            1 => {
                let n = runners[row.runner].models.len().max(1);
                row.model = cycle(row.model, n, delta);
            }
            _ => {
                let n = runners[row.runner].efforts.len().max(1);
                row.effort = cycle(row.effort, n, delta);
            }
        }
    }

    /// `a`: append an actor candidate — `actor-c`, `actor-d`, … by
    /// the next free letter.
    pub(crate) fn add(&mut self, runners: &[RunnerRow]) {
        let used: std::collections::BTreeSet<&str> =
            self.rows.iter().map(|r| r.key.as_str()).collect();
        let seat = runners.iter().position(|r| r.found()).unwrap_or(0);
        for letter in (b'a'..=b'z').map(char::from) {
            let key = format!("actor-{letter}");
            if !used.contains(key.as_str()) {
                self.rows.push(CastRow {
                    key,
                    candidate: true,
                    runner: seat,
                    model: 0,
                    effort: 0,
                });
                self.sel = self.rows.len() - 1;
                return;
            }
        }
    }

    /// `x`: remove the selected candidate row — fixed roles stay.
    pub(crate) fn remove(&mut self) {
        if self.rows.get(self.sel).is_some_and(|row| row.candidate) {
            self.rows.remove(self.sel);
            self.sel = self.sel.min(self.rows.len().saturating_sub(1));
        }
    }

    /// The generated `cast.toml` — `custom` wins when the user took
    /// the file over with `Ctrl+E`. R14: an experimental `mode`
    /// writes `mode = "experimental"` beside `rework_cap`; standard
    /// writes no mode line at all.
    pub(crate) fn toml(
        &self,
        runners: &[RunnerRow],
        mode: backlot_model::event::StudioMode,
    ) -> String {
        if let Some(custom) = &self.custom {
            return custom.clone();
        }
        let seat = |row: &CastRow| -> String {
            let Some(runner) = runners.get(row.runner) else {
                return String::new();
            };
            let mut out = format!("runner = {:?}\n", runner.runner);
            if let Some(model) = runner.models.get(row.model) {
                out.push_str(&format!("model = {model:?}\n"));
            }
            if let Some(effort) = runner.efforts.get(row.effort) {
                out.push_str(&format!("effort = {effort:?}\n"));
            }
            out.push_str("permission_profile = \"standard\"\n");
            out
        };
        let mode_line = match mode {
            backlot_model::event::StudioMode::Experimental => "mode = \"experimental\"\n",
            backlot_model::event::StudioMode::Standard => "",
        };
        let mut text = format!(
            "# cast.toml — generated by the New Studio dialog.\nrework_cap = 2\n{mode_line}\n"
        );
        for row in &self.rows {
            if row.candidate {
                text.push_str(&format!("[[candidates]]\nname = {:?}\n", row.key));
            } else {
                text.push_str(&format!("[{}]\n", row.key));
            }
            text.push_str(&seat(row));
            text.push('\n');
        }
        text
    }
}

fn cycle(index: usize, n: usize, delta: isize) -> usize {
    if n == 0 {
        return 0;
    }
    (index as isize + delta).rem_euclid(n as isize) as usize
}
