# rvim

Small, fast terminal-first editor inspired by Vim, implemented in Rust.

## Status

This project is actively evolving. See `BLUEPRINT.md` for the roadmap and
`COMMANDS.md` for supported commands and options.

## Quick start

```bash
cargo run -- path/to/file
```

## Syntax highlighting

Tree-sitter based highlighting is enabled for:

- Rust, JavaScript, TypeScript, TSX, JSON, TOML, Markdown, HTML, CSS, Python, Lua, Go

Queries are loaded from `queries/<lang>/highlights.scm` first, falling back to built-ins
when available.

## Recent changes (last 5 hours)

- Added Tree-sitter syntax highlighting (Rust + multi-language queries).
- Updated Tree-sitter core/grammar versions and fixed initialization quirks.
- Improved TypeScript/TSX query compatibility.
- Added tilde expansion for `:e` paths.

To add or override a language, place a `highlights.scm` file under:

```
queries/<lang>/highlights.scm
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
syntax_keyword = "#569CD6"
syntax_string = "#6A9955"
syntax_comment = "#6A9955"
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
