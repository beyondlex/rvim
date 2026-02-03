# rvim Blueprint

Owner: Lex  
Last updated: 2026-02-03

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

## Roadmap

### Phase 1 — MVP Editor (current)
Goal: Open, edit, save, quit reliably.

Checklist:
- [x] Open file from CLI arg
- [x] Basic text editing (insert/delete/backspace/enter)
- [x] Scrollable viewport
- [x] Save (Ctrl+S)
- [x] Quit with dirty confirmation (Ctrl+Q)
- [ ] Basic cursor info + status messages polish
- [ ] Input edge cases (multi-byte chars, wide glyphs)

### Phase 2 — Modal Core
Goal: Minimal Vim-like feel.

Checklist:
- [ ] Command line (":" prompt)
- [ ] `:w`, `:q`, `:wq`, `:e`
- [ ] Motions: w, b, e, 0, $, gg, G
- [ ] Operators: d, y, c (with motions)
- [ ] Visual mode (char-wise)
- [ ] Registers (unnamed + simple named)
- [ ] Macro record/playback (single register)

### Phase 3 — Reliability & Performance
Goal: Scale to large files; improve correctness.

Checklist:
- [ ] Rope-based buffer (ropey)
- [ ] Undo/redo (command log)
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

### Phase 5 — IDE Features (optional)
Goal: Modern coding features.

Checklist:
- [ ] LSP client (tower-lsp)
- [ ] Syntax highlighting (tree-sitter)
- [ ] File explorer

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
