# Manual Verification Checklist

## Base
- `cargo run -- test.txt` starts and shows editor UI.

## Buffers
- `:e a.txt`, `:e b.txt`, then `:ls` shows 2 buffers and status shows `buf:x/y`.
- `:b <id>` switches and restores cursor/scroll.
- `:bn` / `:bp` cycles buffers.
- Normal mode `]b` / `[b` cycles buffers.
- `:bd` closes current; dirty buffer warns; `:bd!` forces close.

## Command History
- Run a few commands (e.g. `:set rnu`, `:set nornu`, `:w`).
- In command mode, ↑/↓ navigates history.
- Typing or backspace clears history index.
- Command/search line editing: `Alt`/`Ctrl`+`Backspace` deletes a word, `Alt`/`Ctrl`+`←/→` moves by word (word boundaries treat `/` as a separator), `Cmd+Backspace` or `Cmd+←/→` go to line start/end (when your terminal sends those keys). Terminal paste (bracketed paste) inserts into the prompt.

## Command Completion
- `:` then `Tab/Shift-Tab` cycles command names.
- `:b<Tab>` / `:w<Tab>` narrows matches.
- Popover shows candidates and highlights current.
- `:map` shows keymaps in a popover; press Esc to close.
- `:perf` shows recent render average (enable with `RVIM_PERF=1`).
- `:perf detail` shows min/avg/max.
- `:perf reset` clears samples.

## Path Completion
- `:e<Tab>` cycles current dir entries.
- If there is a single match, `Tab` completes it without showing the popover; pressing `Tab` again shows the list for the completed directory (when applicable).
- `:e src/<Tab>` cycles entries in `src/`.
- `:e ..<Tab>` / `:e ./<Tab>` complete to `../` / `./`.
- `:e ~/` or `:e "~/My Docs/` completes (spaces supported).
- Directory candidates display as `name/` and popover anchor is stable.
- When the popover is open and a directory is selected, press `/` to enter it and list its contents.

## Keymap
- Add a `[keymap.command]` section in `rvim.toml`, restart, and verify the mapped key triggers the expected action.
- Enable `keymap_debug = true` to log key events to `~/.config/rvim/rvim.log`.
- Use `noop` to disable a default mapping (e.g. `"]b" = "noop"`).

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

## Syntax Highlighting (Tree-sitter)
- Open `src/main.rs` and verify identifiers/types are colored.
- If highlighting is missing, ensure `queries/rust/highlights.scm` exists.
- Try other languages (js/ts/tsx/json/toml/md/html/css/py/lua/go) and verify colors appear.
