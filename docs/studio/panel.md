# Studio panel

The right-hand studio column (S4b, reworked by U1). Everything below is client-local state — per
client, in memory only. With the panel closed the client is cell-identical to herdr.

The panel and the sidebar's cast are viewers: what they show comes from the `backlot` document and
the terminal inventory; what they change goes through `backlot` commands and the existing focus
path.

## Studio mode

Everything on this page applies only in **studio mode**: the last good `backlot` document
(`schema = "backlot/1"`) lists at least one studio. A missing, malformed or foreign-schema
document, or a stopped stage, never flips the mode — the panel keeps the last document. Only a
good document with zero studios leaves it. Outside studio mode the client is exactly herdr: the
agents list, herdr's sidebar width, herdr's key bindings.

The **current studio** is the studio whose person owns the focused terminal. When the focus
belongs to no studio the current studio stays what it was; at the start it is the first studio in
document order. The client remembers, per studio, the person it last focused there.

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
| `Space` | Collapse/expand the selected studio's block |
| `Enter` | Open person: focus their terminal. Studio or a block's title: focus its producer (or remembered person). Closed person: arm `enter: reopen`, a second Enter runs `backlot reopen`. A non-resumable person says why. Collapsed-mode row: expand it. |
| `+`/`=` | Raise the selected actor's task priority (`backlot priority`) |
| `-`/`_` | Lower it |

All other keys are eaten while the panel owns focus, except the prefix, which still arms the
toggle chord — and the four studio keys, which still switch studios and roles with the panel
focused.

## Full screen

When the window cannot fit a beside panel (under the mobile threshold, or fewer than 60
columns left for the panes) an open panel covers the whole frame. In that shape:

- The panel owns the keyboard for as long as it is drawn — the `focused` latch does not
  matter, since nobody can see the panes it covers.
- `Esc` closes the panel. The toggle chord also closes it on the first press, even after a
  resize from beside to full screen.
- `Enter` on a live person or a studio focuses that terminal and closes the panel.
- The panel's rect is reset cell by cell before painting, so nothing from the frame
  underneath shows through its blank cells.

Beside the panes nothing changes: `Esc` only leaves focus and the panel stays open; `Enter`
on a live person or studio focuses the terminal and leaves the panel open unfocused.

## The blocks

Expanded, the panel is a narrow column (`[panel] width`, default 26, clamped to 22–60) whose top
line is `studios <n>` — one **block per studio**, in document order, drawn as a rounded frame
(`╭╮╰╯`).

- The studio id sits in the top border; its open-attention count sits at the right end of that
  border as `!<n>` in the blocked colour, only when above zero. The current studio's frame is the
  accent colour, the others a muted surface colour.
- Inside a block: the studio's `error` (blocked colour) and `next` (`→ deliver t1`) lines when
  present, then one section per role group — producer, director, writer, actors — divided by
  `├──┤` rules. Empty groups are skipped.
- A person is one line: mark, name (with an actor's task and the priority flag), and the state
  word right-aligned (`working`, `idle`, `blocked`, `lost`, `closed`). An open attention adds one
  line `! <attention>` under the person. No model, no runner, no `doing` line, no group header
  rows — those live in the sidebar's cast.
- **Stacking:** when the blocks do not all fit the panel's height, every block but the current
  one shows as a single line, `╶ <studio id>` followed by its mark counts (`⠿2 ○3 ×1`, each in
  its mark's colour) and `!<n>`. When even that does not fit, the body scrolls as before. `Space`
  on any row of a block collapses or expands that block by hand (client-local).
- **Shortcuts line:** the last row of the panel, dim. Unfocused: the four studio keys with their
  configured labels (`alt ↑↓ studio  ←→ role`). Focused: the panel's own keys (`↑↓ move  ⏎ open
  +/- priority  esc`). Dropped when the panel is under 8 rows tall.
- The collapsed 16-column strip: one line per studio, its id cut to fit, the working count and
  `!<n>`.

## The cast

In studio mode the sidebar's agents section is the current studio's cast instead (see
[guide.md](guide.md)). Header: `cast · <studio id>` on the left, `<n>/<total>` on the right — the
studio's position in document order. One entry per person, in the summary's order, two rows each:
mark, name and task with the runner right-aligned; then the terminal's title — falling back to
`doing`, then the state word — with the model right-aligned in short form. An attention turns row
two into `! <attention>` in the blocked colour; a failed turn says `last turn failed`. Either takes
the whole row, model included, so it stays readable at the default width. The person
whose terminal is focused gets the active-row background on both rows and an accent `▌` in the
first column. Closed people stay listed, dimmed, after the live ones. At the bottom one row
`others <n> ▸` expands in place to herdr's own agent rows — the agents in no studio (the stage
terminal is never listed). Enter or a click toggles it; collapsed by default.

## Mouse

- Click on the title flips collapsed and expanded.
- Click on a block's top border makes that studio current (as `Alt+Down` would).
- Click on a single-line (stacked) block expands it.
- Click on a person focuses them; a click on a row selects it and acts like `Enter`.
- The wheel scrolls the body; the scrollbar clicks and drags.
- A click outside a focused panel hands focus back to the panes.

## Config

`$XDG_CONFIG_HOME/backlot/config.toml` (default `~/.config/backlot/config.toml`):

```toml
[sidebar]
width = 26   # cast sidebar width in studio mode, clamped to 20-60

[panel]
width = 26   # expanded column width, clamped to 22-60
key = "y"    # the chord tail after the prefix

[keys]
previous_studio = "alt+up"
next_studio = "alt+down"
previous_role = "alt+left"
next_role = "alt+right"
```

A missing file means defaults. A malformed file means defaults plus one notice carrying only
the line and column — file contents never reach the screen. A `key` that does not parse falls
back to `y` with one notice naming the file; an unparseable studio key falls back to its default
the same way — the invalid value is never echoed. An empty string unbinds a studio key. Outside
studio mode the sidebar follows herdr's `ui.sidebar_width`, as before; a drag on the edge still
resizes it for the session. herdr's own `config.toml` is read exactly as before, except for its
`[theme]`: the same file holds backlot's own ([Theme](../theme.md)).

## Behaviour notes

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
- Entering or leaving studio mode may resize the panes once — the sidebar's width switches
  between herdr's configured width and backlot's `[sidebar] width` unless the user dragged it.
