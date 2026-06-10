# SmartRM — File Lifecycle System

## 1. Vision

SmartRM is not a "better rm." It is a **File Lifecycle System** that treats deletion as a state transition, not destruction.

```
ACTIVE → ARCHIVED → EXPIRED → PURGED
```

Phase 1 ships an rm-compatible CLI for adoption. Phases 2-3 evolve into a system service with background intelligence — where deletion becomes observable, reversible, and adaptive.

**Product identity**: Phase 1 positions as "Safe rm" for adoption. The architecture quietly builds toward "Filesystem memory layer" — timeline, undo, retention, policy, prediction, agent guidance. That is the real product.

## 2. Core Insight

`rm` models deletion as an event: instant, irreversible, context-blind.

SmartRM models deletion as a **lifecycle**: files transition through states, accumulate metadata, and can be recalled — until retention policy expires them.

This reframe unlocks: undo, timeline, behavior learning, storage prediction, and agent-driven optimization.

## 3. Target Users

- Developers (CLI-heavy workflows)
- Founders / builders
- Data engineers
- Power users / sysadmins

## 4. Phased Roadmap

### Phase 1 — Adoption Layer (MVP)
rm-compatible CLI. Safe archive. Undo. Restore. The hook that gets users in.

### Phase 2 — System Layer
Background daemon. Async indexing + hashing. Lifecycle states. Batch tracking. Timeline.

### Phase 3 — Intelligence Layer
Behavior learning. Retention optimization. Storage prediction. Agent-driven suggestions.

---

## 5. File Lifecycle Model

### 5.1 States

| State | Description | Storage |
|-------|-------------|---------|
| **ACTIVE** | File exists on filesystem (not SmartRM's concern) | User filesystem |
| **ARCHIVED** | Soft-deleted, fully restorable | Archive directory |
| **EXPIRED** | Past retention policy, candidate for purge | Archive (compressed) |
| **PURGED** | Permanently deleted, metadata retained for analytics | Metadata only |
| **FAILED** | Archive attempted but failed (permission, disk, etc.) | Metadata only |

### 5.2 Transitions

```
ACTIVE --[smartrm]--> ARCHIVED --[retention policy]--> EXPIRED --[cleanup]--> PURGED
                          |                                |
                          +--[restore]-->  ACTIVE          +--[restore]--> ACTIVE
                          |
                          +--[permanent]-> PURGED

ACTIVE --[smartrm fails]--> FAILED (terminal for that archive object)
```

**Invariants:**
- `archive_id` is immutable and never reused
- Restore does not erase history — it creates a restore event and updates object state
- A restored file deleted again creates a **new** archive object with a new `archive_id`
- `PURGED` and `FAILED` are terminal states

### 5.3 Deletion Intent (Phase 2+)

Every deletion carries intent, inferred or explicit:

```bash
smartrm file.txt                      # intent=default (inferred from file type)
smartrm file.txt --intent=temp        # short TTL, aggressive cleanup
smartrm file.txt --intent=cleanup     # build artifacts, safe to purge fast
smartrm file.txt --intent=permanent   # skip archive, go straight to PURGED
```

Phase 1: intent inferred from file classification (see 7.1).
Phase 2: explicit `--intent` flag + learned defaults.

### 5.4 Retention Policies (Phase 2+)

```bash
smartrm file.txt --ttl=7d            # expire after 7 days
smartrm file.txt --ttl=forever       # never auto-expire
smartrm -r node_modules/ --policy=aggressive  # short retention, compress immediately
```

Phase 1: global `retention_days` config.
Phase 2: per-file TTL, per-directory policies, policy presets.

---

## 6. Phase 1 — Adoption Layer (MVP)

### 6.1 Safe Delete

```bash
smartrm file.txt
smartrm -r folder/
smartrm file1.txt file2.txt
```

Behavior:
- Move file to archive directory under `<archive_root>/<archive_id>/payload` (content-addressed by immutable ID, not by filename or path)
- Record metadata: timestamp, original path, size, object type, file type tags, lifecycle state
- Every invocation creates a **batch** — even single-file deletes get a batch ID
- `batch_items` creates **one row per CLI argument**, not per file inside a directory tree. `smartrm -r node_modules/` = 1 batch_item + 1 archive_object.
- Cross-filesystem: copy+delete fallback with progress bar for files > 100MB
- Pre-flight disk space check before archiving (see 6.10)

**Archive identity**: Each archived object gets an immutable `archive_id` (ULID). This is the primary identity — not the filename, not the path. The restore system resolves by archive object identity. This matters because the same filename gets deleted multiple times, directories get renamed, and symlinks create ambiguity. ULIDs are time-sortable, so `ORDER BY archive_id` equals `ORDER BY creation time`. Human output displays the first 8 characters; `--json` emits the full ULID.

**Directory archival strategy**:
- Phase 1: **tree-preserving** — move/copy the directory tree as-is into `<archive_id>/payload/`. Simple restore, efficient same-fs rename.
- Data model: record per-file entries in `batch_items` for granular tracking, even though physical storage is a single tree.
- Phase 2+: evolve toward manifest-aware indexing without breaking restore UX. Per-file entries enable future partial restore, dedup, and analytics.

### 6.2 Undo (First-Class Primitive)

This is the core UX moment.

```bash
smartrm undo                  # Restore entire last batch
smartrm undo 3                # Restore last 3 batches
```

`undo` is not an alias — it is the primary recovery interface. Every operation is a batch, and `undo` reverses the most recent batch atomically.

### 6.3 Restore

Restore is a **first-class operation**, not merely "undoing delete." Every restore creates its own batch, records restore events, resolves conflicts deterministically, and reports exact outcomes.

```bash
smartrm restore <archive_id>              # Restore by immutable archive ID (primary)
smartrm restore --batch <batch-id>        # Restore entire batch
smartrm restore --last                    # Restore most recent deletion
smartrm restore --all                     # Restore everything (for migration/uninstall)
```

**Restore modes:**

| Mode | Syntax | Behavior |
|------|--------|----------|
| Original path | `smartrm restore <id>` | Restore to `original_path`. Recreate missing parent dirs. |
| Alternate path | `smartrm restore <id> --to /tmp/recovered/` | Restore to specified path. Record as `alternate_path` mode. |
| Batch restore | `smartrm restore --batch <id>` | Restore all succeeded objects from batch. |
| Partial batch | `smartrm restore --batch <id> --only path/to/file` | Restore subset of a batch. |
| Last operation | `smartrm restore --last` / `smartrm undo` | Restore most recent delete batch. |

**Conflict policies:**

| Policy | Behavior | When |
|--------|----------|------|
| `fail` | Do not overwrite, mark restore failed | Default in non-interactive/scripts |
| `rename` | Restore as `file (restored).txt`, `file (restored 2).txt` | Default in interactive mode |
| `overwrite` | Replace target atomically | Only with `--conflict overwrite` or `--force` |
| `skip` | Skip conflicting item, continue with rest | For multi-item restore workflows |

```bash
smartrm restore <id> --conflict rename
smartrm restore <id> --conflict overwrite
smartrm restore <id> --to /alt/path --conflict fail
```

**Restore behavior details:**
- Missing parent directories: created automatically (inheriting process umask) unless `--no-create-parents`
- Metadata restoration order: place content → restore mode → restore timestamps → attempt uid/gid
- Ownership failure does not fail the restore if content was placed successfully
- Content placement failure is a hard failure
- Multi-item restore: continue on error, report summary, exit code 1 if any failed
- Restore eligibility: only `archived` and `expired` states (not `purged` or `failed`)
- Each restore attempt recorded in `restore_events` table regardless of outcome

**Object-specific restore:**

| Object type | Restore behavior |
|-------------|-----------------|
| File | Restore content, then metadata |
| Directory | Restore tree recursively (Phase 1 tree-preserving) |
| Symlink | Restore symlink itself, never follow target. Broken symlinks restored as-is (not an error). |

### 6.4 List / Search Archive

```bash
smartrm list                              # List archived files (most recent first)
smartrm list --limit 20                   # Paginate
smartrm list --state archived             # Filter by lifecycle state
smartrm search "*.log"                    # Glob pattern search
smartrm search config                     # Substring match (no wildcards = substring)
smartrm search --after "2026-03-01"       # Filter by date
smartrm search --larger-than 10M          # Filter by size
smartrm search --dir /projects/app        # Filter by original directory
```

- Pattern with `*` or `?` = glob match. Plain string = case-insensitive substring on full path.
- Tabular output: short ID (first 8 chars of ULID), original path, deleted date, size, state
- `--json` flag for machine-readable output (emits full ULID)
- **Pagination**: `list` uses keyset pagination (`WHERE created_at < :cursor`, indexed, fast at any size). `search` uses `--offset` for simple offset pagination. LIKE-based substring search is a scan — acceptable for Phase 1 archive sizes; FTS5 deferred to Phase 2 if needed.

### 6.5 Version History

```bash
smartrm history /path/to/file.txt     # All versions of this path
```

When the same path is deleted multiple times, each deletion is a separate archive object with its own `archive_id`. `history` shows all archive objects for a given original path, ordered by deletion time. Each is independently restorable.

### 6.6 Cleanup

```bash
smartrm cleanup --older-than 30d      # Transition ARCHIVED → PURGED for old files
smartrm cleanup --dry-run             # Preview what would be cleaned
smartrm cleanup --expired             # Purge only EXPIRED state files
```

- Respects protection rules (files tagged `protected` require `--force`)
- Phase 1: manual. Phase 2: auto-cleanup via daemon.

### 6.7 Batch Tracking

Every CLI invocation creates a batch. Batches are the unit of undo. `batch_id` uses a ULID.

Batch status: `pending` | `in_progress` | `complete` | `partial` | `failed` | `rolled_back`.

On partial failure: continue archiving remaining files, mark batch as `partial`, report failures at end with per-item detail. Exit code 1. Each item's outcome is tracked in `batch_items` — the system always knows exactly what succeeded and what didn't.

**Per-file transaction strategy**: Each file in a multi-file delete is an independent transaction (DB write → FS move → commit). This ensures per-file atomicity. ~1ms overhead per file. For directory deletes (tree-preserving), it's a single transaction regardless of directory contents.

This is different from rm's "continue and forget" model: SmartRM's batches give stateful visibility into every operation.

### 6.8 Timeline (Phase 1 — basic)

```bash
smartrm timeline                      # Chronological batch history
smartrm timeline --today              # Today's activity
smartrm timeline --dir /projects/     # Scoped to directory
```

Phase 1: simple chronological list of batches with operation type, file count, status.
Phase 2: rich "git log for your filesystem" with batch grouping, restore events, pattern annotations.

### 6.9 Stats

```bash
smartrm stats                         # Archive size, file count, top deleted dirs
smartrm stats --predict               # (Phase 2) "Archive will fill in 5 days at current rate"
```

### 6.10 Disk Space Guardrails

Before every archive operation:
- `statvfs()` on archive filesystem
- If `free_space - file_size < min_free_space` (default 1GB, configurable): **block** with actionable message

```
Archive disk 96% full (1.2 GB free).
Run 'smartrm cleanup --older-than 7d' to free ~4.3 GB.
Use --force to archive anyway.
```

Phase 2: predictive model — "You're deleting 3 GB/day. Archive will fill in 5 days."

### 6.11 Danger Detection

Block or warn on dangerous operations:

| Operation | Behavior |
|-----------|----------|
| `smartrm -rf /` | **BLOCKED**. No override. |
| `smartrm -rf ~` | **WARNING**, requires `--yes-i-am-sure` |
| `smartrm -rf .git` | **WARNING**: "This will archive your git history" |
| `smartrm` on mount points | **WARNING** |
| `smartrm --permanent` on >1GB | **WARNING** with size confirmation |

Protected paths (configurable):
- `/`, `/home`, `/etc`, `/usr`, `/var`, `/boot`, `/bin`, `/sbin`
- `~/.ssh`, `~/.gnupg`

### 6.12 rm Flag Compatibility

Phase 1 supports core flags for adoption. SmartRM is NOT trying to be rm forever — these exist so `alias rm=smartrm` works during the transition period. Phase 2+ gradually introduces SmartRM-native primitives (`--intent`, `--ttl`, `--policy`) that are better than rm's legacy flags.

**Supported:**

| Flag | Behavior |
|------|----------|
| `-r`, `-R`, `--recursive` | Archive directory recursively |
| `-f`, `--force` | No prompts, no error on missing files |
| `-i` | Prompt before each file |
| `-I` | Prompt once if >3 files or recursive |
| `-d`, `--dir` | Archive empty directories |
| `-v`, `--verbose` | Print each file as archived |
| `--preserve-root` | On by default. Blocks `smartrm -rf /` |
| `--no-preserve-root` | Ignored. SmartRM always protects root. |
| `--one-file-system` | Honored — don't cross mount boundaries |
| `--` | End of flags |

**Not supported (SmartRM-native flags replace these over time):**
- `--interactive=WHEN` — use `-i` or `-I`

### 6.13 Permanent Delete

```bash
smartrm --permanent file.txt          # Prompt: "Permanently delete file.txt? [y/N]"
smartrm --permanent --force file.txt  # No prompt (non-protected paths only)
smartrm --permanent -r folder/        # Shows count+size before prompting
```

For multiple files/directories: `Permanently delete 47 files (2.3 GB)? [y/N]`

**Two-tier behavior**:
- `--permanent` on **non-protected paths**: interactive y/N confirmation only. No passphrase/phrase gate required. `--permanent --force` suppresses the prompt and works in non-interactive mode.
- `--permanent` on **protected paths**: triggers the full destructive gate (see Section 13). `--force` alone is not sufficient.

### 6.14 Configuration

```bash
smartrm config                            # Show current config
smartrm config set retention_days 30      # Set default retention
smartrm config set archive_dir /mnt/archive
smartrm config set min_free_space 2G
smartrm config set danger_protection true
```

**Config resolution order** (highest priority first):
1. `SMARTRM_HOME` env var (overrides all paths)
2. `$XDG_CONFIG_HOME/smartrm/config.json` (user config)
3. `/etc/smartrm/config.json` (system-wide defaults)
4. Built-in defaults

Uses `dirs` crate: XDG on Linux, `~/Library/Application Support/` on macOS.

Default config:
```json
{
  "default_delete_mode": "archive",
  "min_free_space_bytes": 1073741824,
  "default_restore_conflict_mode": "rename",
  "default_ttl_seconds": null,
  "protected_patterns": [".env", ".env.*"],
  "excluded_patterns": [],
  "archive_root": null,
  "danger_protection": true,
  "auto_cleanup": false,
  "allow_destructive_commands": "interactive_with_confirmation",
  "agent_detection": true
}
```

`allow_destructive_commands` values: `disabled` | `interactive_only` | `interactive_with_confirmation` | `interactive_with_passphrase` | `break_glass`. Default is `interactive_with_confirmation`.

`agent_detection` is a separate layer from the auth method — always on by default. Disable only in known-safe environments where automation legitimately needs to invoke SmartRM non-destructively.

### 6.15 Shell Completions

```bash
smartrm completions bash > /etc/bash_completion.d/smartrm
smartrm completions zsh > ~/.zfunc/_smartrm
smartrm completions fish > ~/.config/fish/completions/smartrm.fish
```

Phase 1: static completions via `clap_complete` (subcommands + flags).
Phase 2: dynamic completions querying archive DB for `restore`, `history`.

### 6.16 Migration / Uninstall

```bash
smartrm restore --all           # Put all archived files back
smartrm purge                   # Delete archive dir + DB
```

`purge` checks if archive is non-empty:
```
Archive contains 234 files (1.3 GB).
Run 'smartrm restore --all' first, or 'smartrm purge --force' to discard.
```

Full uninstall: `restore --all` → `purge` → remove binary → remove shell alias.

### 6.17 Exit Codes

Delete operations (rm-compatible):

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error (permission denied, missing file, partial failure) |

Subcommands (`restore`, `search`, `cleanup`, etc.):

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error |
| 2 | Nothing found / nothing to do |

### 6.18 Explain (Phase 1 — basic)

```bash
smartrm explain <archive_id>          # Why was this archived? What policy applied?
smartrm explain-policy /path/to/file  # What would happen if this file were deleted?
```

Shows effective policy settings and their source. Powered by `effective_policies` table — every policy decision is recorded at archive time for later inspection.

Phase 1: populated for key settings (delete_mode, ttl, conflict_mode).
Phase 2+: full explainability across all settings.

---

## 7. Intelligence Layer (Spans All Phases)

### 7.1 File Classification (Phase 1 — heuristics)

Classify on delete via extension + path patterns:

| Category | Examples | Tag | Default behavior |
|----------|----------|-----|------------------|
| Build artifacts | `node_modules/`, `dist/`, `__pycache__/`, `*.o` | `build` | Short retention |
| Temp files | `*.tmp`, `*.swp`, `*.log`, `*.bak` | `temp` | Short retention |
| User content | `*.md`, `*.py`, `*.js`, `*.ts`, source code | `content` | Long retention |
| Config/secrets | `.env`, `*credentials*`, `*key*`, `*.pem` | `protected` | Warn before cleanup |

The unified `classifier.rs` returns `Classification { tags: Vec<Tag>, danger_level: DangerLevel }` from a single pattern-matching engine. Tags and danger level are computed together — no separate danger detection pass.

Phase 1: extension + path heuristics. Phase 3: learned classification from behavior.

### 7.2 Importance Scoring (Phase 3)

Track deletion + restoration patterns:

- Files restored frequently → high importance → longer retention, warn on cleanup
- Files never restored after 7 days → low importance → early cleanup candidate
- Per-directory deletion frequency informs default policies

### 7.3 Storage Prediction (Phase 2+)

```bash
smartrm stats --predict
# Archive growing at 3.2 GB/day
# Estimated full in 5 days
# Recommendation: enable auto_cleanup or increase retention aggressiveness
```

### 7.4 Agent Suggestions (Phase 3)

```bash
smartrm suggest
# "You restore .env files 80% of the time. Consider excluding them from deletion."
# "node_modules/ accounts for 60% of archive. Consider --policy=aggressive for build dirs."
```

---

## 8. Symlink Handling

Archive the symlink itself (not the target). This matches `rm` behavior — `rm` removes the link, not what it points to.

- Record symlink target path in `link_target` metadata field for debugging
- Object type recorded as `symlink` (distinct from `file` and `dir`)
- Broken symlinks archived normally — this is not an error
- Symlinks inside directories preserved as-is during tree-preserving archive
- `smartrm history` shows what each symlink pointed to
- Restore recreates the symlink, never follows the target

---

## 9. Permissions & Ownership

Store in metadata on archive: `mode`, `uid`, `gid`, `mtime_ns`, `ctime_ns`.

On restore (in order):
1. Place content at destination
2. Restore `mode` (permissions) — always
3. Restore `mtime` — always
4. Attempt `uid`/`gid` — warn if not root and can't; **not a failure**

Same-filesystem `rename()` preserves all attributes inherently. Cross-filesystem copy explicitly records + restores them.

Restore event records: `mode_restored`, `ownership_restored`, `timestamps_restored` booleans for explainability.

xattrs/ACLs: fields reserved in schema (`xattrs_json`, `acl_blob`), populated in Phase 2.

---

## 10. Concurrency

SQLite in WAL mode with 5-second busy timeout. Set at DB creation. `synchronous = NORMAL` for performance. Handles concurrent SmartRM processes across terminals, scripts, cron.

---

## 11. Hashing Strategy

**Phase 1:**
- Same-filesystem moves: no hash (instant `rename()`, no bytes read)
- Cross-filesystem copies: SHA-256 computed during copy (free — bytes are already streaming) using 256KB chunks (`const HASH_BUF_SIZE: usize = 262144`)
- Hash field nullable in DB
- `hash_jobs` table created (Phase 2-ready) to track pending hashes

**Phase 2:**
- Background daemon hashes all un-hashed archives asynchronously
- Eventually consistent: every file gets a hash
- Enables dedup, integrity checks, content-addressable storage

This avoids the false choice between "hash everything synchronously" (slow) and "never hash" (kills future features).

---

## 12. Policy Precedence Model

Lifecycle policy (intent, TTL, retention, cleanup eligibility, conflict mode) is resolved per-operation from multiple sources. A clear precedence model prevents "magic" behavior and preserves trust.

### 12.1 Precedence Order (highest to lowest)

| Priority | Source | Example |
|----------|--------|---------|
| 0 | **Hard safety constraints** | Cannot delete `/`, cannot override root protection |
| 1 | **Explicit CLI flags** | `--ttl=7d`, `--permanent`, `--conflict overwrite` |
| 2 | **Interactive user choices** | User confirms overwrite at prompt |
| 3 | **User-defined rules** | `protect .env`, `*.log ttl 3d` in user config |
| 4 | **Project/path-scoped rules** | `/repo/build/** ttl 1d` |
| 5 | **System-wide defaults** | `/etc/smartrm/config.json` settings |
| 6 | **Learned recommendations** | "User never restores /tmp/ build artifacts" |
| 7 | **Built-in fallback defaults** | Hardcoded safe defaults |

### 12.2 Resolution

For each operation, these settings are resolved independently through the precedence chain:
- `delete_mode` (archive vs permanent)
- `ttl_seconds`
- `delete_intent`
- `min_free_space_bytes`
- `restore_conflict_policy`

The winning value and its source are recorded in `effective_policies` for explainability.

**Phase 1 scope**: `effective_policies` is written at **batch level only** — one row per setting per batch. Per-object policy recording is deferred to Phase 2. The `archive_id` column exists in the schema for forward-compatibility but is not populated in Phase 1.

### 12.3 Learned Behavior Guardrails (Phase 3)

| Allowed by default | Requires explicit opt-in |
|---------------------|--------------------------|
| Recommendations, warnings, suggestions | Automatic TTL tuning |
| Dashboard/timeline annotations | Automatic cleanup actions |
| | Automatic compression tier changes |
| | Silent reclassification of protected paths |

**Core rule**: Learned behavior recommends before it silently overrides — unless the user has explicitly enabled automation.

---

## 13. Destructive Command Gate

Irreversible commands require a **human-only interactive gate**. No flag, env var, piped input, or config secret can bypass it. This is the last line of defense against automation-driven data loss.

### 13.1 Commands Under Gate

| Command | Gate level |
|---------|-----------|
| `smartrm purge` (non-protected) | Confirmation phrase gate |
| `smartrm --permanent <files>` (protected paths) | Confirmation phrase gate |
| `smartrm restore --conflict overwrite` (destructive overwrite) | Confirmation phrase gate |
| `smartrm config set` that shortens retention or disables safeguards | Confirmation phrase gate |
| `smartrm purge` affecting protected paths | **Elevated**: type `PURGE PROTECTED` + auth |
| `smartrm purge --all` | **Elevated**: type `PURGE ALL` + auth |
| Disabling archive mode globally | **Elevated** |
| Removing protected path rules | **Elevated** |

Note: `smartrm --permanent <files>` on **non-protected paths** is NOT subject to the destructive gate — only an interactive y/N prompt (see Section 6.13).

### 13.2 Required Conditions

A gated command may proceed **only if all** of these are true:

1. **Interactive TTY session**: `stdin` is a TTY, `stderr` is a TTY, session is not headless
2. **Auth challenge from `/dev/tty`**: confirmation phrase or passphrase read directly from terminal device — not stdin, not piped, not env var, not config file
3. **Scope preview shown before auth prompt**: object count, total size, scope, protected paths affected, representative examples, exact action
4. **Attempt logged**: success and failure, with timestamp, user, host, cwd, command, scope summary
5. **Cooldown enforced**: limited retries, backoff after failures

If **any** condition is not met, the command **fails immediately**. No fallback prompt. No alternative path.

### 13.3 UX Flow

```
$ smartrm purge --expired

About to purge 482 expired objects (18.4 GB)
Scope: expired-only
Protected paths affected: 0
Examples:
  /home/max/project/logs/app.log
  /home/max/tmp/build-cache.bin
  /home/max/archive/old-report.pdf

This action is irreversible.
Type to confirm: PURGE 482 OBJECTS ▊

Approval accepted.
Proceeding with purge of 482 objects.
```

The confirmation phrase is dynamically generated from the scope (e.g. `PURGE 482 OBJECTS`). It is read from `/dev/tty`. Case-sensitive exact match required.

On failure:
```
Approval denied.
2 attempts remaining before temporary lockout.
```

When protected paths are in scope:
```
About to purge 15 objects (3.1 GB)
Scope: all archived
Protected paths affected: 3
  ~/.env
  ~/project/.env.production
  ~/secrets/api-key.pem

Additional confirmation required.
Type: PURGE PROTECTED
Passphrase (if set): ▊
```

No TTY present:
```
Error: Destructive command requires interactive terminal session.
Cannot proceed without TTY on stdin and stderr.
```

### 13.4 Implementation Rules

1. **Auth input from `/dev/tty` only** — `open("/dev/tty")` for reading. Piped input (`echo "phrase" | smartrm purge`) must fail.
2. **Hard-block when no TTY** — do not downgrade to another mode. No "pass auth another way." Command fails.
3. **Agent detection is a separate, always-on layer** — if stdin is redirected or automation markers are detected (env vars like `CI=true`, `TERM=dumb`, etc.), block even if a TTY technically exists. Configurable via `"agent_detection": true` (default).
4. **Scope preview generated before prompt** — the human approves a concrete action, not a vague one.
5. **Protected paths trigger elevated friction** — require typing a static confirmation phrase (`PURGE PROTECTED`) plus the configured auth method.

**Auth methods** (configured via `allow_destructive_commands`):
- `interactive_with_confirmation` (DEFAULT): dynamically generated phrase like `PURGE 482 OBJECTS`, read from `/dev/tty`. No setup required.
- `interactive_with_passphrase`: user sets passphrase via `smartrm config set-passphrase`; stored as argon2 hash; read from `/dev/tty` at gate time. Opt-in.

### 13.5 Scope Preview (Minimum Fields)

Every gated command must display before prompting:

- Action type (purge, permanent delete, overwrite restore, policy change)
- Object/batch count
- Total size
- Protected paths count (with paths listed if > 0)
- Age range of affected objects
- Exact filters used
- Representative examples (up to 5)

### 13.6 Cooldown / Retry Defaults

| Setting | Default |
|---------|---------|
| Max password attempts per invocation | 3 |
| Cooldown after 3 failures | 30 seconds |
| Escalating cooldown on repeated sessions | 2x per consecutive lockout |
| Lockout after repeated failures | 5 minutes |

### 13.7 Audit Logging

Every destructive attempt writes to `destructive_audit_log` table:

```sql
CREATE TABLE IF NOT EXISTS destructive_audit_log (
    attempt_id TEXT PRIMARY KEY, -- ULID
    timestamp TEXT NOT NULL,
    os_user TEXT,
    hostname TEXT,
    cwd TEXT,
    command TEXT NOT NULL,
    arguments TEXT,
    interactive_tty_present INTEGER NOT NULL CHECK (interactive_tty_present IN (0, 1)),
    scope_count INTEGER,
    scope_bytes INTEGER,
    protected_paths_affected INTEGER,
    result TEXT NOT NULL CHECK (result IN ('allowed', 'denied', 'locked_out', 'no_tty', 'blocked_agent')),
    failure_reason TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dal_timestamp ON destructive_audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_dal_result ON destructive_audit_log(result);
CREATE INDEX IF NOT EXISTS idx_dal_os_user ON destructive_audit_log(os_user);
```

Logs **both** successful and denied attempts.

### 13.8 Destructive Commands Config

```json
{
  "allow_destructive_commands": "interactive_with_confirmation",
  "agent_detection": true
}
```

| Value | Behavior |
|-------|----------|
| `disabled` | All gated commands blocked. For agent/CI environments. |
| `interactive_only` | Requires TTY, scope preview, y/N confirm only (no phrase or passphrase) |
| `interactive_with_confirmation` | Full gate: TTY + scope preview + dynamically generated confirmation phrase from `/dev/tty` **(DEFAULT)** |
| `interactive_with_passphrase` | Full gate: TTY + scope preview + user-set passphrase (argon2) from `/dev/tty`. Set via `smartrm config set-passphrase`. |
| `break_glass` | Same as `interactive_with_confirmation` but additionally enables `purge --all` and protected path removal |

Default: `interactive_with_confirmation`. No setup required.
Agent environments should pin to `disabled`.

### 13.9 Gate Classification

| Tier | Commands | Requirements |
|------|----------|-------------|
| **Simple confirm** | `smartrm --permanent <files>` on non-protected paths | Interactive y/N prompt only. `--force` bypasses in non-interactive mode. |
| **Standard gate** | `purge --expired`, `--permanent` on protected paths, destructive overwrite restore, TTL shortening | TTY + scope preview + confirmation phrase (default) or passphrase (opt-in) |
| **Elevated gate** | `purge --all`, `purge` affecting protected paths, disabling archive mode, removing protected rules | TTY + scope preview + static phrase (e.g. `PURGE PROTECTED`) + confirmation phrase or passphrase |
| **Forbidden** | N/A (reserved for future) | Cannot proceed under any circumstances |

---

## 14. Architecture

### Phase 1 Components

```
CLI Binary (Rust)
    |
    +-- Command Parser (clap v4)
    |
    +-- Batch Manager
    |       +-- Batch creation + tracking
    |       +-- Partial failure handling
    |       +-- Per-item outcome recording (batch_items)
    |
    +-- Archive Manager
    |       +-- File Mover (rename, copy+delete fallback)
    |       +-- Metadata Tracker (SQLite, WAL mode)
    |       +-- Restore Engine (first-class, with conflict policies)
    |       +-- Lifecycle State Machine
    |
    +-- Policy Engine
    |       +-- Config resolver (env → user → system → defaults)
    |       +-- Effective policy recorder
    |
    +-- Intelligence Layer
    |       +-- File Classifier (extension/path heuristics)
    |       +-- Danger Detector
    |       +-- Disk Space Monitor
    |
    +-- Destructive Command Gate
    |       +-- TTY Verifier
    |       +-- Agent Detector
    |       +-- Scope Preview Generator
    |       +-- /dev/tty Password Reader
    |       +-- Cooldown Manager
    |       +-- Audit Logger
    |
    +-- Storage Engine
            +-- Archive Layout (<archive_id>/payload)
            +-- Cross-filesystem Handler
            +-- Streaming Hasher (cross-fs only)
```

### Phase 2 Additions

```
Background Daemon (smartrmd)
    |
    +-- Async Hasher (fill missing hashes)
    +-- Auto-Cleanup (expire ARCHIVED → PURGED)
    +-- Storage Predictor
    +-- Compression Engine (zstd for EXPIRED state)
    +-- Dedup Engine (content-addressable by hash)
    +-- Manifest Indexer (evolve from tree-preserving to manifest-aware)
```

### Phase 3 Additions

```
Intelligence Agent
    |
    +-- Behavior Tracker (delete/restore patterns)
    +-- Importance Scorer
    +-- Retention Optimizer
    +-- Suggestion Engine
```

### Technology

- **Language**: Rust (single static binary, fast, safe)
- **Database**: SQLite via `rusqlite` (WAL mode)
- **CLI framework**: clap v4 with `clap_complete`
- **Hashing**: SHA-256 via `sha2` crate (streaming, 256KB chunks — `const HASH_BUF_SIZE`; 4x fewer syscalls per GB vs 64KB)
- **IDs**: `ulid` crate — time-sortable, lexicographically ordered, collision-resistant
- **Paths**: `dirs` crate (XDG on Linux, platform-native on macOS)
- **Compression**: zstd (Phase 2)
- **Cross-platform**: Linux + macOS (primary), Windows (future)

### Binary Distribution

- Single self-contained binary: `smartrm`
- Install: copy to `/usr/local/bin/smartrm`
- Optional: `alias rm='smartrm'` in shell config (adoption bridge, not permanent design goal)

### Recommended Module Structure

```
src/
  main.rs
  cli.rs
  commands/           # Thin CLI glue — parse args, call operations, format output
    delete.rs
    restore.rs
    undo.rs
    list.rs
    search.rs
    cleanup.rs
    purge.rs
    timeline.rs
    explain.rs
    completions.rs
    config.rs
    stats.rs
    history.rs
  operations/         # Business logic layer
    delete.rs         # Full delete flow: classify → check danger → check disk → move → DB write
    restore.rs        # Full restore flow: resolve → conflict → FS restore → metadata → DB
    cleanup.rs
    purge.rs
  db/
    mod.rs
    schema.rs         # DDL, migrations, PRAGMA setup
    operations.rs     # Transactional writes (batch + items + archive_objects in single tx)
    queries.rs        # Read queries (list, search, history, timeline, stats)
  fs/                 # Purely mechanical FS operations
    archive.rs        # move/copy files
    restore.rs        # place files, set permissions
    metadata.rs       # read permissions/ownership
    mounts.rs
    disk_space.rs     # statvfs checks
    hashing.rs        # streaming SHA-256
  policy/
    resolver.rs
    config.rs
    classifier.rs     # Unified: returns Classification { tags: Vec<Tag>, danger_level: DangerLevel }
  gate/
    mod.rs
    tty.rs
    scope_preview.rs
    auth.rs           # confirmation phrase + passphrase (argon2) auth
    cooldown.rs
    agent_detection.rs
    audit.rs
  models/
    batch.rs
    archive_object.rs
    batch_item.rs
    restore_event.rs
    policy.rs
  output/
    mod.rs            # Output trait, format selection
    human.rs          # impl HumanOutput for each result type
    # JSON output via #[derive(Serialize)] on result structs — no separate json.rs needed
```

**Architecture notes**:
- **Commands are thin**: parse CLI args into typed request structs, call operations, format output. No business logic.
- **Operations own business logic**: policy resolution → conflict checking → FS ops → DB writes. Operations call `fs/` and `db/` directly.
- **`policy/classifier.rs` is unified**: returns `Classification { tags: Vec<Tag>, danger_level: DangerLevel }` from one pattern-matching engine. No separate `danger.rs`.
- **`Filesystem` trait**: FS operations abstracted behind a `Filesystem` trait for testability. Production impl hits real FS. Tests inject mock that can simulate EXDEV, permission errors, disk full.
- **`GateEnvironment` trait**: destructive gate depends on `GateEnvironment`: `is_tty()`, `read_secret()`, `get_env()`, `now()`. Production impl hits real TTY/env. Tests inject mock.
- **DB consistency contract**: DB write first (within transaction), FS move second, commit. If FS move fails, transaction rolls back automatically. If commit fails after FS move, compensating action moves file back to original path.

---

## 15. CLI Interface Summary

### Core (Phase 1)

| Command | Description |
|---------|-------------|
| `smartrm <files...>` | Archive files (safe delete) |
| `smartrm -r <dir>` | Archive directory recursively |
| `smartrm -f <files...>` | Archive without prompts |
| `smartrm -rf <dir>` | Archive directory without prompts |
| `smartrm undo [N]` | Restore last N batches (default 1) |
| `smartrm restore <archive_id>` | Restore by immutable archive ID |
| `smartrm restore --batch <id>` | Restore entire batch |
| `smartrm restore --last` | Restore most recent deletion |
| `smartrm restore --all` | Restore everything (for uninstall) |
| `smartrm list [--state X]` | List archived files |
| `smartrm search <pattern>` | Search archives (glob or substring) |
| `smartrm history <path>` | Version history for a path |
| `smartrm timeline [--today]` | Chronological batch history |
| `smartrm cleanup --older-than <dur>` | Transition old archives to purged |
| `smartrm stats` | Archive statistics |
| `smartrm config [set key value]` | View/modify configuration |
| `smartrm completions <shell>` | Generate shell completions |
| `smartrm explain <archive_id>` | Why was this archived? What policy? |
| `smartrm explain-policy <path>` | What would happen to this file? |
| `smartrm --permanent <files...>` | True delete (destructive gate) |
| `smartrm purge` | Delete entire archive (destructive gate) |

### Phase 2+

| Command | Description |
|---------|-------------|
| `smartrm <files...> --intent=X` | Explicit deletion intent |
| `smartrm <files...> --ttl=Xd` | Per-file retention override |
| `smartrm <files...> --policy=X` | Apply retention policy preset |
| `smartrm stats --predict` | Storage growth prediction |
| `smartrm suggest` | Agent-driven recommendations |
| `smartrmd start` | Start background daemon |

---

## 16. Success Metrics

| Metric | Target | Phase |
|--------|--------|-------|
| Accidental data loss incidents | Zero | 1 |
| Same-fs delete latency | < 50ms | 1 |
| Cross-fs delete latency | < file copy time + 100ms overhead | 1 |
| Restore latency (single file) | < 100ms | 1 |
| Archive metadata overhead | < 1KB per file | 1 |
| Binary size | < 10MB | 1 |
| Hash coverage | 100% of archives | 2 |
| Storage prediction accuracy | Within 20% over 7-day horizon | 2 |
| Auto-cleanup disk savings | > 40% reduction vs no cleanup | 2 |

---

## 17. MVP Scope (Phase 1)

**In scope:**
- Safe delete (files, directories, multiple files, cross-filesystem)
- Immutable archive identity (archive_id per object, not filename-based)
- Undo (batch-based, first-class command)
- Restore (first-class operation: by ID, batch, last, alternate path, conflict policies)
- List, search (glob + substring), timeline (basic)
- SQLite metadata with WAL mode (full schema from Spec 1)
- Batch tracking with per-item outcomes and partial failure handling
- Restore events recorded separately (restore_events table)
- File classification by extension/path heuristics
- Danger detection for destructive paths
- Disk space pre-flight check
- Version tracking via independent archive objects per path
- Symlink archival with target metadata
- Permission/ownership preservation and restoration
- rm flag compatibility (core set, as adoption bridge)
- Policy precedence model (minimal for Phase 1, extensible)
- Effective policy recording for explainability
- Config with XDG compliance and layered resolution
- Shell completions (static)
- Stats command
- Explain command (basic)
- Purge (for uninstall)
- Exit codes (rm-compatible for deletes, richer for subcommands)
- JSON output on all subcommands (`--json`)
- Tree-preserving directory archival with per-file batch_items
- Human-only destructive command gate (TTY + scope preview + `/dev/tty` confirmation phrase or passphrase)
- Destructive audit logging (all attempts, success and failure)
- Cooldown/retry limits on gated commands
- `allow_destructive_commands` config (`disabled` / `interactive_only` / `interactive_with_confirmation` / `interactive_with_passphrase` / `break_glass`)
- Agent detection layer (`"agent_detection": true`, always on by default)

**Phase 2 (System Layer):**
- Background daemon (`smartrmd`)
- Async hashing (eventually consistent, backfill via hash_jobs)
- Compression (zstd for expired files)
- Deduplication (content-addressable by hash)
- Auto-cleanup daemon
- Storage prediction
- Per-file TTL and retention policies
- Deletion intent (explicit `--intent` flag)
- Dynamic shell completions
- Rich timeline with batch grouping and restore events
- xattrs/ACL preservation
- Manifest-aware directory indexing
- Per-mount logical archive support

**Phase 3 (Intelligence Layer):**
- Behavior learning (delete/restore patterns)
- Importance scoring
- Retention optimization
- Agent-driven suggestions (recommend, never silently override without opt-in)
- Learned file classification
- Cross-directory pattern detection

---

# Appendix A: Spec 1 — Data Model / Schema

## Design Principles

- Every archived object has an **immutable identity** (`archive_id`)
- Batches are first-class entities — the unit of undo
- Lifecycle state is explicit and timestamped
- Physical storage location is separate from logical identity
- Restore events are recorded independently, not implied
- Schema supports same-fs rename now and logical archive indexing later

## PRAGMA Setup

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;
```

## Tables

### batches

Represents a top-level operation (delete, restore, cleanup, purge).

```sql
CREATE TABLE IF NOT EXISTS batches (
    batch_id TEXT PRIMARY KEY, -- ULID
    operation_type TEXT NOT NULL CHECK (operation_type IN ('delete', 'restore', 'cleanup', 'purge')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'complete', 'partial', 'failed', 'rolled_back')),

    requested_by TEXT,
    cwd TEXT,
    hostname TEXT,
    command_line TEXT,

    total_objects_requested INTEGER NOT NULL DEFAULT 0,
    total_objects_processed INTEGER NOT NULL DEFAULT 0,
    total_objects_succeeded INTEGER NOT NULL DEFAULT 0,
    total_objects_failed INTEGER NOT NULL DEFAULT 0,
    total_bytes INTEGER NOT NULL DEFAULT 0,

    interactive_mode INTEGER NOT NULL DEFAULT 0 CHECK (interactive_mode IN (0, 1)),
    used_force INTEGER NOT NULL DEFAULT 0 CHECK (used_force IN (0, 1)),

    started_at TEXT NOT NULL,
    completed_at TEXT,
    summary_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_batches_started_at ON batches(started_at);
CREATE INDEX IF NOT EXISTS idx_batches_status ON batches(status);
CREATE INDEX IF NOT EXISTS idx_batches_operation_type ON batches(operation_type);
```

### batch_items

Tracks each input target and its resolution within a batch.

```sql
CREATE TABLE IF NOT EXISTS batch_items (
    batch_item_id TEXT PRIMARY KEY, -- ULID
    batch_id TEXT NOT NULL,

    input_path TEXT NOT NULL,
    resolved_path TEXT,
    archive_id TEXT,

    status TEXT NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed', 'skipped')),
    error_code TEXT,
    error_message TEXT,

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    FOREIGN KEY (batch_id) REFERENCES batches(batch_id) ON DELETE CASCADE,
    FOREIGN KEY (archive_id) REFERENCES archive_objects(archive_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_batch_items_batch_id ON batch_items(batch_id);
CREATE INDEX IF NOT EXISTS idx_batch_items_status ON batch_items(status);
CREATE INDEX IF NOT EXISTS idx_batch_items_input_path ON batch_items(input_path);
```

### mounts

Logical tracking for cross-filesystem evolution.

```sql
CREATE TABLE IF NOT EXISTS mounts (
    mount_id TEXT PRIMARY KEY, -- ULID
    device_name TEXT,
    mount_point TEXT NOT NULL,
    fs_type TEXT,
    archive_root TEXT,

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mounts_mount_point ON mounts(mount_point);
```

### policies

Declared policy objects for lifecycle and retention behavior.

```sql
CREATE TABLE IF NOT EXISTS policies (
    policy_id TEXT PRIMARY KEY, -- ULID
    name TEXT NOT NULL UNIQUE,

    scope_type TEXT NOT NULL CHECK (scope_type IN ('global', 'user', 'path_prefix', 'project', 'mount')),
    scope_value TEXT,

    priority INTEGER NOT NULL,

    intent_default TEXT,
    ttl_seconds_default INTEGER,
    min_free_space_bytes INTEGER,
    auto_cleanup_enabled INTEGER NOT NULL DEFAULT 0 CHECK (auto_cleanup_enabled IN (0, 1)),

    protect_patterns_json TEXT,
    exclude_patterns_json TEXT,

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_policies_scope ON policies(scope_type, scope_value);
CREATE INDEX IF NOT EXISTS idx_policies_priority ON policies(priority DESC);
```

### archive_objects

Core table. One row per archived filesystem object.

```sql
CREATE TABLE IF NOT EXISTS archive_objects (
    archive_id TEXT PRIMARY KEY, -- ULID
    batch_id TEXT NOT NULL,
    parent_archive_id TEXT,

    object_type TEXT NOT NULL CHECK (object_type IN ('file', 'dir', 'symlink', 'other')),
    state TEXT NOT NULL CHECK (state IN ('archived', 'restored', 'expired', 'purged', 'failed')),

    original_path TEXT NOT NULL,
    archived_path TEXT,
    storage_mount_id TEXT,
    original_mount_id TEXT,

    size_bytes INTEGER,
    content_hash TEXT,
    link_target TEXT,

    mode INTEGER,
    uid INTEGER,
    gid INTEGER,
    mtime_ns INTEGER,
    ctime_ns INTEGER,

    xattrs_json TEXT,
    acl_blob BLOB,

    delete_intent TEXT,
    ttl_seconds INTEGER,
    policy_id TEXT,
    delete_reason TEXT,

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    restored_at TEXT,
    expired_at TEXT,
    purged_at TEXT,

    failure_code TEXT,
    failure_message TEXT,

    FOREIGN KEY (batch_id) REFERENCES batches(batch_id) ON DELETE RESTRICT,
    FOREIGN KEY (parent_archive_id) REFERENCES archive_objects(archive_id) ON DELETE SET NULL,
    FOREIGN KEY (storage_mount_id) REFERENCES mounts(mount_id) ON DELETE SET NULL,
    FOREIGN KEY (original_mount_id) REFERENCES mounts(mount_id) ON DELETE SET NULL,
    FOREIGN KEY (policy_id) REFERENCES policies(policy_id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_ao_batch_id ON archive_objects(batch_id);
CREATE INDEX IF NOT EXISTS idx_ao_parent_archive_id ON archive_objects(parent_archive_id);
CREATE INDEX IF NOT EXISTS idx_ao_state ON archive_objects(state);
CREATE INDEX IF NOT EXISTS idx_ao_original_path ON archive_objects(original_path);
CREATE INDEX IF NOT EXISTS idx_ao_created_at ON archive_objects(created_at);
CREATE INDEX IF NOT EXISTS idx_ao_content_hash ON archive_objects(content_hash);
CREATE INDEX IF NOT EXISTS idx_ao_delete_intent ON archive_objects(delete_intent);
CREATE INDEX IF NOT EXISTS idx_ao_policy_id ON archive_objects(policy_id);
```

### restore_events

Every restore attempt is recorded, regardless of outcome.

```sql
CREATE TABLE IF NOT EXISTS restore_events (
    restore_event_id TEXT PRIMARY KEY, -- ULID
    archive_id TEXT NOT NULL,
    restore_batch_id TEXT NOT NULL,

    restore_mode TEXT NOT NULL CHECK (restore_mode IN ('original', 'alternate_path', 'overwrite', 'rename_on_conflict')),
    requested_target_path TEXT,
    final_restored_path TEXT,

    status TEXT NOT NULL CHECK (status IN ('succeeded', 'failed', 'partial')),
    conflict_policy TEXT NOT NULL CHECK (conflict_policy IN ('fail', 'rename', 'overwrite', 'skip')),

    mode_restored INTEGER NOT NULL DEFAULT 0 CHECK (mode_restored IN (0, 1)),
    ownership_restored INTEGER NOT NULL DEFAULT 0 CHECK (ownership_restored IN (0, 1)),
    timestamps_restored INTEGER NOT NULL DEFAULT 0 CHECK (timestamps_restored IN (0, 1)),

    error_code TEXT,
    error_message TEXT,

    created_at TEXT NOT NULL,

    FOREIGN KEY (archive_id) REFERENCES archive_objects(archive_id) ON DELETE RESTRICT,
    FOREIGN KEY (restore_batch_id) REFERENCES batches(batch_id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_re_archive_id ON restore_events(archive_id);
CREATE INDEX IF NOT EXISTS idx_re_restore_batch_id ON restore_events(restore_batch_id);
CREATE INDEX IF NOT EXISTS idx_re_created_at ON restore_events(created_at);
```

### effective_policies

Stores resolved policy decisions for explainability.

```sql
CREATE TABLE IF NOT EXISTS effective_policies (
    effective_policy_id TEXT PRIMARY KEY, -- ULID

    batch_id TEXT,
    archive_id TEXT,

    setting_key TEXT NOT NULL,
    setting_value TEXT,
    source_type TEXT NOT NULL CHECK (source_type IN ('cli', 'interactive', 'user_rule', 'project_rule', 'system_rule', 'learned', 'default', 'hard_safety')),
    source_ref TEXT,
    created_at TEXT NOT NULL,

    FOREIGN KEY (batch_id) REFERENCES batches(batch_id) ON DELETE CASCADE,
    FOREIGN KEY (archive_id) REFERENCES archive_objects(archive_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ep_batch_id ON effective_policies(batch_id);
CREATE INDEX IF NOT EXISTS idx_ep_archive_id ON effective_policies(archive_id);
CREATE INDEX IF NOT EXISTS idx_ep_setting_key ON effective_policies(setting_key);
```

### hash_jobs (Phase 2-ready)

```sql
CREATE TABLE IF NOT EXISTS hash_jobs (
    hash_job_id TEXT PRIMARY KEY, -- ULID
    archive_id TEXT NOT NULL,

    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    FOREIGN KEY (archive_id) REFERENCES archive_objects(archive_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_hj_status ON hash_jobs(status);
CREATE INDEX IF NOT EXISTS idx_hj_archive_id ON hash_jobs(archive_id);
```

### destructive_audit_log

Audit trail for all destructive command attempts (success and failure).

```sql
CREATE TABLE IF NOT EXISTS destructive_audit_log (
    attempt_id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL,
    os_user TEXT,
    hostname TEXT,
    cwd TEXT,
    command TEXT NOT NULL,
    arguments TEXT,
    interactive_tty_present INTEGER NOT NULL CHECK (interactive_tty_present IN (0, 1)),
    scope_count INTEGER,
    scope_bytes INTEGER,
    protected_paths_affected INTEGER,
    result TEXT NOT NULL CHECK (result IN ('allowed', 'denied', 'locked_out', 'no_tty', 'blocked_agent')),
    failure_reason TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dal_timestamp ON destructive_audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_dal_result ON destructive_audit_log(result);
CREATE INDEX IF NOT EXISTS idx_dal_os_user ON destructive_audit_log(os_user);
```

## Archive Layout

```
<data_root>/
  db.sqlite3
  archive/
    <archive_id>/
      payload          # file content, or directory tree, or symlink
```

Content-addressed by immutable `archive_id`, not by filename or date. No path collisions. Simple restore by identity.

## Phase Guidance

**Phase 1**: All tables created. `archive_objects`, `batches`, `batch_items`, `restore_events`, `effective_policies` actively used. `policies` minimal (global defaults only). `mounts` populated opportunistically. `hash_jobs` created but only populated for cross-fs copies. `effective_policies` written at **batch level only** — one row per setting per batch; `archive_id` column left NULL. Per-object policy recording deferred to Phase 2.

**Phase 2**: `hash_jobs` actively processed by daemon. `policies` fully used with scoped rules. `mounts` used for per-mount archive roots. `effective_policies` extended to per-object recording.

**Phase 3**: `effective_policies` queried for behavior learning. `restore_events` analyzed for importance scoring.

---

# Appendix B: Spec 2 — Restore Semantics

## Core Rule

Restore is a **new operation**, not merely "undoing delete." Every restore:
1. Creates a restore batch (`batches` with `operation_type = 'restore'`)
2. Creates restore events (`restore_events` per object)
3. Resolves conflicts deterministically
4. Reports exact outcome per item

## Supported Restore Modes

### 1. Restore to original path (default)

```bash
smartrm restore <archive_id>
```

- Restore to `original_path` from archive metadata
- Recreate missing parent directories (inheriting process umask) unless `--no-create-parents`
- Attempt full metadata restoration after content placement

### 2. Restore to alternate path

```bash
smartrm restore <archive_id> --to /tmp/recovered/
```

- Restore to specified path
- Record as `alternate_path` mode in restore event
- Preserve original path in history

### 3. Restore batch

```bash
smartrm restore --batch <batch_id>
```

- Restore all `archived`-state objects from that delete batch
- Result is a new restore batch

### 4. Restore selected items from a batch

```bash
smartrm restore --batch <batch_id> --only path/to/file
```

- Subset restore is allowed
- Batch grouping is historical, not mandatory for full restore

### 5. Restore from partial batch

```bash
smartrm restore --batch <batch_id>
```

- If batch status is `partial`, restore only the objects that succeeded (state = `archived`)
- Failed objects from the original batch are not restorable

### 6. Restore last / Undo

```bash
smartrm undo
smartrm restore --last
```

- Find most recent delete batch by `started_at DESC`
- Restore all `archived`-state objects from that batch

## Conflict Policies

| Policy | Behavior | Flag |
|--------|----------|------|
| `fail` | Do not overwrite. Mark restore event failed. | `--conflict fail` (default non-interactive) |
| `rename` | Restore as `file (restored).txt` | `--conflict rename` (default interactive) |
| `overwrite` | Replace target atomically | `--conflict overwrite` or `--force` |
| `skip` | Skip conflicting item, continue | `--conflict skip` |

## Metadata Restoration Order

1. Place object content at destination
2. Restore `mode` (permissions)
3. Restore `mtime`/`ctime` timestamps
4. Attempt `uid`/`gid` restoration (warn if not root)

**Rules:**
- Failure to restore ownership does NOT fail the restore if content was placed
- Content placement failure IS a hard failure
- Restore event records: `mode_restored`, `ownership_restored`, `timestamps_restored`

## Object-Specific Rules

### Files
- Restore content → mode → timestamps → ownership

### Directories
- Restore directory container, then contents recursively (tree-preserving in Phase 1)
- If parent directory exists, contents are placed inside it (no merge in Phase 1)

### Symlinks
- Restore symlink itself, never follow target
- Broken symlinks restored as-is (not an error)
- `link_target` metadata is informational only

## Missing Parent Directories

If original parent directories do not exist:
- Create them unless `--no-create-parents` specified
- Created parents inherit normal process umask
- Note parent creation in restore event metadata

## Multi-Item Restore Behavior

- Continue on error
- Report summary at end
- Exit code 1 if any item failed

Example summary:
```
Restored 8/10 objects
  1 conflict (skipped)
  1 permission denied (ownership not restored, content OK)
```

## Restore Eligibility

| State | Restorable? |
|-------|-------------|
| `archived` | Yes |
| `expired` | Yes (still has content in archive) |
| `restored` | No (already restored; delete again to create new archive object) |
| `purged` | No (content deleted, only metadata remains) |
| `failed` | No (content was never archived) |

## Idempotency

Restore is NOT idempotent:
- Repeating restore to original path may conflict if file already exists
- Each attempt is recorded in `restore_events`
- Undo of same batch can only auto-run once safely unless conflict policy permits

## CLI Contract

**Commands:**
```bash
smartrm restore <archive_id>
smartrm restore --batch <batch_id>
smartrm restore --batch <batch_id> --only <path>
smartrm restore --last
smartrm restore --all
smartrm undo [N]
```

**Options:**
```
--to <path>              Restore to alternate location
--conflict <policy>      fail | rename | overwrite | skip
--force                  Shorthand for --conflict overwrite
--no-create-parents      Don't create missing parent directories
--json                   JSON output
```

**Exit codes:**
```
0  All items restored successfully
1  One or more failures
2  Nothing found / nothing restorable
```

---

# Appendix C: Spec 3 — Policy Precedence Rules

## Goal

Define how deletion and lifecycle behavior is chosen when multiple rules apply. Prevent "magic" behavior. Preserve user trust.

## Precedence Order

From highest to lowest priority:

| Priority | Source | Scope | Example |
|----------|--------|-------|---------|
| 0 | **Hard safety constraints** | Engine invariant | Cannot delete `/`, root always protected |
| 1 | **Explicit CLI flags** | This operation | `--ttl=7d`, `--permanent`, `--intent=temp` |
| 2 | **Interactive user choices** | This operation | User confirms overwrite at prompt |
| 3 | **User-defined rules** | User config | `protect .env`, `*.log ttl 3d` |
| 4 | **Project/path-scoped rules** | Path prefix or project | `/repo/build/** ttl 1d` |
| 5 | **System-wide defaults** | Installation | `/etc/smartrm/config.json` |
| 6 | **Learned recommendations** | Intelligence layer | "User never restores /tmp/ artifacts" |
| 7 | **Built-in fallback defaults** | Hardcoded | Safe defaults |

## Hard Safety Constraints

These cannot be overridden:
- Preserve root by default (no `--no-preserve-root` override in SmartRM)
- Block archive if disk space threshold violated (unless `--force`)
- Never follow symlinks for delete/archive semantics
- Ownership restoration degrades gracefully without privilege

These are engine invariants, not normal policy rules.

## Resolution Algorithm

For each delete/restore operation:

1. **Gather context**: target path, object type, mount, free space, user flags, cwd, interactive mode
2. **Collect applicable policies**: user config, path/project rules, system config, built-ins
3. **Sort by precedence**: using the order above
4. **Resolve per setting**: each setting is resolved independently through the chain

Settings resolved:
- `delete_mode` (archive vs permanent)
- `ttl_seconds`
- `delete_intent`
- `min_free_space_bytes`
- `restore_conflict_policy`
- `hashing_priority` (Phase 2)
- `compression_tier` (Phase 2)
- `auto_purge_eligibility` (Phase 2)

5. **Record effective policy**: store winning value, source type, and source reference in `effective_policies` table

## Explainability Requirement

The system must answer:
- "Why was this archived?"
- "Why did it expire after 7 days?"
- "Why was overwrite denied?"
- "Why was this protected?"

```bash
smartrm explain <archive_id>
# effective_ttl: 604800 seconds (7 days)
# source: CLI flag --ttl=7d
# overridden: user_rule (*.log ttl 3d), default (30d)

smartrm explain-policy /path/to/file
# delete_mode: archive (default)
# ttl: 259200 seconds (3 days) — user_rule: *.log ttl 3d
# intent: temp — classifier: extension .log
```

## Conflict Examples

### Example 1: CLI overrides user rule

User config: `*.log → ttl 3d`
CLI: `smartrm app.log --ttl=30d`
**Result**: effective TTL = 30d, source = `cli`, ref = `--ttl=30d`

### Example 2: System vs user config

System policy: `min_free_space 1GB`
User config: `min_free_space 500MB`
**Result**: user config wins (priority 3 > priority 5) — unless system marks it as hard safety constraint (priority 0)

### Example 3: Learned recommendation without opt-in

Learned: "user never restores /tmp/ build artifacts; suggest ttl 1d"
User has no rule.
**Result**: recommendation shown in `smartrm suggest` output. NOT auto-applied. Default TTL used.

### Example 4: Learned recommendation with opt-in

User config: `auto_optimization: true`
Learned: "user never restores /tmp/ build artifacts; suggest ttl 1d"
**Result**: TTL automatically set to 1d. Source recorded as `learned`.

## Learned Behavior Rules (Phase 3)

| Behavior | Default | With opt-in |
|----------|---------|-------------|
| Recommendations / suggestions | Allowed | Allowed |
| Warnings | Allowed | Allowed |
| Dashboard/timeline annotations | Allowed | Allowed |
| Silent TTL shortening | **Not allowed** | Allowed |
| Silent purge policy changes | **Not allowed** | Allowed |
| Silent compression tier changes | **Not allowed** | Allowed |
| Silent reclassification of protected | **Not allowed** | **Not allowed** (requires explicit rule) |

## Config Layering

Priority (runtime):
1. `SMARTRM_*` env var overrides
2. CLI flags
3. User config (`$XDG_CONFIG_HOME/smartrm/config.json`)
4. Project/path rules
5. System config (`/etc/smartrm/config.json`)
6. Built-in defaults

## Minimum Phase 1 Policy Settings

Even before the full policy engine, support these in config:

```json
{
  "default_delete_mode": "archive",
  "min_free_space_bytes": 1073741824,
  "default_restore_conflict_mode": "rename",
  "default_ttl_seconds": null,
  "protected_patterns": [".env", ".env.*"],
  "excluded_patterns": [],
  "danger_protection": true,
  "auto_cleanup": false
}
```

This gives a clean bridge into Phase 2 policy engine without redesign.

---

# Appendix D: State Transition Tables

## Archive Object States

| Current State | Event | Next State | Notes |
|---------------|-------|------------|-------|
| *(none)* | Archive succeeds | `archived` | Normal delete flow |
| *(none)* | Archive fails | `failed` | Row created if path resolved but move/copy failed |
| `archived` | Restore succeeds | `restored` | Restore event also written |
| `archived` | Restore partial (metadata only) | `restored` | Partial fidelity recorded in restore event |
| `archived` | TTL expiry marked | `expired` | Daemon or manual cleanup |
| `archived` | Cleanup/purge succeeds | `purged` | Terminal |
| `archived` | Cleanup fails | `archived` | Failure logged, state unchanged |
| `expired` | Restore succeeds | `restored` | Content still exists |
| `expired` | Purge succeeds | `purged` | Terminal |
| `restored` | Deleted again | *(new `archived`)* | New `archive_id` created |
| `failed` | *(terminal)* | — | Cannot transition |
| `purged` | *(terminal)* | — | Metadata retained, content gone |

## Batch States

| Current State | Event | Next State |
|---------------|-------|------------|
| `pending` | Operation begins | `in_progress` |
| `in_progress` | All items succeed | `complete` |
| `in_progress` | Some succeed, some fail | `partial` |
| `in_progress` | All items fail | `failed` |
| `in_progress` | Transactional rollback | `rolled_back` |

## Restore Event Outcomes

| Condition | Status |
|-----------|--------|
| Content restored, metadata fully restored | `succeeded` |
| Content restored, some metadata not restored | `succeeded` (with flags showing partial metadata) |
| Content placement failed | `failed` |
| Target conflict with `fail` policy | `failed` |
| Target conflict with `skip` policy | `failed` (item skipped, batch continues) |
| Target conflict with `rename` and rename succeeds | `succeeded` |
| Overwrite requested and succeeds | `succeeded` |

---

# Appendix E: JSON Output Contract

All subcommands with `--json` emit structured results.

## Delete Batch Result

```json
{
  "batch_id": "01JXYZ...",
  "operation_type": "delete",
  "status": "partial",
  "requested": 3,
  "succeeded": 2,
  "failed": 1,
  "items": [
    {
      "input_path": "a.txt",
      "status": "succeeded",
      "archive_id": "01JAAA..."
    },
    {
      "input_path": "b.txt",
      "status": "succeeded",
      "archive_id": "01JBBB..."
    },
    {
      "input_path": "c.txt",
      "status": "failed",
      "error_code": "permission_denied",
      "error_message": "Permission denied: c.txt"
    }
  ]
}
```

## Restore Result

```json
{
  "batch_id": "01JREST...",
  "operation_type": "restore",
  "status": "complete",
  "restored": 2,
  "items": [
    {
      "archive_id": "01JAAA...",
      "final_restored_path": "/home/user/a.txt",
      "status": "succeeded",
      "mode_restored": true,
      "ownership_restored": false,
      "timestamps_restored": true
    },
    {
      "archive_id": "01JBBB...",
      "final_restored_path": "/home/user/b.txt",
      "status": "succeeded",
      "mode_restored": true,
      "ownership_restored": true,
      "timestamps_restored": true
    }
  ]
}
```

## Explain Result

```json
{
  "archive_id": "01JAAA...",
  "effective_settings": [
    {
      "key": "delete_mode",
      "value": "archive",
      "source_type": "default",
      "source_ref": null
    },
    {
      "key": "ttl_seconds",
      "value": "604800",
      "source_type": "cli",
      "source_ref": "--ttl=7d"
    }
  ]
}
```

---

# Appendix F: Minimum Test Matrix

## Delete Tests
- Same-fs file delete
- Same-fs directory delete (tree-preserving)
- Cross-fs file delete (copy+delete+hash)
- Multiple file delete in single batch
- Symlink delete (archives link, not target)
- Broken symlink delete
- Low disk space block
- Permanent delete prompt and `--force` bypass
- Batch partial failure (continue + report)
- rm-compatible exit codes (0/1)
- Danger detection: blocked paths, warnings
- File classification tagging
- Effective policy recording

## Restore Tests
- Restore file to original path
- Restore file to alternate path (`--to`)
- Restore with conflict `fail`
- Restore with conflict `rename`
- Restore with conflict `overwrite`
- Restore with conflict `skip`
- Restore symlink (recreates link, not target)
- Restore broken symlink (not an error)
- Restore when parent directory missing (auto-create)
- Restore with ownership downgrade (warn, not fail)
- Undo last batch
- Undo last N batches
- Restore from partial batch (only succeeded items)
- Restore eligibility: archived=yes, expired=yes, purged=no, failed=no

## DB Tests
- WAL mode enabled
- Batch counters accurate after partial failure
- State transitions follow valid paths only
- Effective policies populated on delete
- Restore events created on every restore attempt
- Hash job inserted for cross-fs archives
- Foreign key constraints enforced

## CLI Tests
- All rm-compatible flags parsed correctly
- `--json` output on all subcommands
- Exit codes correct per spec
- Config layering: env > user > system > default
- Shell completion generation (bash, zsh, fish)

## Destructive Gate Tests
- Purge blocked when no TTY present (exit code 1, no prompt)
- Purge blocked when stdin is piped (`echo "phrase" | smartrm purge` fails)
- Purge blocked when `allow_destructive_commands = disabled`
- Scope preview displays before auth prompt (object count, size, protected count)
- Protected paths trigger elevated static phrase requirement (`PURGE PROTECTED`)
- Confirmation phrase / passphrase read from `/dev/tty`, not stdin
- Confirmation phrase is dynamically generated from scope (e.g. `PURGE 482 OBJECTS`)
- Wrong phrase rejected, attempt counter incremented
- `interactive_with_passphrase`: passphrase verified against stored argon2 hash
- Failed auth attempt logged to `destructive_audit_log`
- Successful approval logged to `destructive_audit_log`
- Cooldown enforced after 3 failures (30s lockout)
- Escalating cooldown on repeated lockouts
- Agent detection: `CI=true` env var blocks gate even with TTY
- Agent detection: `TERM=dumb` blocks gate
- Agent detection disabled via `"agent_detection": false` in config
- `--permanent` on protected paths subject to gate; on non-protected paths: y/N only
- `--permanent --force` bypasses y/N for non-protected paths, does NOT bypass gate
- Config `interactive_only` mode: y/N confirm only (no phrase or passphrase)
- Config `break_glass` mode enables `purge --all` path

## Lifecycle Scenario Tests
- delete → restore → delete again → restore second version (two archive_ids, verify version history shows both)
- delete → partial restore → undo
- delete → cleanup (purge) → attempt restore (must fail with clear error: content purged)
- multi-file delete → undo → verify all files restored at original paths
- delete → restore to alternate path → delete original again (new archive_id created)

## DB Error Path Tests
- busy timeout exhausted (SQLite WAL locked): propagate error, no FS move attempted
- disk full mid-transaction: transaction rolls back, file remains at original path
- FK constraint violation: reject insert, surface error to operation layer
- duplicate ULID: unique constraint on PRIMARY KEY catches it (should not happen; test the guard)
- commit failure after FS move: compensating action moves file back to original path, error surfaced

---

# Appendix G: Spec 4 — Destructive Command Gate

## Goal

Ensure irreversible commands can only be executed by a human operator in an interactive terminal session. No automation, agent, script, or piped input can bypass the gate.

## Gated Commands

### Simple Confirm (interactive y/N only — no gate)
- `smartrm --permanent <files>` on non-protected paths
  - `--force` suppresses the prompt for scripting on non-protected paths

### Standard Gate (TTY + scope preview + confirmation phrase or passphrase)
- `smartrm purge --expired`
- `smartrm --permanent <files>` on protected paths
- `smartrm restore --conflict overwrite` (destructive overwrite)
- `smartrm config set` that shortens retention or disables safeguards

### Elevated Gate (TTY + scope preview + static phrase + confirmation phrase or passphrase)
- `smartrm purge --all`
- `smartrm purge` when protected paths in scope
- Disabling archive mode globally
- Removing protected path rules

## Gate Verification Steps

```
1. Check allow_destructive_commands config
   - "disabled" → fail immediately
   - "interactive_only" → proceed to step 2, skip auth challenge (y/N confirm only)
   - "interactive_with_confirmation" → full gate (default)
   - "interactive_with_passphrase" → full gate with passphrase
   - "break_glass" → full gate + unlocks purge --all

2. Verify TTY
   - isatty(stdin) must be true
   - isatty(stderr) must be true
   - If false → fail: "Destructive command requires interactive terminal session"

3. Check agent markers (if agent_detection = true, the default)
   - CI=true, GITHUB_ACTIONS=true, JENKINS_URL set, TERM=dumb → block
   - stdin redirected (not a real terminal) → block
   - If detected → fail: "Destructive commands blocked in automation environment"

4. Generate scope preview
   - Query DB for affected objects
   - Calculate: count, total bytes, protected paths, age range
   - Select up to 5 representative examples
   - Display preview

5. If elevated gate required (protected paths > 0 or purge --all):
   - Display "Additional confirmation required"
   - Prompt: "Type: PURGE PROTECTED" (or appropriate static phrase)
   - Read from /dev/tty
   - Validate exact match (case-sensitive)

6. Auth challenge (read from /dev/tty, never stdin)
   - "interactive_with_confirmation": generate dynamic phrase from scope
     (e.g. "PURGE 482 OBJECTS"), prompt user to type it exactly
   - "interactive_with_passphrase": prompt for passphrase, verify against
     stored argon2 hash (set via `smartrm config set-passphrase`)
   - Open /dev/tty directly, disable echo, read input, re-enable echo

7. Validate input
   - On failure: increment attempt counter
     - If attempts < max (3): prompt again
     - If attempts >= max: cooldown, log lockout

8. Log attempt to destructive_audit_log
   - Always, regardless of outcome

9. Proceed with command
```

## Auth Input Source

Auth input MUST be read from `/dev/tty`:

```rust
// Correct
let tty = File::open("/dev/tty")?;
let input = read_secret_from(&tty)?;

// WRONG — an agent can pipe input
let input = read_secret_from(stdin)?;
```

On platforms without `/dev/tty` (some containers): command fails. No fallback.

## Agent Detection Heuristics

Block the gate (even with TTY) if any of these are true:

| Signal | Rationale |
|--------|-----------|
| `CI=true` | CI/CD environment |
| `GITHUB_ACTIONS=true` | GitHub Actions |
| `JENKINS_URL` set | Jenkins |
| `BUILDKITE=true` | Buildkite |
| `TERM=dumb` | Non-interactive terminal |
| `SMARTRM_AGENT_MODE=true` | Explicit agent flag |
| stdin is a pipe (not TTY) | Piped input |

These are heuristics, not security boundaries. The real protection is `/dev/tty` password read. Agent detection is an additional layer.

## Scope Preview Format

```
About to <action> <count> <scope> objects (<size>)
Scope: <filter description>
Protected paths affected: <count>
  <path 1>           (only if count > 0)
  <path 2>
  <path 3>
Age range: <oldest> to <newest>
Filters: <exact filters used>
Examples:
  <path a>
  <path b>
  <path c>

This action is irreversible.
```

## Cooldown State

Stored in-memory per process (not persisted across invocations for simplicity):

```
max_attempts_per_invocation: 3
cooldown_after_max_failures: 30s
escalation_multiplier: 2x per consecutive lockout in same session
max_lockout: 5 minutes
```

Cross-session cooldown is NOT enforced in Phase 1 (would require lockfile or DB state). Phase 2 daemon can enforce persistent cooldown.

## Config Integration

```json
{
  "allow_destructive_commands": "interactive_with_confirmation",
  "agent_detection": true
}
```

Resolved through standard policy precedence (section 12):
- `SMARTRM_ALLOW_DESTRUCTIVE` env var overrides config
- Setting `disabled` in CI config prevents all gated commands
- Cannot be overridden by CLI flags (this is a hard safety constraint)
- `agent_detection` can be set to `false` in known-safe non-interactive tooling; this does not disable TTY verification
