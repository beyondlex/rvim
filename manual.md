# Manual Verification Checklist

## Base
- `cargo run -- test.txt` starts and shows editor UI.

## Buffers
- `:e a.txt`, `:e b.txt`, then `:ls` shows 2 buffers and status shows `buf:x/y`.
- `:b <id>` switches and restores cursor/scroll.
- `:bn` / `:bp` cycles buffers.
- `:bd` closes current; dirty buffer warns; `:bd!` forces close.

## Command History
- Run a few commands (e.g. `:set rnu`, `:set nornu`, `:w`).
- In command mode, ↑/↓ navigates history.
- Typing or backspace clears history index.

## Command Completion
- `:` then `Tab/Shift-Tab` cycles command names.
- `:b<Tab>` / `:w<Tab>` narrows matches.
- Popover shows candidates and highlights current.

## Path Completion
- `:e<Tab>` cycles current dir entries.
- `:e src/<Tab>` cycles entries in `src/`.
- `:e ..<Tab>` / `:e ./<Tab>` complete to `../` / `./`.
- `:e ~/` or `:e "~/My Docs/` completes (spaces supported).
- Directory candidates display as `name/` and popover anchor is stable.

## :set Completion
- `:set <Tab>` shows options.
- `:set theme=<Tab>` cycles theme names (including overrides).
- `:set shiftwidth=<Tab>` cycles 2/4/8.
- `:set rnu<Tab>` / `:set findcross<Tab>` completes on/off/`?`.

## Popover UX
- Popover connects to command line without hiding cursor.
- Highlight stays centered while list scrolls when possible.
- Block scrollbar visible; bottom row not affected by status bar color.

## Wide Char/Column Indexing
- Cursor column uses screen width (tabs + wide chars).
- New helpers in `src/app/types.rs` map char/byte/screen columns.
