# rvim Blueprint

Owner: Lex  
Last updated: 2026-02-04

## Vision
Build a small, fast, terminal-first editor inspired by Vim, implemented in Rust.
Start with a minimal usable editor, then incrementally add modal editing and extensibility.

## Principles
- Keep startup time and input latency low.
- Prefer simple data models early; optimize only after correctness.
- Every milestone should be runnable and demoable.

## Current Status (MVP)
- Terminal UI (crossterm + ratatui)
- Basic buffer editing, file open/save, simple modes, scrolling
- Modal editing (Normal/Insert/Visual/Command)
- Basic motions, operators, and visual selection
- Undo/redo + bracketed paste
- Multi-buffer management + command completion UX

## Roadmap

### Phase 1 — MVP Editor (current)
Goal: Open, edit, save, quit reliably.

Checklist:
- [x] Open file from CLI arg
- [x] Basic text editing (insert/delete/backspace/enter)
- [x] Scrollable viewport
- [x] Save (Ctrl+S)
- [x] Quit with dirty confirmation (Ctrl+Q)
- [x] Basic cursor info + status messages polish
- [~] Input edge cases (multi-byte chars, wide glyphs) — partial coverage via screen-width helpers + tests

### Phase 2 — Modal Core
Goal: Minimal Vim-like feel.

Checklist:
- [x] Command line (":" prompt)
- [x] `:w`, `:q`, `:wq`, `:e`
- [x] Search prompt (`/` and `?`)
- [x] Repeat search (`n`/`N`)
- [x] Motions: w, b, e, 0, $, gg, G
- [x] Operators: d, y, c (with motions)
- [x] Visual mode (char-wise)
- [x] Visual mode (line/block)
- [x] Registers (unnamed + simple named)
- [x] Text objects (iw/aw, i{}/a{}, i()/a(), i[]/a[], i</a>, i"/i', it/at)
- [x] Command line history + completion (popover)
- [ ] Macro record/playback (single register)

### Phase 3 — Reliability & Performance
Goal: Scale to large files; improve correctness.

Checklist:
- [ ] Rope-based buffer (ropey)
- [x] Undo/redo (command log)
- [ ] Incremental re-rendering
- [ ] Crash-safe recovery (swap/backup)
- [ ] Tests for editor core (buffer, cursor, motions)

### Phase 4 — Extensibility
Goal: Make it hackable.

Checklist:
- [ ] Config file (TOML)
- [ ] Plugin API (Rust trait or WASM)
- [ ] Event hooks (on save, on open, on key)
- [ ] Theme support
- [ ] Plugin API: commands registration/query
- [ ] Plugin API: buffer list/query
- [ ] Plugin API: status hooks (status line + transient messages)

### Plugin System Design (Draft)

Goals:
- Stable, versioned API with backward compatibility expectations.
- Clear separation between host and plugin runtime.
- Safe defaults (sandboxed when possible).
- Extensible registry for commands, keymaps, and status hooks.

Core components:
- PluginHost: loads plugins, manages lifecycle, dispatches events.
- PluginApi: stable interface exposed to plugins (host functions for WASM/Lua).
- Registries: commands, keymaps, status providers.
- Event bus: on_open, on_save, on_buffer_enter, on_key, on_command.

Proposed API surface (host-facing, versioned):
- Query: list_buffers, current_buffer, get_buffer_text (range)
- Mutation: open_buffer, switch_buffer, apply_edit, set_status
- Registration: register_command, register_keymap, register_status
- Events: on_event(event, handler)

Runtime plan:
- Phase A: Rust trait-based plugins (internal) for rapid iteration.
- Phase B: WASM runtime (wasmtime/wasmer) with host functions.
- Optional: Lua runtime for Neovim-like plugin authoring.

Config discovery (rvim.toml):
- [plugins] enabled = ["path/to/plugin.wasm", "..."]
- [plugin_settings.<name>] for plugin options

Keymap/command precedence:
- plugin > user > builtin

Status hooks:
- Plugins can register status items with priority.
- Host composes status line items deterministically.

Versioning:
- PluginApi::version() returns "0.x".
- Host rejects incompatible plugin versions.

### Plugin System Readiness (Notes)

Is it the right time?
- Phase 3 still has reliability/perf gaps (rope buffer, incremental render, tests).
- Plugin system can start in parallel, but should avoid deep core coupling until Phase 3 stabilizes.

Recommended prerequisites before full plugin runtime:
- Stable buffer model (rope or equivalent) and well-tested cursor/edit primitives.
- Event model defined (open/save/buffer enter/key/command).
- Command and keymap registries solidified (already underway).
- Status message lifecycle clarified (transient vs persistent).

Suggested phased delivery (to resume quickly later):
1) Define Rust interfaces only (no runtime): PluginHost, PluginApi, Event types.
2) Add registries for plugin commands/keymaps/status with priority resolution.
3) Add `:plugins` and `:map` expansions for introspection.
4) Select runtime (WASM first recommended), define host functions.

Decision pointers:
- If Phase 3 is active, prioritize correctness and tests.
- Start plugin work once core edit/undo/redo/motions are stable.

### Phase 5 — IDE Features (optional)
Goal: Modern coding features.

Checklist:
- [ ] LSP client (tower-lsp)
- [x] Syntax highlighting (tree-sitter)
- [ ] File explorer
- [ ] Incremental parsing (tree.edit + parser.parse(old_tree)) for large files

## Implementation Notes (recent)
- Bracketed paste enabled for faster Insert-mode paste.
- Cursor style differs by mode (Insert = bar).
- Smart indent supports `{[(` and optional `:` rules; `:set shiftwidth=` and `:set indentcolon`.
- Codebase split into modules under `src/app/` plus `src/ui.rs`.
- Delete/change yanks into register (dw/ciw/diw/visual d/c/x).
- Multi-buffer commands: `:ls`, `:b`, `:bn`, `:bp`, `:bd`.
- Command completion: `Tab`/`Shift-Tab` for `:e`, `:w`, `:set`, and command names; popover list with scrollbar.

## Architecture Sketch

Modules:
- `app`: event loop, mode state machine
- `core`: buffer, cursor, undo/redo, motions
- `ui`: rendering, layout, status bar
- `io`: file loading/saving

Data flow:
1) Input event -> app state machine
2) app updates buffer/cursor/mode
3) ui renders viewport + status

## Near-term Implementation Plan

Week 1 (stability):
- Add `:w`, `:q`, `:wq`
- Add word/line motions
- Improve status bar + message timeout
- Add basic tests for buffer ops

Week 2 (modal basics):
- Operators (d/y/c) + motions
- Visual mode
- Simple registers

## Open Questions
- How far should Vim compatibility go? (limit to core motions)
- Config format: TOML vs Lua
- Plugin story: Rust-only vs WASM

## Reference
- `COMMANDS.md` — supported commands and options
