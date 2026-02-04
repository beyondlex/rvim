# Session Notes (2026-02-04)

Quick index for next session.

## Key docs
- `README.md` — overview, config, themes
- `COMMANDS.md` — command/normal/search features
- `BLUEPRINT.md` — roadmap

## Recent features
- Dot repeat (`.`) for edits, including text objects; `dd` fixed; `Esc` cancels pending ops.
- Search: `/` and `?` prompt, `n/N` repeat, highlight, search history (↑/↓), `Esc` clears highlight.
- Line numbers + current line highlight; relative numbers via `:set relativenumber`.
- Counts for motions: `h/j/k/l`, `w/b/e`, `W/B/E`, `G`, `gg` (e.g. `11gg`, `5j`).
- `%` jump: supports `()[]{}` and `<>`, finds next bracket if not on one.
- Visual case change: `u`/`U`/`~`; Normal `~` toggles current char and moves right.

## Themes/config
- Theme presets: `light`, `dark`, `solarized`.
- Config search order: `rvim.toml`, `.rvim.toml`, `~/.config/rvim/config.toml`.
- Theme overrides per preset:
  ```toml
  theme = "dark"
  [themes.dark]
  current_line_bg = "#464646"
  ```
- Runtime switch: `:set theme=dark` (applies overrides), `:set theme?`.
- `Tab` in command mode cycles `:set` options and `theme=` values.

## Files to know
- `src/ui.rs` — rendering, line numbers, highlight, status bar.
- `src/app/command.rs` — `:set` options, theme switch.
- `src/app/theme.rs` — theme presets.
- `src/app/config.rs` — config + overrides parsing.

## Open ideas
- Extend counts to more motions (optional).
- Add theme config for more UI parts (if needed).
