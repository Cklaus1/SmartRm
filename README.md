# SmartRM

**Your AI agent has full shell access. One bad `rm -rf` and your work is gone forever.**

SmartRM makes every delete reversible — and makes permanent destruction impossible without a human at the keyboard.

![SmartRM Competitive Matrix](docs/assets/competitive-matrix.png)

## What It Does

You use `rm` every day. So does your AI agent. The difference: when an agent runs `rm -rf` on the wrong directory at 3am, there's no undo. SmartRM fixes that.

```
rm file.txt          # safely archived (reversible)
rm -rf project/      # safely archived (reversible)
rm undo              # everything back, instantly
```

Normal deletes become safe archives. You can undo any delete, search your history, restore specific versions. Your workflow doesn't change — `rm` still works exactly the same, it just stops being permanent.

## What It Blocks

Every destructive path requires a human at a real terminal:

```
rm --permanent       # BLOCKED — requires TTY + confirmation
rm purge             # BLOCKED — requires TTY + confirmation
rm cleanup           # BLOCKED — requires TTY + confirmation
```

No flag bypasses this. No script. No AI agent. No piped input. The only way to permanently destroy files is a human typing a confirmation phrase at a physical terminal.

## Why This Exists

AI coding agents (Claude Code, Cursor, Copilot, custom toolchains) run shell commands with your full filesystem permissions. They call `/usr/bin/rm` directly — shell aliases don't protect you. A hallucinating agent, a bad prompt, or a logic error in an autonomous loop can permanently destroy:

- Your source code
- Your `.env` files and credentials
- Your git history
- Your entire home directory

SmartRM replaces `rm` at the system level so that every process — human or agent — goes through the same safety layer.

## 8-Layer Agent Protection

SmartRM's destructive command gate is designed so that no automation can bypass it:

| Layer | What it does |
|-------|-------------|
| **TTY required** | stdin and stderr must be a real terminal |
| **`/dev/tty` read** | Confirmation read from terminal device, not stdin — `echo "yes" \| rm purge` fails |
| **Agent detection** | Blocks CI/automation environments (CI, GITHUB_ACTIONS, JENKINS_URL, TERM=dumb) |
| **Scope preview** | Shows exactly what will be destroyed before prompting |
| **Confirmation phrase** | Must type a dynamic phrase like `PURGE 12 OBJECTS` — can't be scripted |
| **Passphrase option** | Optional argon2-hashed passphrase for higher security |
| **Elevated tier** | Protected files (.env, keys) require typing `PURGE PROTECTED` |
| **Audit log** | Every attempt (allowed or denied) logged with timestamp, user, scope |

## Key Features

### Every Delete is Undoable

```
$ rm important.txt
$ rm undo
restored '/home/user/important.txt'
```

Undo the last delete, the last 5 deletes, or restore any specific file by ID. Restore to the original location or a different path. Handle conflicts automatically (rename, overwrite, skip, or fail).

### Search and Find Anything You Deleted

```
$ rm search "*.env"
01ABCDEF   /home/user/project/.env     2 days ago   128 B  archived

$ rm search --after 2026-03-01 --larger-than 1M
01BCDEFG   /home/user/database.sql     1 week ago   4.2 MB  archived
```

Search by name (glob or substring), filter by date, size, or directory. Every deleted file is indexed in a local SQLite database.

### Version History

Delete the same file multiple times? Every version is kept separately:

```
$ rm history config.yaml
#  ID         State      Deleted              Size
1  01ABCDEF   archived   2026-04-01 10:00     1.0 KB
2  01BCDEFG   restored   2026-03-28 14:22     896 B
3  01CDEFGH   archived   2026-03-25 09:15     750 B
```

Restore any version by ID.

### Danger Detection

SmartRM classifies every path before acting:

- `rm -rf /` — **hard blocked**, no override, ever
- `rm -rf ~` — **warning**, requires `--yes-i-am-sure`
- `rm -rf .git` — **warning**, "this will archive your git history"
- `.ssh`, `.gnupg`, system paths — **warning**

### Protected File Classification

Files are automatically tagged. Protected files (.env, credentials, keys, certificates) get stronger safety requirements:

- `.env`, `.env.local`, `.env.production` — tagged `protected`
- `*.pem`, `*.key`, `*.p12` — tagged `protected`
- Files with `credential`, `secret`, `token` in the name — tagged `protected`

Protected files require the elevated gate tier for permanent destruction.

### Disk Space Protection

SmartRM checks available disk space before every archive. If archiving would drop free space below 1 GB (configurable), it blocks the operation with an actionable suggestion:

```
Archive disk 96% full (1.2 GB free).
Run 'smartrm cleanup --older-than 7d' to free ~4.3 GB.
```

### Timeline — Git Log for Your Filesystem

```
$ rm timeline
Batch      Type     Status    Files   Size     Started
01ABCDEF   delete   complete  3       4.2 KB   10 minutes ago
01BCDEFG   restore  complete  1       1.0 KB   15 minutes ago
01CDEFGH   delete   partial   5       12 MB    1 hour ago
```

Every operation is tracked. See what happened, when, and how many files were affected.

### Works Exactly Like rm

All standard flags work: `-r` `-R` `-f` `-i` `-I` `-d` `-v` `--preserve-root` `--one-file-system` `--`

Your muscle memory, your scripts, your Makefiles — everything works the same. SmartRM just makes it reversible.

## Install

### Build

```bash
cargo build --release    # 4.4 MB binary, zero dependencies
```

### System-Wide Replacement (recommended)

Replace the actual `/usr/bin/rm` so all processes (including agents) use SmartRM:

```bash
sudo ./install.sh              # replace rm with smartrm
sudo ./install.sh --status     # check what's installed
sudo ./install.sh --uninstall  # restore original rm (SHA-256 verified)
```

The install script finds every `rm` binary on your system, backs up the originals with SHA-256 hashes, and replaces them. Fully reversible.

### Shell Alias (lightweight alternative)

```bash
echo 'alias rm="smartrm"' >> ~/.bashrc
```

Protects your interactive sessions but not agents calling `/usr/bin/rm` directly.

### Lock Down for Agent Environments

```bash
# Disable all destructive commands entirely
smartrm config set allow_destructive_commands disabled

# Or require passphrase (stronger than confirmation phrase)
smartrm config set destructive_gate_method passphrase
smartrm config set-passphrase
```

---

## Command Reference

### Delete and Restore

| Command | Description |
|---------|-------------|
| `smartrm <files...>` | Archive files (safe delete) |
| `smartrm -r <dir>` | Archive directory recursively |
| `smartrm undo [N]` | Restore last N batches (default: 1) |
| `smartrm restore <id>` | Restore by archive ID (8+ char prefix) |
| `smartrm restore --batch <id>` | Restore entire batch |
| `smartrm restore --all` | Restore everything |
| `smartrm restore <id> --to <path>` | Restore to alternate location |
| `smartrm restore --conflict <policy>` | fail, rename, overwrite, or skip |

### Query

| Command | Description |
|---------|-------------|
| `smartrm list [--state X]` | List archives with pagination |
| `smartrm search <pattern>` | Glob or substring search with filters |
| `smartrm history <path>` | Version history for a file path |
| `smartrm timeline [--today]` | Chronological operation history |
| `smartrm stats` | Archive size, counts, top directories |
| `smartrm explain <id>` | Why was this archived? What policy? |
| `smartrm explain-policy <path>` | What would happen to this path? |

### Maintenance (requires human TTY)

| Command | Gate |
|---------|------|
| `smartrm cleanup --older-than 30d` | Confirmation phrase |
| `smartrm cleanup --dry-run` | None (preview only) |
| `smartrm purge --expired` | Confirmation phrase |
| `smartrm purge --all` | Elevated (type "PURGE PROTECTED" + phrase) |
| `smartrm --permanent <files>` | y/N confirm, or full gate if protected |

### Config and Shell

```bash
smartrm config                        # show all settings
smartrm config set <key> <value>      # change a setting
smartrm config set-passphrase         # set passphrase for gate
smartrm completions bash|zsh|fish     # shell completions
```

## Configuration

SmartRM works with zero configuration. All settings have sensible defaults.

| Key | Default | Description |
|-----|---------|-------------|
| `allow_destructive_commands` | `interactive_with_confirmation` | `disabled`, `interactive_only`, `interactive_with_confirmation` |
| `destructive_gate_method` | `confirmation_phrase` | `confirmation_phrase` or `passphrase` |
| `agent_detection` | `true` | Block destructive commands in CI/automation |
| `danger_protection` | `true` | Enable path danger classification |
| `min_free_space_bytes` | 1 GB | Minimum free disk space to maintain |
| `default_restore_conflict_mode` | `rename` | `fail`, `rename`, `overwrite`, `skip` |
| `protected_patterns` | `.env, .env.*` | Glob patterns for protected files |
| `default_ttl_seconds` | none | Auto-expire archives after this duration |
| `auto_cleanup` | `false` | Automatically clean expired archives |

Config locations (checked in order): `$SMARTRM_HOME/config.json`, `~/.config/smartrm/config.json`, `/etc/smartrm/config.json`

---

## Technical Details

### Architecture

Single static Rust binary. No runtime dependencies. 4.4 MB release build.

- **SQLite** (WAL mode) -- 10 tables tracking archive objects, batches, restore events, policies, audit logs
- **ULID primary keys** -- time-sortable, 8-char prefix in CLI, full ID in `--json`
- **Per-file transactions** with compensating rollback -- no partial state on failures
- **Filesystem trait** -- all I/O abstracted for testability
- **GateEnvironment trait** -- TTY/env abstracted for gate testing

### File Lifecycle

```
ACTIVE  -->  ARCHIVED  -->  EXPIRED  -->  PURGED (terminal)
                |               |
                +-- RESTORED    +-- RESTORED
```

### JSON Output

All commands support `--json` for machine-readable output and integration with other tools.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error (I/O, database, permission, gate denied) |
| 2 | Nothing found (list, search, history returned no results) |

### Development

```bash
cargo build                      # debug build
cargo test                       # 347 tests
cargo test --test e2e_cli        # E2E CLI tests (spawns binary)
cargo test --test user_journeys  # real-world scenario tests
cargo build --release            # release build
```

347 tests across 5 layers: unit, integration (DB + lifecycle), E2E CLI, and user journey stories.

## License

CC BY-NC 4.0 (Creative Commons Attribution-NonCommercial 4.0 International)
