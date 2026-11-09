# Studio panel

The right-hand studio column (S4b). Everything below is client-local state — per client, in
memory only. With the panel closed the client is cell-identical to herdr.

## Toggle

`prefix+y` cycles: closed → expanded and focused → (unfocused) focused → closed. `y` is unbound
in herdr's defaults and the parity scripts never send it. A configured herdr binding on the same
tail still wins while the panel is closed or unfocused.

## Focused keys

| Key | Action |
|---|---|
| `j`/`k`, `↑`/`↓` | Move the selection one row |
| `PageUp`/`PageDown` | Move one viewport |
| `Esc` | Leave focus; the panel stays open |
| `c` | Switch collapsed and expanded |
| `Space` | Collapse/expand the selected group |
| `Enter` | Open person: focus their terminal. Studio: focus its producer. Group: collapse. Closed person: arm `enter: reopen`, a second Enter runs `backlot reopen`. A non-resumable person says why. Collapsed-mode working group: expand. |
| `+`/`=` | Raise the selected actor's task priority (`backlot priority`) |
| `-`/`_` | Lower it |

All other keys are eaten while the panel owns focus, except the prefix, which still arms the
toggle chord.

## Mouse

- Click on the title flips collapsed and expanded.
- Click on a row selects it and acts like `Enter`.
- The wheel scrolls the body; the scrollbar clicks and drags.
- A click outside a focused panel hands focus back to the panes.

## Config

`$XDG_CONFIG_HOME/backlot/config.toml` (default `~/.config/backlot/config.toml`):

```toml
[panel]
width = 30  # expanded column width, clamped to 24-60
key = "y"   # the chord tail after the prefix
```

A missing file means defaults. A malformed file means defaults plus one notice carrying only
the line and column — file contents never reach the screen. herdr's own `config.toml` is read
exactly as before.

## Behaviour notes

- Collapsed is a 16-column strip listing only the groups with someone working (`director`,
  `actors 2`), or a dim `idle` row.
- Expanded shows a title (`working: director, actors 2`, else `idle`), each studio, the groups
  `producer`, `director`, `writer`, `actors`, and under each group its people: mark, name,
  priority flag, `runner/model`, a one-line `doing` and the attention text. Empty groups are
  skipped; closed people stay listed.
- The five marks — working (animated), waiting, blocked, lost, closed — pair a distinct symbol
  with a `chrome/status.rs` colour; an unknown `work` reads as waiting.
- Working marks spin about every 120 ms through a one-shot deadline that arms only while a
  working mark is actually drawn; an idle or closed panel wakes nothing.
- Beside the panes only when past the mobile threshold with at least 60 columns left for them;
  otherwise the open panel covers the frame and the pane geometry keeps the closed shape.
- The document is the local `backlot` summary, schema `backlot/1`. A missing, malformed or
  foreign-schema document reads as `no studios`, never an error. Without a live `backlot-stage`
  terminal the panel shows `stage not running` and keeps the last document.
- `reopen`/`priority` run `backlot --studio <id> --as user …` off the event loop through the
  injected runner (`std::env::current_exe()` in production, a recorder in tests); a failure
  shows its first stderr line.
