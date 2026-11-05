//! Initial attach state and verified program identity.
use super::*;

impl Pane {
    /// The attach landed: the writer spawns and buffered input stays
    /// parked — the retained screen still shows until the first DATA
    /// builds a fresh VT at the reply's authoritative grid (input
    /// encodes under that fresh VT, not the old modes). A
    /// *user-initiated* attach (`resumed: false`) claims the allocated
    /// size when the core grid disagrees; a resumed attach never
    /// synthesizes ownership activity — a differing grid is marked.
    pub(super) fn on_opened(&mut self, attach: Arc<Attach>) {
        let State::Opening {
            generation,
            spec,
            task,
            out,
            out_rx,
            resumed,
            ..
        } = std::mem::replace(&mut self.state, State::Idle)
        else {
            return;
        };
        let term = attach.terminal();
        let incarnation = (term.created_ms, term.pid);
        if resumed && self.incarnation.is_some_and(|old| old != incarnation) {
            self.discard_pending_incarnation();
        }
        self.incarnation = Some(incarnation);
        let core_grid = (term.cols.max(1), term.rows.max(1));
        self.vt_grid = core_grid;
        let negotiated = attach.resize_notifications();
        let mismatched = spec.role == Role::Active && core_grid != self.allocated;
        let desired_facts = if spec.role == Role::Active {
            self.host.display()
        } else {
            None
        };
        let writer = tokio::spawn(writer_task(
            attach.clone(),
            out_rx,
            self.ev.clone(),
            generation,
        ));
        let observe = spec.role == Role::Observe;
        self.state = State::Live(Box::new(Live {
            held: held::Held::default(),
            spec,
            attach,
            out,
            reader: task,
            writer,
            core_grid,
            negotiated,
            await_paint: true,
            paint_started: false,
            paint_tail: Vec::new(),
            repaint: None,
            sent_size: None,
            desired_size: None,
            desired_focus: if observe {
                None
            } else if resumed {
                self.outer_focus
                    .filter(|focused| !*focused || self.focus_changed_disconnected)
            } else {
                Some(self.outer_focus.unwrap_or(true))
            },
            desired_facts,
            sent_facts: None,
            pending_reply: VecDeque::new(),
            mismatched_grid: mismatched,
            vt_broken: None,
        }));
        // Only a user-initiated attach claims our allocation when the
        // grid differs — a real change of ownership, not synthetic
        // activity on a resumed wire.
        if (!resumed || self.allocation_changed_disconnected)
            && let State::Live(live) = &mut self.state
            && live.spec.role == Role::Active
            && live.core_grid != self.allocated
        {
            live.desired_size = Some(self.allocated);
        }
        self.stage_initial_controls();
        self.focus_changed_disconnected = false;
        self.allocation_changed_disconnected = false;
        self.flush_pending();
    }
}
