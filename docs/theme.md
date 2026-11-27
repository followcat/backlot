# Theme

backlot has a look of its own, so you can tell it from hc-tui and Herdr at a glance: the built-in `backlot` theme
(*Babylon* Night: black lacquer, rose velvet, and brass with warm ivory text), paired with a matching light theme
(`backlot-day`). `backlot` is the default. The previous marquee gold on midnight blue look remains selectable as
`backlot-midnight`.

Every bundled RGB theme in studio mode (`!plain` and without `[theme] source = "herdr"`) is opaque across all four chrome frames: its left sidebar, top studio header, Lot dock, and Call Sheet share the same `elevation.surface` separated by one-column `border_muted` rules (unless `[theme.custom] sidebar_bg` overrides the sidebar, or the 16-color `terminal` theme falls back to terminal default), while the center terminal pane keeps `Color::Reset` and modal overlays fill default-background cells with `elevation.overlay`. A translucent terminal therefore never shows through the studio chrome, whereas `backlot --plain` and `[theme] source = "herdr"` keep Herdr's translucent sidebar (`Color::Reset`) and `panel_bg` top bar unchanged.

The theme is read from backlot's own config, `~/.config/backlot/config.toml` (`$XDG_CONFIG_HOME/backlot/config.toml`
when that is set), not from your Herdr config. With no `[theme]` table there, or no file, you get `backlot`.

```toml
[theme]
name = "backlot"          # or backlot-day, backlot-midnight, or any built-in theme: catppuccin, nord, dracula, …

[theme.custom]            # optional: change single colours on top of the theme
accent = "#f2b84b"
```

`[theme]` takes the same keys as Herdr's: `name`, `auto_switch` with `dark_name` and `light_name`, and `[theme.custom]`
with its `dark` and `light` tables. `name` defaults to `backlot`, and `babylon` / `babylon-day` are accepted as aliases
for `backlot` / `backlot-day`. When `auto_switch = true`, `backlot` switches between `backlot` (dark) and `backlot-day`
(light); `backlot-midnight` has no light sibling, so `auto_switch` keeps `backlot-midnight` on both appearances unless
you set `light_name` explicitly.

## Picking a theme

`Ctrl+B s` opens Settings on the theme list, with `backlot`, `backlot-day`, and `backlot-midnight` first. Moving
through the list previews each theme; Enter saves the one you picked to backlot's config and `Esc` puts the old one
back. Your Herdr config is not written.

After editing the file by hand, `Ctrl+B Shift+R` reloads it.

## Using your Herdr theme instead

```toml
[theme]
source = "herdr"
```

backlot then reads `[theme]` from your Herdr config exactly as hc-tui does (including the old `ui.accent`), the
Settings list is Herdr's, and a theme picked there is saved to your Herdr config. The other keys of backlot's `[theme]`
are ignored while `source = "herdr"` is set.

## When something is wrong

- An unknown theme name falls back to `catppuccin` and says so in the notice line, with the list of valid names.
- A `[theme]` table that cannot be read (a number where a name belongs, for example) gives the `backlot` theme and one
  notice. A `source` other than `"backlot"` or `"herdr"` is treated as `"backlot"`, with a notice.

## Modal popups and elevation

All bundled RGB themes derive tonal surface, elevated modal overlay, dimmed backdrop, drop-shadow, and subdued divider tokens from their own palette colours using relative luminance and CIE lightness — nothing is hard-coded per theme name, and light themes invert cleanly. Floating popups (`peek`, `Alt+F`, ephemeral popups), dialogs, wizards, navigator, help, settings, Task Sheet, and the Reader share dimmed backdrops, 1-cell outer gutters, rounded borders, internal breathing padding, and 1-cell drop shadows (context menus keep rounded borders, padding, and drop shadows without dimming the full screen); the docked Call Sheet sits directly over the top edge ending at its `▀▀` perforation row without an external drop shadow. Inside floating VT popups, non-`backlot` Herdr themes still preserve terminal-default translucency. To turn either depth layer off globally, set `[ui] popup_backdrop = "none"` or `[ui] popup_shadow = false` in `~/.config/herdr/config.toml`.
