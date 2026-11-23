//! The New Studio dialog's key and mouse handling. The dialog owns
//! every key while it is open: `Esc` steps back 4→3→2→1 then
//! closes, `Ctrl+C` cancels from any step, and a raw `^[` is never
//! emitted — the dialog is a chrome overlay, not a terminal.

use hc_vt::{Key, Mods};

use crate::app::App;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, TerminalKey};
use crate::app::mouse::contains;
use crate::new_studio::{self, NsHit, Probe, RepoKind, Step, Step1Pane, Step2Field};

impl App {
    /// A key while the dialog is open. Editors take keys first
    /// (the step-2 fields, a model cell's `e`); navigation and
    /// the Esc rule wrap them — step 1's filter is always live.
    pub(crate) fn new_studio_key(&mut self, key: &TerminalKey) {
        let Some(d) = &mut self.new_studio else {
            return;
        };
        // `^C` cancels from any step — the spec's one hard kill.
        if key.input.key == Key::Char('c') && key.input.mods == Mods::CTRL {
            self.new_studio = None;
            return;
        }
        // The model list owns keys while open: `↑↓` move, typing
        // narrows it live, `⏎` seats the row's model, `esc` drops
        // the list and keeps the old value — the repo picker's
        // own rules.
        if d.model_pick.is_some() {
            match key.input.key {
                Key::Escape => d.model_pick = None,
                Key::Enter => {
                    let model = d
                        .model_pick
                        .as_ref()
                        .and_then(|pick| pick.chosen().map(str::to_string));
                    d.model_pick = None;
                    if let Some(model) = model {
                        d.seat_model(&model);
                    }
                }
                Key::ArrowUp | Key::ArrowDown => {
                    let pick = d.model_pick.as_mut().unwrap();
                    let n = pick.visible().len();
                    if n > 0 {
                        let delta = if key.input.key == Key::ArrowUp { -1 } else { 1 };
                        pick.sel = (pick.sel as isize + delta).rem_euclid(n as isize) as usize;
                    }
                }
                _ => {
                    let pick = d.model_pick.as_mut().unwrap();
                    if pick.filter.handle_key(key) == Some(true) {
                        pick.sel = 0;
                        pick.scroll = 0;
                    }
                }
            }
            return;
        }
        // A free-text model cell owns keys until Enter/Esc.
        if d.model_edit.is_some() {
            match key.input.key {
                Key::Enter => {
                    let Some(edit) = d.model_edit.take() else {
                        return;
                    };
                    let text = edit.as_str().trim().to_string();
                    if !text.is_empty() {
                        d.seat_model(&text);
                    }
                }
                Key::Escape => {
                    d.model_edit = None;
                }
                _ => {
                    if let Some(edit) = &mut d.model_edit {
                        edit.handle_key(key);
                    }
                }
            }
            return;
        }
        match key.input.key {
            Key::Escape => {
                // 4 → 3 → 2 → 1 → closed, values kept (the spec's
                // Esc rule — `^[` never reaches a program). On step
                // 1 a non-empty filter eats the first Esc — it
                // clears, the next one cancels.
                match d.step {
                    Step::MachineRepo if !d.repos.filter.as_str().is_empty() => {
                        d.repos.filter.clear();
                        d.repos.sel = 0;
                        d.repos.scroll = 0;
                    }
                    Step::MachineRepo => self.new_studio = None,
                    Step::IdTitle => d.step = Step::MachineRepo,
                    Step::Cast => d.step = Step::IdTitle,
                    Step::CheckStart => d.step = Step::Cast,
                }
            }
            _ => match d.step {
                Step::MachineRepo => self.ns_key_step1(key),
                Step::IdTitle => self.ns_key_step2(key),
                Step::Cast => self.ns_key_step3(key),
                Step::CheckStart => self.ns_key_step4(key),
            },
        }
    }

    /// Step 1: the machines column and the repository list, with
    /// `tab` between them and `^R` a re-check. Every other
    /// text-shaped key is the always-live filter's — typing
    /// narrows the repository list, no mode to enter first.
    fn ns_key_step1(&mut self, key: &TerminalKey) {
        let ctrl_r = key.input.key == Key::Char('r') && key.input.mods == Mods::CTRL;
        if ctrl_r {
            self.ns_probe_versions();
            let d = self.new_studio.as_mut().unwrap();
            d.repos.dir = None;
            d.repos.home = None;
            self.ns_probe_repos(None);
            return;
        }
        match key.input.key {
            Key::Tab | Key::BackTab => {
                let d = self.new_studio.as_mut().unwrap();
                d.pane = match d.pane {
                    Step1Pane::Machines => Step1Pane::Repos,
                    Step1Pane::Repos => Step1Pane::Machines,
                };
            }
            Key::ArrowUp if key.input.mods == Mods::NONE => self.ns_step1_move(-1),
            Key::ArrowDown if key.input.mods == Mods::NONE => self.ns_step1_move(1),
            Key::ArrowRight if key.input.mods == Mods::NONE => self.ns_repo_descend(),
            Key::ArrowLeft if key.input.mods == Mods::NONE => self.ns_repo_ascend(),
            Key::Enter => self.ns_step1_enter(),
            _ => {
                // An actual edit re-seats the selection and claims
                // the pane it narrows — `hjkl`, `/` and Backspace
                // are just text here.
                let d = self.new_studio.as_mut().unwrap();
                if d.repos.filter.handle_key(key) == Some(true) {
                    d.pane = Step1Pane::Repos;
                    d.repos.sel = 0;
                    d.repos.scroll = 0;
                }
            }
        }
    }

    /// `↑↓` inside the active half: a machine switch re-lists the
    /// repositories and refreshes the dup-check set.
    fn ns_step1_move(&mut self, delta: isize) {
        let Some(d) = &mut self.new_studio else {
            return;
        };
        match d.pane {
            Step1Pane::Machines => {
                if d.machines.is_empty() {
                    return;
                }
                let next =
                    (d.machine_sel as isize + delta).rem_euclid(d.machines.len() as isize) as usize;
                if next != d.machine_sel {
                    d.machine_sel = next;
                    // The other machine's breadcrumb and home anchor
                    // no longer apply — its $HOME listing re-seeds.
                    d.repos.dir = None;
                    d.repos.home = None;
                    d.repos.filter = crate::chrome::text_editor::TextEditor::new("", false);
                    self.ns_refresh_studios();
                    self.ns_probe_repos(None);
                }
            }
            Step1Pane::Repos => {
                let n = d.repos.visible().len();
                if n == 0 {
                    return;
                }
                d.repos.sel = (d.repos.sel as isize + delta).rem_euclid(n as isize) as usize;
            }
        }
    }

    /// `→`/`l` descends into the selected directory — `repos
    /// --from` on the target machine again; a repo or file stays.
    fn ns_repo_descend(&mut self) {
        let Some(d) = &self.new_studio else { return };
        let Some(entry) = d.repo() else { return };
        if entry.kind != RepoKind::Dir {
            return;
        }
        let path = entry.path.clone();
        self.ns_probe_repos(Some(path));
    }

    /// `←`/`h` climbs to the parent — never past the machine's
    /// `$HOME` (the first listing taught it).
    fn ns_repo_ascend(&mut self) {
        let Some(d) = &self.new_studio else { return };
        let Some(dir) = d.repos.dir.clone() else {
            return;
        };
        if d.repos.home.as_deref() == Some(dir.as_str()) {
            return;
        }
        let parent = std::path::Path::new(&dir)
            .parent()
            .map(|p| p.to_string_lossy().into_owned());
        if let Some(parent) = parent {
            self.ns_probe_repos(Some(parent));
        }
    }

    /// `Enter`: a machine row moves focus to the repository list;
    /// a repository row picks it and steps forward when its gate
    /// allows — a `git_empty` or unusable row flashes its reason.
    fn ns_step1_enter(&mut self) {
        let Some(d) = &self.new_studio else { return };
        if d.pane == Step1Pane::Machines {
            if d.machine().is_some_and(|m| m.usable()) {
                let d = self.new_studio.as_mut().unwrap();
                d.pane = Step1Pane::Repos;
            }
            return;
        }
        let picked = d.repo().map(|r| (r.path.clone(), r.name.clone()));
        if let Some(reason) = self.new_studio.as_ref().and_then(|d| d.step1_enter_error()) {
            self.show_notice("New studio", &reason);
            return;
        }
        let Some((path, name)) = picked else { return };
        let d = self.new_studio.as_mut().unwrap();
        if d.prefilled_for.as_deref() != Some(path.as_str()) {
            // A different bound path re-opens the no-git question —
            // step 4's toggle starts unchecked again.
            d.no_git = false;
            d.id = crate::chrome::text_editor::TextEditor::new(&new_studio::studio_id(&name), true);
            d.title = crate::chrome::text_editor::TextEditor::new(
                &new_studio::title_prefill(&name),
                true,
            );
            d.prefilled_for = Some(path);
        }
        d.step = Step::IdTitle;
        d.field = Step2Field::Id;
    }

    /// Step 2: `tab`/`↑↓` between the four fields, `←`/`→`/space
    /// toggles the mode field, `Enter` steps forward when the id
    /// validates — the error stays drawn under it. The build brief
    /// takes multi-line text: `alt+⏎` adds a line, `^E` opens
    /// `$EDITOR`, and a paste lands whole.
    fn ns_key_step2(&mut self, key: &TerminalKey) {
        // `^E` on the build field edits the brief in `$EDITOR`.
        if key.input.key == Key::Char('e')
            && key.input.mods == Mods::CTRL
            && self
                .new_studio
                .as_ref()
                .is_some_and(|d| d.field == Step2Field::Build)
        {
            self.ns_open_build_editor();
            return;
        }
        // `alt+⏎` on the build field adds a line — a plain `⏎` is
        // the step's continue, owned by the match below.
        if key.input.key == Key::Enter
            && key.input.mods.contains(Mods::ALT)
            && self
                .new_studio
                .as_ref()
                .is_some_and(|d| d.field == Step2Field::Build)
        {
            self.new_studio.as_mut().unwrap().build.insert("\n");
            return;
        }
        match key.input.key {
            Key::Tab | Key::BackTab | Key::ArrowUp | Key::ArrowDown => {
                let d = self.new_studio.as_mut().unwrap();
                d.field = match d.field {
                    Step2Field::Id => Step2Field::Title,
                    Step2Field::Title => Step2Field::Mode,
                    Step2Field::Mode => Step2Field::Build,
                    Step2Field::Build => Step2Field::Id,
                };
            }
            Key::Enter => {
                let ok = self
                    .new_studio
                    .as_ref()
                    .is_some_and(|d| new_studio::id_error(d.id.as_str(), &d.studios).is_ok());
                if ok {
                    let d = self.new_studio.as_mut().unwrap();
                    d.step = Step::Cast;
                    self.ns_probe_runners();
                }
            }
            _ => {
                let d = self.new_studio.as_mut().unwrap();
                match d.field {
                    // R14: the mode field is a toggle, not a text
                    // editor — arrows and space flip it, other keys
                    // do nothing.
                    Step2Field::Mode => {
                        if matches!(
                            key.input.key,
                            Key::ArrowLeft | Key::ArrowRight | Key::Char(' ')
                        ) {
                            use backlot_model::event::StudioMode;
                            d.mode = match d.mode {
                                StudioMode::Standard => StudioMode::Experimental,
                                StudioMode::Experimental => StudioMode::Standard,
                            };
                        }
                    }
                    Step2Field::Id => {
                        d.id.handle_key(key);
                    }
                    Step2Field::Title => {
                        d.title.handle_key(key);
                    }
                    Step2Field::Build => {
                        let combo = crate::input::terminal_key_of(key);
                        if let Some(text) = key.text_commit.as_deref().filter(|t| !t.is_empty()) {
                            d.build.insert(text);
                        } else {
                            d.build.key(combo.code, combo.modifiers);
                        }
                    }
                }
            }
        }
    }

    /// `^E` on step 2's build field: write the brief to a temp file
    /// and open `$VISUAL`/`$EDITOR` on it in a floating terminal —
    /// on the editor's exit the file is read back into the field.
    fn ns_open_build_editor(&mut self) {
        let Some(d) = &self.new_studio else { return };
        let text = d.build.text();
        let file = std::env::temp_dir().join(format!("backlot-ask-{}.md", std::process::id()));
        if std::fs::write(&file, text).is_err() {
            self.show_notice("New studio", "cannot write the brief for editing");
            return;
        }
        self.ns_build_editor = Some(file.clone());
        let line = format!(
            "${{VISUAL:-${{EDITOR:-vi}}}} {}",
            sh_quote(&file.to_string_lossy())
        );
        self.spawn_ns_editor(line);
    }

    /// The build brief's editor popup closed — read the file back
    /// into the field and remove it.
    pub(crate) fn ns_build_editor_check(&mut self) {
        let Some(file) = self.ns_build_editor.clone() else {
            return;
        };
        if self.popup.is_some() || self.popup_pending.is_some() {
            return;
        }
        self.ns_build_editor = None;
        let Some(d) = &mut self.new_studio else {
            let _ = std::fs::remove_file(&file);
            return;
        };
        if let Ok(text) = std::fs::read_to_string(&file) {
            d.build = crate::studio_dialog::BodyEditor::new(&text);
        }
        let _ = std::fs::remove_file(&file);
    }

    /// Step 3: the cast table — `↑↓` rows, `tab`/`←→` the cell,
    /// `space` the focused cell's picker (MODEL) or its next value
    /// (RUNNER/EFFORT), `⏎` always the Step-4 dry run, `a`/`x` the
    /// candidates, `e` free text on a model, `^E` the generated file
    /// in `$EDITOR`.
    fn ns_key_step3(&mut self, key: &TerminalKey) {
        if key.input.key == Key::Char('r') && key.input.mods == Mods::CTRL {
            self.ns_probe_runners();
            return;
        }
        if key.input.key == Key::Char('e') && key.input.mods == Mods::CTRL {
            self.ns_open_cast_editor();
            return;
        }
        if key.input.key == Key::Enter {
            // `⏎` advances to the dry run on every column. The model
            // list is `space`'s; while it is open the earlier branch
            // seats the pick, so a second `⏎` lands here and advances.
            let ready = self
                .new_studio
                .as_ref()
                .is_some_and(|d| matches!(d.runners, Probe::Done(_)));
            if ready {
                self.new_studio.as_mut().unwrap().step = Step::CheckStart;
                self.ns_dry_run();
            }
            return;
        }
        let d = self.new_studio.as_mut().unwrap();
        match key.input.key {
            Key::ArrowUp | Key::Char('k') if key.input.mods == Mods::NONE => {
                d.cast.sel = d.cast.sel.saturating_sub(1);
            }
            Key::ArrowDown | Key::Char('j') if key.input.mods == Mods::NONE => {
                d.cast.sel = (d.cast.sel + 1).min(d.cast.rows.len().saturating_sub(1));
            }
            Key::Tab => {
                d.cast.col = (d.cast.col + 1) % 3;
            }
            Key::BackTab => {
                d.cast.col = (d.cast.col + 2) % 3;
            }
            Key::ArrowLeft | Key::Char('h') if key.input.mods == Mods::NONE => {
                let runners = match &d.runners {
                    Probe::Done(rows) => rows.as_slice(),
                    _ => &[],
                };
                d.cast.cycle(runners, -1);
            }
            Key::ArrowRight | Key::Char('l') if key.input.mods == Mods::NONE => {
                let runners = match &d.runners {
                    Probe::Done(rows) => rows.as_slice(),
                    _ => &[],
                };
                d.cast.cycle(runners, 1);
            }
            Key::Char('a') if key.input.mods == Mods::NONE => {
                let runners = match &d.runners {
                    Probe::Done(rows) => rows.as_slice(),
                    _ => &[],
                };
                d.cast.add(runners);
            }
            Key::Char('x') if key.input.mods == Mods::NONE => {
                d.cast.remove();
            }
            Key::Char(' ') if key.input.mods == Mods::NONE => {
                // MODEL opens its catalog; RUNNER and EFFORT have no
                // sub-picker, so space takes their next value.
                if d.cast.col == 1 {
                    d.open_model_pick();
                } else {
                    let runners = match &d.runners {
                        Probe::Done(rows) => rows.as_slice(),
                        _ => &[],
                    };
                    d.cast.cycle(runners, 1);
                }
            }
            Key::Char('e') if key.input.mods == Mods::NONE && d.cast.col == 1 => {
                let current = match &d.runners {
                    Probe::Done(runners) => runners
                        .get(d.cast.rows.get(d.cast.sel).map(|r| r.runner).unwrap_or(0))
                        .and_then(|r| r.models.get(d.cast.rows[d.cast.sel].model).cloned())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                d.model_edit = Some(crate::chrome::text_editor::TextEditor::new(&current, false));
            }
            _ => {}
        }
    }

    /// `^E`: write the generated cast.toml to a temp file and open
    /// `$VISUAL`/`$EDITOR` on it in a floating terminal — on the
    /// editor's exit the file is reloaded and validated.
    fn ns_open_cast_editor(&mut self) {
        let Some(d) = &self.new_studio else { return };
        let runners = runners_of(d);
        let text = d.cast.toml(runners, d.mode);
        let file = std::env::temp_dir().join(format!("backlot-cast-{}.toml", std::process::id()));
        if std::fs::write(&file, text).is_err() {
            self.show_notice("New studio", "cannot write the cast file for editing");
            return;
        }
        self.ns_editor = Some(file.clone());
        let line = format!(
            "${{VISUAL:-${{EDITOR:-vi}}}} {}",
            sh_quote(&file.to_string_lossy())
        );
        self.spawn_ns_editor(line);
    }

    /// The editor's popup closed — read the file back: a valid
    /// parse replaces the table's rows with the file (custom);
    /// a broken one shows its error and keeps the table.
    pub(crate) fn ns_editor_check(&mut self) {
        let Some(file) = self.ns_editor.clone() else {
            return;
        };
        if self.popup.is_some() || self.popup_pending.is_some() {
            return;
        }
        self.ns_editor = None;
        let Some(d) = &mut self.new_studio else {
            let _ = std::fs::remove_file(&file);
            return;
        };
        match backlot_stage::validate_cast_file(&file) {
            Ok(()) => {
                if let Ok(text) = std::fs::read_to_string(&file) {
                    d.cast.custom = Some(text);
                    d.cast.custom_error = None;
                }
            }
            Err(e) => {
                d.cast.custom_error = Some(e);
            }
        }
        let _ = std::fs::remove_file(&file);
    }

    /// Step 4: `space` the start-stage checkbox, `g` the no-git
    /// toggle a plain-directory pick offers, `Enter` runs the
    /// create — a failed dry run retries it first.
    fn ns_key_step4(&mut self, key: &TerminalKey) {
        match key.input.key {
            Key::Char(' ') if key.input.mods == Mods::NONE => {
                let d = self.new_studio.as_mut().unwrap();
                d.start_stage = !d.start_stage;
            }
            Key::Char('g') if key.input.mods == Mods::NONE => {
                let plain = self
                    .new_studio
                    .as_ref()
                    .and_then(|d| d.repo().map(|r| r.shape()))
                    == Some(new_studio::PathShape::Plain);
                if plain {
                    let d = self.new_studio.as_mut().unwrap();
                    d.no_git = !d.no_git;
                    self.ns_dry_run();
                }
            }
            Key::Enter => self.ns_step4_enter(),
            _ => {}
        }
    }

    /// Step 4's `Enter` (key or Create-button click): a pending or
    /// failed dry run retries it; a passed one creates.
    pub(crate) fn ns_step4_enter(&mut self) {
        let Some(d) = &mut self.new_studio else {
            return;
        };
        if d.creating {
            return;
        }
        match &d.dry {
            Probe::Failed(_) | Probe::Pending => self.ns_dry_run(),
            Probe::Done(_) => self.ns_create(),
        }
    }

    /// Mouse while the dialog is open: a click outside closes (the
    /// overlay's rule), a row or cell selects what it paints.
    pub(crate) fn new_studio_mouse(&mut self, mouse: MouseInput) {
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let d = self.new_studio.as_mut().unwrap();
            let delta = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                -1
            } else {
                1
            };
            if let Some(pick) = &mut d.model_pick {
                // The wheel walks the open list's selection.
                let n = pick.visible().len();
                if n > 0 {
                    pick.sel = (pick.sel as isize + delta).rem_euclid(n as isize) as usize;
                }
            } else if d.step == Step::MachineRepo && d.pane == Step1Pane::Repos {
                self.ns_step1_move(delta);
            }
            return;
        }
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        let point = (mouse.col, mouse.row);
        let hits = self.ns_hits.cells.clone();
        if !contains(self.ns_hits.area, point) {
            self.new_studio = None;
            return;
        }
        for (rect, hit) in hits.iter().rev() {
            if !contains(*rect, point) {
                continue;
            }
            // The open model list owns the mouse like the keys: its
            // rows pick, every other hit stays put.
            if self
                .new_studio
                .as_ref()
                .is_some_and(|d| d.model_pick.is_some())
                && !matches!(hit, NsHit::ModelPick(_))
            {
                return;
            }
            match hit {
                NsHit::Machine(i) => {
                    let i = *i;
                    let switch = self.new_studio.as_ref().is_some_and(|d| d.machine_sel != i);
                    if let Some(d) = &mut self.new_studio {
                        d.machine_sel = i;
                        if switch {
                            d.repos.dir = None;
                            d.repos.home = None;
                            d.repos.filter = crate::chrome::text_editor::TextEditor::new("", false);
                        }
                    }
                    if switch {
                        self.ns_refresh_studios();
                        self.ns_probe_repos(None);
                    }
                }
                NsHit::Repo(i) => {
                    let i = *i;
                    let Some(d) = &mut self.new_studio else {
                        return;
                    };
                    let was = d.repos.sel;
                    d.repos.sel = i;
                    // Click a directory to enter it; a repo's second
                    // click picks it.
                    let kind = d.repo().map(|r| r.kind);
                    if kind == Some(RepoKind::Dir) {
                        self.ns_repo_descend();
                    } else if was == i {
                        self.ns_step1_enter();
                    }
                }
                NsHit::StepTab(step) => {
                    if let Some(d) = &mut self.new_studio {
                        d.step = *step;
                    }
                }
                NsHit::IdField => {
                    if let Some(d) = &mut self.new_studio {
                        d.field = Step2Field::Id;
                    }
                }
                NsHit::TitleField => {
                    if let Some(d) = &mut self.new_studio {
                        d.field = Step2Field::Title;
                    }
                }
                NsHit::ModeField => {
                    if let Some(d) = &mut self.new_studio {
                        use backlot_model::event::StudioMode;
                        d.field = Step2Field::Mode;
                        d.mode = match d.mode {
                            StudioMode::Standard => StudioMode::Experimental,
                            StudioMode::Experimental => StudioMode::Standard,
                        };
                    }
                }
                NsHit::BuildField => {
                    if let Some(d) = &mut self.new_studio {
                        d.field = Step2Field::Build;
                    }
                }
                NsHit::CastCell { row, col } => {
                    if let Some(d) = &mut self.new_studio {
                        let seat = (*row).min(d.cast.rows.len().saturating_sub(1));
                        // The repo rows' own rule: the second click
                        // acts — an already-active MODEL cell opens
                        // its list.
                        let open = d.cast.sel == seat && d.cast.col == *col && *col == 1;
                        d.cast.sel = seat;
                        d.cast.col = *col;
                        if open {
                            d.open_model_pick();
                        }
                    }
                }
                NsHit::ModelPick(i) => {
                    if let Some(d) = &mut self.new_studio
                        && let Some(pick) = &mut d.model_pick
                    {
                        pick.sel = *i;
                        // A click picks outright — the menu rows' rule.
                        let model = pick.chosen().map(str::to_string);
                        d.model_pick = None;
                        if let Some(model) = model {
                            d.seat_model(&model);
                        }
                    }
                }
                NsHit::StartToggle => {
                    if let Some(d) = &mut self.new_studio {
                        d.start_stage = !d.start_stage;
                    }
                }
                NsHit::NoGitToggle => {
                    let plain = self
                        .new_studio
                        .as_ref()
                        .and_then(|d| d.repo().map(|r| r.shape()))
                        == Some(new_studio::PathShape::Plain);
                    if plain {
                        self.new_studio.as_mut().unwrap().no_git ^= true;
                        self.ns_dry_run();
                    }
                }
                NsHit::Create => self.ns_step4_enter(),
            }
            return;
        }
    }
}

/// The probed runners when they landed — the table's options.
fn runners_of(d: &new_studio::NewStudio) -> &[new_studio::RunnerRow] {
    match &d.runners {
        Probe::Done(rows) => rows.as_slice(),
        _ => &[],
    }
}

/// POSIX single-quote escaping — the same rule the wizard line
/// used for its file arguments.
pub(super) fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}
