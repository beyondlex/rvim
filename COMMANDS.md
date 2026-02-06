# rvim Commands & Options

This document summarizes the currently supported normal/command mode features.

## Command mode

Enter command mode with `:` then press Enter to execute.

Command completion: in command mode, `Tab`/`Shift-Tab` cycles completion for commands and options.

Command-line editing (command and search prompts):
- `Alt+Backspace` or `Ctrl+Backspace` deletes previous word (word boundaries treat `/` as a separator).
- `Cmd+Backspace` deletes to line start (when the terminal sends it).
- `Alt+←/→` or `Ctrl+←/→` moves by word (word boundaries treat `/` as a separator).
- `Cmd+←/→` moves to line start/end (when the terminal sends it).
- Terminal paste (bracketed paste) inserts into the command/search prompt.

## Keymap

Keymaps are configured in `rvim.toml` under `[keymap.normal]`, `[keymap.insert]`,
`[keymap.visual]`, and `[keymap.command]`. The keys are key specs and the values are actions.

Key spec examples:
- `"x"` (literal character)
- `"]b"` (multi-key sequence)
- `"<C-x>"`, `"<M-x>"`, `"<D-x>"`, `"<S-x>"`
- `"<Left>"`, `"<Right>"`, `"<Backspace>"`, `"<Tab>"`

Supported actions:
- `buffer_next`, `buffer_prev`
- `noop`
- `left`, `right`, `up`, `down`
- `word_left`, `word_right`
- `line_start`, `line_end`
- `backspace`, `delete_word`, `delete_line_start`
- `enter`, `escape`, `tab`, `backtab`

Note: Actions are applied per mode; unsupported actions are ignored.
Set `keymap_debug = true` in `rvim.toml` to log key events to `~/.config/rvim/rvim.log`.
Example (plugin): register custom commands via `App::register_command_candidate("MyCmd")`.

### File commands

- `:w` / `:write` — save (use `:w <path>` if no file name)
- `:q` / `:quit` — quit (fails if dirty)
- `:q!` / `:quit!` — quit without saving
- `:wq` / `:x` — save and quit (use `:wq <path>` if no file name)
- `:e <path>` / `:edit <path>` — open file (new buffer if not already open; `Tab`/`Shift-Tab` completes paths, including `~/`, quoted paths, and `./`/`../`; `/` enters a selected directory in the completion popover)
- `:w <path>` / `:write <path>` — save to path (`Tab`/`Shift-Tab` completes paths; `/` enters a selected directory in the completion popover)

### Buffers

- `:ls` / `:buffers` — list buffers (id + dirty state)
- `:b <id>` / `:buffer <id>` — switch to buffer
- `:bn` / `:bnext` — next buffer
- `:bp` / `:bprev` — previous buffer
- Normal mode: `]b` next buffer, `[b` previous buffer
- `:bd [id]` / `:bdelete [id]` — close buffer (current if no id)
- `:bd! [id]` / `:bdelete! [id]` — force close dirty buffer
- `:map` — show current keymaps (popover list; press Esc to close)

### Options (`:set`)

Use `:set` to change editor options:

- `:set findcross` / `:set nofindcross` / `:set findcross?`
- `:set shiftwidth=4` / `:set shiftwidth?`
- `:set indentcolon` / `:set noindentcolon` / `:set indentcolon?`
- `:set relativenumber` / `:set norelativenumber` / `:set relativenumber?`
- `:set rnu` / `:set nornu` / `:set rnu?` (aliases)
- `:set theme=light|dark|solarized` / `:set theme?`

Tip: In command mode, `Tab` cycles through `:set` options. If the buffer is `:set theme=`,
`Tab` cycles `light → dark → solarized`.

## Search

- `/pattern` — forward search
- `?pattern` — backward search
- `n` / `N` — repeat last search (forward/backward)
- `Esc` in Normal mode clears search highlight

## Normal mode highlights

- `%` — jump to matching bracket. If not on a bracket, searches forward for the next bracket and jumps to its match.
- `gg` — go to top. With count: `11gg` jumps to line 11.
- `G` — go to bottom. With count: `42G` jumps to line 42.
- Counts work with motions: `h/j/k/l`, `w/b/e`, `W/B/E`.
- Visual case change: in Visual mode use `u` (lower), `U` (upper), `~` (toggle).
