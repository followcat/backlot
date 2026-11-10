# Keys

backlot uses Herdr's key model: press a prefix, then an action key. The default prefix is `Ctrl+B`. If you have a Herdr
config (`~/.config/herdr/config.toml`), backlot uses its prefix and bindings.

Press `Ctrl+B ?` for the full list, which you can search. It always matches your config.

![The key bindings overlay](images/keybinds.png)

The defaults you'll use most:

| Keys | Action |
|---|---|
| `Ctrl+B q` | Detach (`hc` keeps running) |
| `Ctrl+B w` | Navigate workspaces |
| `Ctrl+B b` | Show or hide the sidebar |
| `Ctrl+B ?` | Search the key bindings |
| `Ctrl+B s` | Settings |
| `Ctrl+B c` | New tab |
| `Ctrl+B v`, `Ctrl+B -` | Split right, split down |
| `Ctrl+B h/j/k/l` | Move focus |
| `Ctrl+B z` | Zoom the pane |
| `Ctrl+B r` | Resize mode |
| `Ctrl+B x` | Close the pane |
| `Ctrl+B y` | Toggle the studio panel |
| `Ctrl+G` | Lock the keyboard, so every key goes to the program. Press again to unlock. |

`Ctrl+B Ctrl+B` sends a literal `Ctrl+B` to the program. Custom commands bound in your Herdr config work here too.

Settings changed with `Ctrl+B s` (theme, status indicators, sound, toast delivery) are saved to your Herdr config.
Nothing else in that file changes. [Runtime](runtime.md) covers the prefix, lock and navigation in more detail.

## The studio panel

`Ctrl+B y` — the prefix, then `y` — opens the studio panel. Pressing the chord again while it is
focused closes it; while it is open but unfocused the chord focuses it. A Herdr binding on `y`
still wins whenever the panel does not own the keys. The chord's tail is `panel.key` in
`~/.config/backlot/config.toml`.

The panel has three modes: closed (the default — zero width, no keys), a 16-column collapsed
strip, and an expanded column (`[panel] width`, default 30, clamped to 24–60). While it is
focused it owns these keys:

| Keys | Action |
|---|---|
| `j`/`k`, arrows | Move the selection |
| `PageUp`/`PageDown` | Scroll |
| `Enter` | Act on the row (focus a terminal, collapse a group, reopen a closed person) |
| `Space` | Collapse the selected group |
| `c` | Switch collapsed and expanded |
| `+`/`-` | Raise or lower a task's priority |
| `Esc` | Unfocus the panel (close it when it covers the whole frame) |

[docs/studio/guide.md](studio/guide.md) explains what the panel's marks mean.

## Scrolling a pane

`PageUp` and `PageDown` page the focused pane's scrollback by one pane height when the program does
not use them itself: a shell or a line editor on the normal screen, like a prompt that takes pasted
text. A program on the alternate screen, one reporting the mouse, or a pager in application cursor
mode gets the keys instead, so `less`, `vim` and similar apps keep their own paging. Modified page
keys such as `Ctrl+PageUp` always reach the program, and any key the program does get jumps the view
back to the bottom. The mouse wheel scrolls the pane the same way.

Inside a floating terminal (`type = "popup"`), every key goes to the popup's program, so `PageUp` and
`PageDown` do not page the pane behind it.

## Copy mode

`Ctrl+B [` enters copy mode on the focused pane. The program gets none of these keys; the copy cursor
walks the visible screen and the scrollback above it, and `y` copies the selection through OSC 52.

| Keys | Action |
|---|---|
| `h` `j` `k` `l` or arrows | Move the copy cursor |
| `w` `b` `e` | Next word start, previous word start, word end |
| `W` `B` `E` | Same, on whitespace-delimited big words |
| `{` `}` | Previous or next paragraph (blank line) |
| `0` `Home` | Line start |
| `^` `$` `End` | First non-blank cell, line end |
| `g` `G` | Top of history, bottom of the screen |
| `Ctrl+U` `Ctrl+D` | Half page up, half page down |
| `PageUp` `PageDown` | Full page up, full page down |
| `/`, `?` | Search forward or backward; `Enter` runs it, `Esc` cancels |
| `n` `N` | Repeat the search forward or backward |
| `v` or `Space` | Start a selection; `V` selects whole lines |
| `y` or `Enter` | Copy the selection and leave copy mode |
| `Esc` | Clear the selection or search; press again to leave |
| `q` | Leave copy mode |

Rebind the entry key with `keys.copy_mode` in your Herdr config, for example `copy_mode = "prefix+escape"`.

The prefix still arms inside copy mode (unless a search prompt is open): press it, then a normal
binding key, or `Esc` to cancel the armed prefix. Because the prefix key is claimed first, the
copy-mode `Ctrl+B`/`Ctrl+F` page keys only fire when you have bound a different prefix.

Entering copy mode drops any mouse selection in progress.

Inside a popup terminal there is no copy mode: drag with the mouse to select over the popup's visible
frame and the selection copies on release, through the same OSC 52 path as pane selection.

## Alt shortcuts

None of these are bound by default. Add the ones you want to `~/.config/herdr/config.toml`, then press `Ctrl+B Shift+R`
to reload it.

### Switch tabs and agents

```toml
[keys]
previous_tab = "alt+left"
next_tab = "alt+right"
previous_agent = "alt+up"
next_agent = "alt+down"
```

| Keys | Action |
|---|---|
| `Alt+Left`, `Alt+Right` | Previous or next tab in the current workspace. Wraps around. |
| `Alt+Up`, `Alt+Down` | Previous or next entry in the sidebar's agents list. Wraps around. |

![Switching tabs and agents with Alt and the arrow keys](images/alt-arrows.gif)

`Alt+Up` and `Alt+Down` follow the agents list, so one press can land in another pane, tab, workspace or machine.
Machines that are offline are skipped. Plain shells aren't in the list: a program counts as an agent only while it has
a status.

### Floating terminal

A `[[keys.command]]` entry binds a key to a command. With `type = "popup"`, the command runs in a floating terminal over
the current tab. This one opens your shell on `Alt+F`:

```toml
[[keys.command]]
key = "alt+f"
type = "popup"
command = "exec \"${SHELL:-sh}\""
description = "open floating terminal"
width = "80%"
height = "80%"
```

![A floating shell opened with Alt+F](images/alt-f.gif)

`width` and `height` take a percentage or a number of cells. While the popup is open, every key goes to it, including
`Esc`. It closes when its program exits, so type `exit` or press `Ctrl+D`. Leaving its tab also closes it and ends its
program.

## Pasting text and images

`Ctrl+Shift+V` pastes text. That is your terminal's own shortcut, and the program receives it as a bracketed paste,
unchanged. While a popup is open the paste goes to the popup's program, wrapped only when that program asked for
bracketed paste, just like a pane.

On a remote endpoint `Ctrl+V` pastes a clipboard image instead. backlot reads your local clipboard, uploads the image
to the program's machine with `terminal.put`, and the program receives the path of the uploaded file. This applies to
`backlot --remote`, to backlot running inside an SSH session, and to a focused terminal on another machine
(`machine/tN`). On a plain local endpoint `Ctrl+V` goes to the program unchanged. Local programs read the clipboard
themselves, and Vim needs it for block-visual.

The trigger is `keys.remote_image_paste` in `~/.config/herdr/config.toml`, default `"ctrl+v"`. Set it to `""` to
disable the key. Two other gestures do the same thing wherever it applies: dropping an image file on the window, and
an empty bracketed paste. An empty paste is what a terminal sends for `Ctrl+Shift+V` when the clipboard holds only an
image. PNG, JPEG, GIF, WebP and BMP are recognized, up to the 8 MB `terminal.put` limit. On Linux the read needs
`wl-paste` under Wayland or `xclip` under X11.

A copied image file works too. A file manager puts `x-special/gnome-copied-files` or `text/uri-list` on the
clipboard, and some tools put one absolute image path in `text/plain`. backlot resolves the local file and uploads its
bytes, the same way as a drop.

A paste that cannot work never fails silently. When the clipboard holds no image and no readable image file, `Ctrl+V`
still reaches the program, and a notice says so. When nothing can read the clipboard at all (no display, no tool, or a
clipboard owner that stopped answering), the notice says why.
