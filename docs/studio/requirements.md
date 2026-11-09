# Studio requirements

backlot is a terminal client for `hc` with a studio layer for directing coding agents. hc-tui stays a plain terminal
client; everything about multi-agent coordination lives in this project only.

Numbers R01-R16 follow the original requirement list. R17-R20 were added by the user on 2026-11-09 and specify the
panel's interaction. Model names that appeared in the original examples are examples, not product constraints.

## Requirements

| # | Requirement | First version |
|---|---|---|
| R01 | Built on `hc`, with the same functions as the herdr TUI. Inherited from hc-tui through `upstream`. | Inherited |
| R02 | Extended far enough to carry its own name. | Done: `backlot` |
| R03 | Organized like a film studio. Other structures were to be compared. | See `design.md` |
| R04 | A studio panel on the right side, with producer, director, actors and writer. | Yes |
| R05 | Before development starts, set agent, model and adjustable thinking depth for each role. | Yes |
| R06 | The user normally talks to the producer, who understands the requirement and plans development and execution. Other roles stay reachable. | Yes |
| R07 | The producer discusses generalization, extensibility and alternatives with the writer. | Yes |
| R08 | The producer asks the director to schedule actors. The director checks the work against the writer's content. | Yes |
| R09 | The director reports to the producer at nodes. The producer reports to the user. | Yes |
| R10 | While actors work, the user can keep talking to the producer, adding requirements and correcting the plan. | Yes |
| R11 | The user chooses candidate actors at the start. The director selects several from those candidates to do the work. | Yes |
| R12 | The producer and director evaluate actors, as model plus thinking depth, from the finished work. | Yes |
| R13 | Several studios in the panel, scrollable, with click or switch to see what each person is doing. | One studio; panel is built to scroll |
| R14 | An "experimental production" (试验片) option that switches on a research-level process. | Later |
| R15 | Actors are full agents that complete sub-tasks on their own: reading the repo, editing, running, testing and fixing. | Yes |
| R16 | Evaluation comes from each delivery: the upstream reviewing it, correcting it, what changed, and whether it was reworked repeatedly. Every delivery counts. | Yes |
| R17 | The panel can collapse. Its title names the part that is working. Work states have clearly different colours and marks, ideally animated (spinning, slow blinking). | Yes |
| R18 | The panel has a scrollbar and scrolls. | Yes |
| R19 | Expanded, the panel shows the people in each role. A person whose work is finished is closed, which frees their resources, but is not deleted, so the work can be continued later. | Yes |
| R20 | Several people in one role are ordered by priority. | Yes |

## Confirmed decisions

| # | Decision |
|---|---|
| D1 | The project is named `backlot`, in the public repository `followcat/backlot`. |
| D2 | The studio changes zero lines of hc core. See the constraint below. |
| D3 | The first version is one studio running a real closed loop: talk to the producer, the director dispatches, actors deliver, review and rework are recorded, the panel shows each role's state, and requirements can change midway. Several studios and experimental production come afterwards. Neither is dropped. |
| D4 | A studio is one requirement from start to delivery, bound to one repository and one set of candidate actors. One repository can run several studios. Actor records accumulate across studios. |
| D5 | Producer and director are long-lived sessions. The writer starts only to discuss a plan. Actors start per task. Small tasks can take a light path: the producer dispatches to one actor, or one strong agent does the whole job alone. |
| D6 | With the studio panel closed, the screen stays cell-identical to herdr and the parity harness remains the regression gate. An open panel takes width on the right. At narrow and phone widths the panel does not sit beside the panes; it becomes a full-screen view. |
| D7 | "Several actors" (R11) means different actors take different tasks that can each be delivered on their own, and the director integrates them. Several actors racing on one task is left for later and may suit experimental production. |
| D8 | Evaluation in the first version is the factual record plus a written conclusion from the director on each delivery. There is no numeric score or grade yet. |
| D9 | The director may approve an actor's permission prompts inside that actor's own work area. Anything touching real sessions, production, accounts, global settings, or destructive outside the work area goes to the user. |
| D10 | Each actor has its own worktree and branch. The director integrates accepted deliveries into a studio branch and runs the project's checks there. Merging into the user's own branches stays with the user. |
| D11 | Priority (R20) affects only the display order in the panel, never scheduling. The director sets it per task when casting, the default is normal, and the user can change it in the panel. |
| D12 | The collapsed panel (R17) is a narrow column of about 16 columns listing the working roles and their status marks. Each role group can also collapse on its own. |

## Hard constraint: no changes to hc core

- Use only hc's public API.
- Role sessions are ordinary hc terminals.
- Scheduling is this project's own process, itself run in an hc terminal.
- The ledger and other durable state live in this project's own state directory.
- The panel's live summary is a small core document, written with `doc.set` and read through the watcher.
- If a capability is missing, solve it in the client first. If that is impossible, propose only a general capability
  upstream. Studio concepts never go into core.

## Non-goals for the first version

- Several studios running at once (R13 beyond one studio) and experimental production (R14).
- Automatic casting from past records. The director uses the records; nothing selects for it.
- Any claim that a mixed team beats one strong agent. "One strong agent does it" is a first-class path, so the
  comparison can come from real tasks.
