# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                          # Build debug binary
cargo build --release                # Build release binary (4.4MB)
cargo test                           # Run all 293 tests (unit + integration)
cargo test -- test_name              # Run a single test by name
cargo test --test lifecycle_scenarios  # Run only lifecycle integration tests
cargo test --test db_error_tests       # Run only DB error path tests
cargo test -- --nocapture              # Show println output during tests

# The binary
cargo run -- file.txt                # Archive a file
cargo run -- -r dir/                 # Archive a directory
cargo run -- list                    # List archived files
cargo run -- undo                    # Restore last deletion
cargo run -- --help                  # Full CLI help
```

## Architecture

SmartRM is a file lifecycle system — `rm` replacement that archives instead of deletes. Single Rust binary, SQLite-backed, no runtime dependencies.

### Layer Separation

```
commands/     Parse CLI args → typed struct → call operations → format output
operations/   Business logic: policy → danger check → disk check → DB write → FS move
db/           SQLite via rusqlite. schema.rs (DDL), operations.rs (writes), queries.rs (reads)
fs/           All disk I/O behind Filesystem trait for testability
policy/       Config loading, file classification, policy resolution
gate/         Destructive command safety gate behind GateEnvironment trait
models/       Domain types with Serialize + as_str()/FromStr for DB round-trips
output/       HumanOutput trait + serde Serialize for --json on all commands
```

### Key Design Patterns

**Trait-based testability**: `Filesystem` trait (`fs/mod.rs`) abstracts all disk I/O — tests inject mocks that simulate EXDEV, disk full, permission errors. `GateEnvironment` trait (`gate/mod.rs`) abstracts TTY/env — tests inject mock TTY state.

**Per-file transactions with compensating rollback**: In `operations/delete.rs`, each file is: `BEGIN → insert archive_object + batch_item → FS move → COMMIT`. If FS move fails, `ROLLBACK`. If COMMIT fails after FS move, compensating action moves file back.

**Context objects**: Operations take a `XyzContext` struct (conn, fs, config, flags). Commands construct the context and call `execute_xyz()`. This keeps CLI parsing separate from business logic.

**Dual-mode output**: All result types derive `Serialize` (for `--json`) and implement `HumanOutput` trait (for human-readable). `output::print_output()` dispatches based on `--json` flag.

**ULID primary keys**: All IDs are lowercase ULIDs via `id::new_id()`. Time-sortable, displayed as 8-char prefix in human output via `id::short_id()`.

### State Machine

```
ACTIVE → ARCHIVED → EXPIRED → PURGED (terminal)
              ↓          ↓
           RESTORED    RESTORED
              ↓
           (new ARCHIVED on re-delete)

FAILED is terminal (archive attempt failed)
```

### Database

10 SQLite tables. WAL mode, FK enforcement, 5s busy timeout. Core tables: `batches` (operation groups), `archive_objects` (archived files), `batch_items` (per-CLI-arg tracking), `restore_events` (restore audit trail), `effective_policies` (policy decisions at batch level).

`db::open_memory_database()` for tests — same PRAGMAs as production.

### Policy Precedence (7 levels)

hard_safety > cli flags > interactive choices > user rules > project rules > system rules > defaults. Each decision recorded in `effective_policies` for `smartrm explain`.

### File Classification

`policy/classifier.rs` returns `Classification { tags: Vec<Tag>, danger_level: DangerLevel }` from one function. Tags: build/temp/content/protected. Danger: safe/warning(msg)/blocked(msg).

### Destructive Gate

`gate/check_gate()` enforces TTY-only, agent detection, scope preview, confirmation phrase (or passphrase). Three tiers: SimpleConfirm (y/N), Standard (phrase), Elevated (type "PURGE PROTECTED" + phrase). All attempts logged to `destructive_audit_log`.

## Adding a New Command

1. Add variant to `Command` enum in `cli.rs`
2. Create `commands/newcmd.rs` — thin glue with `run()` function
3. Create `operations/newcmd.rs` if it has business logic
4. Add queries to `db/queries.rs` if needed
5. Result struct: `#[derive(Debug, Clone, Serialize)]` + `impl HumanOutput`
6. Wire in `main.rs` match statement
7. Add to `commands/mod.rs` (and `operations/mod.rs` if applicable)

## Testing Patterns

- Unit tests: co-located in `#[cfg(test)] mod tests` within each source file
- Integration tests: `tests/lifecycle_scenarios.rs` (5 end-to-end cycles), `tests/db_error_tests.rs` (6 error paths)
- DB tests: use `db::open_memory_database()`
- FS tests: use `tempfile::TempDir` for isolated directories
- Gate tests: `MockGateEnvironment` in `gate/tests.rs` with queued input lines
- Config tests: set `SMARTRM_HOME` env var to tempdir to isolate config I/O
