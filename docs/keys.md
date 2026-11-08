# Keys

hc-tui uses Herdr's key model: a prefix key, then an action key. The default prefix is
`Ctrl+B`; your existing Herdr configuration (`~/.config/herdr/config.toml`) supplies the
prefix and every binding — hc-tui reads it, it never writes it.

The authoritative, always-current reference is inside the app: press `Ctrl+B ?` for the
searchable keybind help, or `hc-tui --help` for the default map. [runtime.md](runtime.md)
describes how the prefix, lock and navigation model behave.

Defaults in short form:

| Key | Action |
|---|---|
| `Ctrl+B q` | detach (hc keeps running) |
| `Ctrl+B w` | workspace navigation |
| `Ctrl+B b` | toggle sidebar |
| `Ctrl+B ?` | searchable keybind help |
| `Ctrl+B c` | new tab · `v` / `-` split · `h/j/k/l` focus · `z` zoom · `r` resize · `x` close |
| `Ctrl+G` | keyboard lock — every key passes to the terminal until pressed again |

`Ctrl+B Ctrl+B` sends a literal `Ctrl+B` to the program. Custom commands bound in your
Herdr config work as configured.
