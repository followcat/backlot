# Theme

backlot has a look of its own, so you can tell it from hc-tui and Herdr at a glance: the built-in `backlot` theme,
marquee gold on midnight blue with cream text. It is the default.

The backlot theme is opaque: its sidebar is a step darker than the panes with a one-column rule where it meets
them, and its overlays — popups, the wizard, notice and release cards, the Hollywood layer — fill their
default-background cells with the panel colour. A translucent terminal therefore never shows the window behind
the UI. With `[theme] source = "herdr"` (or any built-in Herdr theme) the reference look is unchanged, translucency
included.

The theme is read from backlot's own config, `~/.config/backlot/config.toml` (`$XDG_CONFIG_HOME/backlot/config.toml`
when that is set), not from your Herdr config. With no `[theme]` table there, or no file, you get `backlot`.

```toml
[theme]
name = "backlot"          # or any built-in theme: catppuccin, nord, dracula, gruvbox, tokyo-night, …

[theme.custom]            # optional: change single colours on top of the theme
accent = "#f0b24a"
```

`[theme]` takes the same keys as Herdr's: `name`, `auto_switch` with `dark_name` and `light_name`, and `[theme.custom]`
with its `dark` and `light` tables. `name` defaults to `backlot`.

## Picking a theme

`Ctrl+B s` opens Settings on the theme list, with `backlot` first. Moving through the list previews each theme; Enter
saves the one you picked to backlot's config and `Esc` puts the old one back. Your Herdr config is not written.

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
