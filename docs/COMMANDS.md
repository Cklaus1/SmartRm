# SmartRM Command Reference

Full reference for all SmartRM commands and flags.

---

## smartrm (delete)

Archive files and directories.

### Synopsis

```
smartrm [FLAGS] <files...>
```

### Description

Archives the specified files and directories. By default, files are moved to the SmartRM archive and can be restored later. This is the default behavior when no subcommand is given.

### Options

| Flag | Description |
|------|-------------|
| `-r`, `-R`, `--recursive` | Remove directories and their contents recursively |
| `-f`, `--force` | Ignore nonexistent files and arguments, never prompt |
| `-i` | Prompt before every removal |
| `-I` | Prompt once before removing more than three files, or when removing recursively |
| `-d`, `--dir` | Remove empty directories |
| `-v`, `--verbose` | Explain what is being done |
| `--preserve-root` | Do not remove `/` (default: true) |
| `--no-preserve-root` | Do not treat `/` specially |
| `--one-file-system` | Do not cross filesystem boundaries |
| `--permanent` | Permanently delete, bypassing the archive |
| `--yes-i-am-sure` | Override dangerous operation warnings |
| `--json` | Output in JSON format |

### Examples

```bash
# Archive a single file
smartrm file.txt

# Archive a directory recursively
smartrm -r build/

# Archive multiple files, verbose
smartrm -v *.log data.csv

# Force-archive, ignoring missing files
smartrm -f maybe-exists.txt definitely-exists.txt

# Permanently delete (bypasses archive, requires gate confirmation)
smartrm --permanent temp.dat
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All files archived successfully |
| 1 | One or more files failed to archive |

---

## undo

Restore the most recent delete batches.

### Synopsis

```
smartrm undo [count] [--conflict <policy>]
```

### Description

Restores files from the last N delete batches (default: 1). Batches are identified by reverse chronological order. This is the quick-undo for "I just deleted the wrong thing."

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `count` | `1` | Number of batches to undo |
| `--conflict <policy>` | `rename` | Conflict resolution: `fail`, `rename`, `overwrite`, `skip` |
| `--json` | -- | Output in JSON format |

### Examples

```bash
# Undo the last delete
smartrm undo

# Undo the last 3 deletes
smartrm undo 3

# Undo, overwriting any files that now exist at the original paths
smartrm undo --conflict overwrite
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All files restored successfully |
| 1 | Restore failed or no batches to undo |

---

## restore

Restore archived files with fine-grained control.

### Synopsis

```
smartrm restore [archive_id] [OPTIONS]
```

### Description

Restores one or more archived files. Supports restoring by individual archive ID, batch ID, the most recent deletion, or all archives. Files are restored to their original location by default, or to an alternate path with `--to`.

### Options

| Option | Description |
|--------|-------------|
| `archive_id` | Archive ID to restore (first 8+ characters of ULID) |
| `--batch <id>` | Restore all objects from a specific batch |
| `--last` | Restore the most recent deletion |
| `--all` | Restore all archived files |
| `--only <path>` | Restore only this path from a batch |
| `--to <path>` | Restore to an alternate location |
| `--conflict <policy>` | Conflict resolution: `fail`, `rename`, `overwrite`, `skip` (default: `rename`) |
| `-f`, `--force` | Shorthand for `--conflict overwrite` |
| `--no-create-parents` | Do not create missing parent directories |
| `--json` | Output in JSON format |

### Examples

```bash
# Restore by archive ID (prefix match)
smartrm restore 01JQXYZ1

# Restore the most recent deletion
smartrm restore --last

# Restore all files from a batch
smartrm restore --batch 01JQXYZ0

# Restore to a different directory
smartrm restore 01JQXYZ1 --to /tmp/recovered/

# Force-overwrite existing files
smartrm restore --last -f

# Restore everything
smartrm restore --all --conflict skip
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Files restored successfully |
| 1 | Restore failed |
| 2 | No matching archives found |

---

## list

List archived files.

### Synopsis

```
smartrm list [--state <state>] [--limit <n>] [--cursor <ulid>]
```

### Description

Lists archive objects with their ID, state, size, original path, and archive timestamp. Supports filtering by lifecycle state and keyset pagination for large archives.

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--state <state>` | all | Filter by lifecycle state: `archived`, `restored`, `expired`, `purged`, `failed` |
| `--limit <n>` | `50` | Maximum number of results |
| `--cursor <ulid>` | -- | ULID of last seen item for keyset pagination |
| `--json` | -- | Output in JSON format |

### Examples

```bash
# List all archives
smartrm list

# List only archived (not yet expired) files
smartrm list --state archived

# Paginate through results
smartrm list --limit 20
smartrm list --limit 20 --cursor 01JQXYZ100
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Results found |
| 2 | No results |

---

## search

Search archived files by pattern.

### Synopsis

```
smartrm search <pattern> [OPTIONS]
```

### Description

Searches archive objects by filename pattern. If the pattern contains `*` or `?`, it is treated as a glob; otherwise it is a substring match. Results can be filtered by date, minimum size, and original directory.

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `pattern` | required | Glob pattern (if contains `*` or `?`) or substring match |
| `--after <date>` | -- | Show only archives created after this date (ISO 8601) |
| `--larger-than <size>` | -- | Minimum file size (e.g., `10M`, `1G`) |
| `--dir <path>` | -- | Filter by original directory |
| `--limit <n>` | `50` | Maximum results |
| `--offset <n>` | `0` | Skip first N results |
| `--json` | -- | Output in JSON format |

### Examples

```bash
# Search by glob
smartrm search "*.log"

# Search by substring
smartrm search "config"

# Find large deleted files
smartrm search "*" --larger-than 100M

# Search within a directory, after a date
smartrm search "*.rs" --dir /home/user/project --after 2026-03-01
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Results found |
| 2 | No results |

---

## history

Show version history for a file path.

### Synopsis

```
smartrm history <path>
```

### Description

Shows all archive events for a specific file path, ordered chronologically. Useful for seeing how many times a file has been deleted and restored.

### Options

| Option | Description |
|--------|-------------|
| `path` | File path to show history for |
| `--json` | Output in JSON format |

### Examples

```bash
# Show history for a file
smartrm history /home/user/project/config.json

# JSON output
smartrm history /home/user/.env --json
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | History found |
| 2 | No history for this path |

---

## timeline

Show chronological batch history.

### Synopsis

```
smartrm timeline [--today] [--dir <path>] [--limit <n>]
```

### Description

Shows a chronological view of delete/restore batches. Each entry shows the batch ID, operation type, file count, and timestamp.

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--today` | -- | Show only today's activity |
| `--dir <path>` | -- | Filter by directory |
| `--limit <n>` | `20` | Maximum number of results |
| `--json` | -- | Output in JSON format |

### Examples

```bash
# Show recent activity
smartrm timeline

# Today only
smartrm timeline --today

# Activity in a specific directory
smartrm timeline --dir /home/user/project --limit 50
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Results found |
| 2 | No activity |

---

## cleanup

Remove old archives.

### Synopsis

```
smartrm cleanup [--older-than <duration>] [--expired] [--dry-run] [-f]
```

### Description

Removes archive objects based on age or expiry state. Permanently deletes the archived files from disk and marks them as purged in the database. Use `--dry-run` to preview what would be removed.

### Options

| Option | Description |
|--------|-------------|
| `--older-than <duration>` | Remove archives older than this duration (e.g., `30d`, `7d`, `24h`) |
| `--expired` | Only remove expired-state objects |
| `--dry-run` | Preview what would be cleaned without deleting |
| `-f`, `--force` | Force cleanup of protected files (requires gate confirmation) |
| `--json` | Output in JSON format |

### Examples

```bash
# Preview what would be cleaned
smartrm cleanup --older-than 30d --dry-run

# Clean archives older than 30 days
smartrm cleanup --older-than 30d

# Clean only expired objects
smartrm cleanup --expired

# Force-clean including protected files
smartrm cleanup --older-than 7d -f
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Cleanup completed |
| 1 | Cleanup failed |
| 2 | Nothing to clean |

---

## purge

Permanently delete archive data.

### Synopsis

```
smartrm purge [--expired] [--all] [-f]
```

### Description

Permanently removes archive objects and their stored data. This is irreversible. Intended for reclaiming disk space or uninstalling SmartRM. Requires gate confirmation unless `-f` is used.

### Options

| Option | Description |
|--------|-------------|
| `--expired` | Only purge expired objects |
| `--all` | Purge everything (full archive wipe) |
| `-f`, `--force` | Skip confirmation prompt |
| `--json` | Output in JSON format |

### Examples

```bash
# Purge expired archives
smartrm purge --expired

# Purge everything (uninstall scenario)
smartrm purge --all

# Force purge without confirmation
smartrm purge --all -f
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Purge completed |
| 1 | Purge failed |
| 2 | Nothing to purge |

---

## stats

Show archive statistics.

### Synopsis

```
smartrm stats
```

### Description

Displays summary statistics about the archive: total objects by state, total size, disk usage, and object counts.

### Options

| Option | Description |
|--------|-------------|
| `--json` | Output in JSON format |

### Examples

```bash
smartrm stats
smartrm stats --json
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |

---

## config

View or modify configuration.

### Synopsis

```
smartrm config
smartrm config set <key> <value>
smartrm config set-passphrase
```

### Description

With no arguments, displays the current effective configuration as JSON. `config set` writes a key-value pair to the user config file. `config set-passphrase` interactively sets the passphrase for the destructive command gate.

### Options

| Subcommand | Description |
|------------|-------------|
| (none) | Display current config |
| `set <key> <value>` | Set a configuration value |
| `set-passphrase` | Interactively set the gate passphrase |

### Examples

```bash
# View config
smartrm config

# Set default TTL to 30 days
smartrm config set default_ttl_seconds 2592000

# Enable auto-cleanup
smartrm config set auto_cleanup true

# Set up passphrase for destructive operations
smartrm config set-passphrase
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Invalid key or write failure |

---

## completions

Generate shell completions.

### Synopsis

```
smartrm completions <shell>
```

### Description

Generates shell completion scripts for the specified shell. Output is written to stdout; redirect to the appropriate completions directory.

### Options

| Option | Description |
|--------|-------------|
| `shell` | Shell to generate for: `bash`, `zsh`, `fish`, `elvish`, `powershell` |

### Examples

```bash
# Bash
smartrm completions bash > /etc/bash_completion.d/smartrm

# Zsh
smartrm completions zsh > ~/.zfunc/_smartrm

# Fish
smartrm completions fish > ~/.config/fish/completions/smartrm.fish
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |

---

## explain

Explain why an archive object was archived.

### Synopsis

```
smartrm explain <archive_id>
```

### Description

Shows the full policy trace for an archived object: which settings applied, where each came from (CLI flag, user config, system default, etc.), file classification tags, and danger level assessment.

### Options

| Option | Description |
|--------|-------------|
| `archive_id` | Archive ID to explain |
| `--json` | Output in JSON format |

### Examples

```bash
# Explain why a file was archived
smartrm explain 01JQXYZ123

# JSON output for scripting
smartrm explain 01JQXYZ123 --json
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Object found |
| 1 | Error |
| 2 | Object not found |

---

## explain-policy

Show what policy would apply to a path.

### Synopsis

```
smartrm explain-policy <path>
```

### Description

Without deleting anything, shows what classification, danger level, and policy settings would apply if the given path were deleted. Useful for understanding SmartRM's behavior before running a command.

### Options

| Option | Description |
|--------|-------------|
| `path` | File path to check |
| `--json` | Output in JSON format |

### Examples

```bash
# Check what would happen to a file
smartrm explain-policy /home/user/.env

# Check a directory
smartrm explain-policy /home/user/project/node_modules
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error |
