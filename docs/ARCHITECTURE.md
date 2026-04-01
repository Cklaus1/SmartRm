# SmartRM Architecture

Internal architecture reference for contributors.

## Module Structure

```
src/
  main.rs              CLI entry point and command dispatch
  lib.rs               Library root (re-exports all modules)
  cli.rs               Clap argument definitions and subcommands
  id.rs                ULID generation for archive IDs
  error.rs             Error types and Result alias

  commands/            Thin command handlers (parse args, call operations, format output)
    delete.rs          Default delete (archive) command
    undo.rs            Undo last N batches
    restore.rs         Restore by ID, batch, --last, or --all
    list.rs            List archived objects with filtering
    search.rs          Search by glob/substring with date/size/dir filters
    history.rs         Version history for a specific file path
    timeline.rs        Chronological batch history
    cleanup.rs         Remove old archives by age or expiry
    purge.rs           Permanently delete archives
    stats.rs           Archive statistics
    config.rs          View/modify configuration
    explain.rs         Policy trace for an archive or path

  operations/          Core business logic (stateful, transactional)
    delete.rs          Archive operation with per-file rollback
    restore.rs         Restore with conflict resolution
    cleanup.rs         Cleanup logic with TTL and age filtering
    purge.rs           Permanent deletion of archive data

  db/                  Database layer
    schema.rs          DDL (10 tables), schema versioning, migrations
    operations.rs      Insert/update operations (batches, archive objects, etc.)
    queries.rs         Read queries (list, search, history, timeline, stats)

  models/              Domain types
    archive_object.rs  ArchiveObject, LifecycleState, ObjectType enums
    batch.rs           Batch, BatchStatus, OperationType
    batch_item.rs      BatchItem, per-file status within a batch
    policy.rs          Classification, DangerLevel, Tag, SourceType, EffectivePolicy
    restore_event.rs   RestoreEvent, ConflictPolicy, RestoreMode

  fs/                  Filesystem abstraction
    mod.rs             Filesystem trait + RealFilesystem impl
    archive.rs         Move/copy files into archive directory
    restore.rs         Move/copy files back from archive
    metadata.rs        Preserve/restore permissions, timestamps, ownership
    hashing.rs         SHA-256 content hashing
    disk_space.rs      statvfs wrapper for free space checks

  policy/              Policy engine
    config.rs          SmartrmConfig struct, load/save, path resolution
    classifier.rs      File classification (tags + danger level)
    resolver.rs        Policy resolution (flags + config + classification -> ResolvedPolicy)

  gate/                Destructive command gate
    mod.rs             GateEnvironment trait, GateTier, check_gate()
    tty.rs             Real TTY detection and prompting
    auth.rs            Confirmation phrase generation and verification
    agent_detection.rs Agent/CI environment detection
    audit.rs           Audit log writes to destructive_audit_log table
    cooldown.rs        Rate limiting for failed attempts
    scope_preview.rs   Human-readable scope summary

  output/              Output formatting
    human.rs           Table and text formatting for terminal
    mod.rs             JSON vs human output dispatch
```

## Key Design Patterns

### Filesystem Trait

All file I/O goes through the `Filesystem` trait (`src/fs/mod.rs`). `RealFilesystem` is the production implementation. Tests can inject a mock that records calls without touching disk.

```rust
pub trait Filesystem {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<u64>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    // ... 10 more methods
}
```

### GateEnvironment Trait

The destructive command gate abstracts TTY interaction and environment variable reads behind `GateEnvironment`. Tests provide a mock that returns predetermined values for `is_stdin_tty()`, `read_line_from_tty()`, and `get_env()`.

### Thin Commands + Operations

Command modules (`src/commands/`) are thin: parse arguments, call an operation, format output. Business logic lives in `src/operations/`. This keeps command handlers testable without database or filesystem setup.

### Per-File Transactions

Delete operations process files individually within a batch. If archiving file N fails, files 1..N-1 that were already archived are rolled back (compensating action: move them back from archive to original location). The batch is marked `partial` or `failed` accordingly.

### ULID Primary Keys

All database primary keys are ULIDs (Universally Unique Lexicographically Sortable Identifiers). They encode creation time, sort chronologically, and work as short prefixes for CLI lookups (users type the first 8+ characters).

## Database Schema

10 tables in SQLite (WAL mode, foreign keys enabled):

| Table | Purpose |
|-------|---------|
| `schema_version` | Single-row version tracking for migrations |
| `batches` | Groups of related operations (one `smartrm` invocation = one batch) |
| `archive_objects` | Core table: one row per archived file/dir/symlink with full metadata |
| `batch_items` | Per-input-path tracking within a batch (maps CLI args to archive objects) |
| `restore_events` | Records of restore operations with conflict resolution details |
| `mounts` | Filesystem mount points for cross-device handling |
| `policies` | Stored policy rules (global, user, path, project, mount scopes) |
| `effective_policies` | Audit trail: which policy setting applied to each archive decision |
| `hash_jobs` | Background SHA-256 hashing queue for large files |
| `destructive_audit_log` | Every destructive command attempt (allowed or denied) with context |

Key indexes cover: `archive_objects.original_path`, `archive_objects.state`, `archive_objects.created_at`, `archive_objects.content_hash`, `batches.started_at`, `batch_items.input_path`.

## State Machine

Archive objects follow this lifecycle:

```
            smartrm (delete)
  ACTIVE  ------------------>  ARCHIVED
                                  |
                  restore         |  TTL expiry / cleanup
                <---------        |
                                  v
                               EXPIRED
                                  |
                  restore         |  purge
                <---------        |
                                  v
                               PURGED (terminal, data deleted)

  Any state can also be:
    FAILED  -- operation error, compensating rollback attempted
```

Valid transitions:
- `archived -> restored` (restore command)
- `archived -> expired` (TTL or cleanup --older-than)
- `archived -> purged` (purge command)
- `expired -> restored` (restore before purge)
- `expired -> purged` (purge or cleanup --expired)

## Policy Precedence

Policy settings are resolved with 7 source types, highest priority first:

| Priority | Source Type | Example |
|----------|-------------|---------|
| 1 | `hard_safety` | Block deletion of `/` |
| 2 | `cli` | `--permanent` flag |
| 3 | `interactive` | User answered prompt |
| 4 | `user_rule` | Per-user config rule |
| 5 | `project_rule` | Project-level `.smartrm.json` |
| 6 | `system_rule` | `/etc/smartrm/config.json` |
| 7 | `default` | Built-in defaults |

Additionally, the `learned` source type captures patterns from user behavior for future policy suggestions.

The `effective_policies` table records which source won for each setting on every operation, enabling `smartrm explain` to show exactly why a decision was made.

## File Classification

The classifier (`src/policy/classifier.rs`) assigns tags and a danger level to each path:

**Tags** (non-exclusive, a file can have multiple):
- `build` -- node_modules, dist, target, __pycache__, .o/.pyc files
- `temp` -- .tmp, .swp, .bak, .log, ~ files, # files
- `content` -- source code, documents, configs (40+ extensions)
- `protected` -- .env, .pem, .key, credential/secret/token files

**Danger levels** (exclusive):
- `safe` -- normal operation
- `warning` -- requires `--yes-i-am-sure` (home dir, .git, .ssh, system paths)
- `blocked` -- hard block, cannot proceed (`/`)

## Test Strategy

- **Unit tests**: Co-located with source in `#[cfg(test)]` modules. Cover classification, policy resolution, config loading, schema initialization, gate logic.
- **Integration tests**: In `tests/` directory. Use `tempfile` crate for isolated filesystem operations. Test full delete-restore-cleanup cycles.
- **Trait-based mocking**: `Filesystem` and `GateEnvironment` traits allow testing business logic without real I/O or TTY interaction.
- **In-memory database**: `db::open_memory_database()` provides a fresh SQLite instance for each test.
