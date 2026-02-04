# rvim

Small, fast terminal-first editor inspired by Vim, implemented in Rust.

## Status

This project is actively evolving. See `BLUEPRINT.md` for the roadmap and
`COMMANDS.md` for supported commands and options.

## Quick start

```bash
cargo run -- path/to/file
```

## Config

Configuration is loaded from the first existing file in this order:

1. `rvim.toml`
2. `.rvim.toml`
3. `~/.config/rvim/config.toml`

Example (`rvim.toml`):

```toml
theme = "dark"

[themes.dark]
current_line_bg = "#464646"
```

## Logs

Panics and runtime errors (e.g. save failures) are appended to:

```
~/.config/rvim/rvim.log
```

## Themes

Built-in themes: `light`, `dark`, `solarized`.

You can switch at runtime:

```
:set theme=dark
```

## License

MIT
