# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

**guiguitsu** is a desktop GUI application for visualizing git stacks (groups of commits per branch). It is built with Rust and [Slint](https://slint.dev/) — a declarative UI toolkit where the UI is defined in `.slint` files and the logic is in Rust.

## Commands

```bash
# Build and run
cargo run

# Build only
cargo build

# Run all tests
cargo test

# Run a single test by name
cargo test <test_name>
# e.g.: cargo test parent_shas_returns_two_parents_for_merge_commit
```

Tests in `src/git_utils.rs` spin up real git repos via `tests/repos/repo1.sh` (a bash script that creates a temp repo with a merge commit). They require `git` and `bash` on `$PATH`.

## Architecture

### Data flow

```
git_utils.rs         — raw git operations (runs git CLI, parses output)
stacks.rs            — StackInfo / StackProvider trait; DummyStackProvider for dev
models.rs            — converts Vec<StackInfo> → Slint ModelRc<Stack> (bridges Rust ↔ Slint)
main.rs              — wires provider → model → App widget → run()
```

### UI layer (Slint)

`build.rs` compiles `ui/app.slint` at build time via `slint_build::compile`. The macro `slint::include_modules!()` in `main.rs` brings all exported Slint types (`App`, `Stack`, `StackCommit`) into Rust scope.

```
ui/types.slint                  — StackCommit and Stack structs (shared Slint types)
ui/app.slint                    — root App window; exposes `in property <[Stack]> stacks`
ui/components/stacks_view.slint — horizontal scroll of stack columns, each listing CommitCards
ui/components/commit_card.slint — single commit display (description, change-id, author, timestamp)
ui/components/detail_panel.slint
ui/components/bottom_bar.slint
```

### Key type mapping

| Slint (`.slint`) | Rust (`models.rs`) |
|---|---|
| `StackCommit` | `git_utils::CommitInfo` |
| `Stack` | `stacks::StackInfo` |
| `[Stack]` model | `ModelRc<Stack>` via `Rc<VecModel<Stack>>` |

Slint property names use hyphens (`change-id`) but are accessed from Rust with underscores (`change_id`).

### Extending data sources

Implement the `StackProvider` trait in `stacks.rs` to replace `DummyStackProvider` with real git data. The `git_utils` module already has `commits_in_range` and `parent_shas` helpers for reading from a real repo.

## Slint-specific notes

- The agent guide at `docs/slint-coder.agent.md` (also at `.github/agents/slint-coder.agent.md`) is a comprehensive Slint language and Rust API reference — read it when working on UI.
- All components intended to be used from Rust must be `export`ed.
- Use `root.` for the current component's properties, `self.` for the current element's own properties.
- `SharedString` is Slint's string type; convert from Rust strings with `.into()` or `SharedString::from(...)`.
