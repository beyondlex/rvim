# rvim Commands & Options

This document summarizes the currently supported normal/command mode features.

## Command mode

Enter command mode with `:` then press Enter to execute.

### File commands

- `:w` / `:write` — save (use `:w <path>` if no file name)
- `:q` / `:quit` — quit (fails if dirty)
- `:q!` / `:quit!` — quit without saving
- `:wq` / `:x` — save and quit (use `:wq <path>` if no file name)
- `:e <path>` / `:edit <path>` — open file

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
