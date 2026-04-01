#!/usr/bin/env bash
#
# SmartRM Install/Uninstall Script
#
# Replaces system rm with smartrm, archives the original rm binary.
# Fully reversible via --uninstall.
#
# Usage:
#   sudo ./install.sh              # Install: replace rm with smartrm
#   sudo ./install.sh --uninstall  # Uninstall: restore original rm
#   sudo ./install.sh --status     # Show current state
#
set -euo pipefail

SMARTRM_BIN="$(cd "$(dirname "$0")" && pwd)/target/release/smartrm"
BACKUP_DIR="/usr/local/share/smartrm-backup"
BACKUP_MANIFEST="$BACKUP_DIR/manifest.txt"

# Colors (if terminal)
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    NC='\033[0m'
else
    RED='' GREEN='' YELLOW='' NC=''
fi

die() { echo -e "${RED}error: $1${NC}" >&2; exit 1; }
info() { echo -e "${GREEN}$1${NC}"; }
warn() { echo -e "${YELLOW}$1${NC}"; }

require_root() {
    if [ "$(id -u)" -ne 0 ]; then
        die "this script must be run as root (sudo ./install.sh)"
    fi
}

# Find the canonical rm binary (handling hardlinks and symlinks)
# Returns a single path — the "real" rm binary.
# Other paths (hardlinks, symlinks) are recorded separately for replacement.
find_rm_paths() {
    # Collect all paths where rm exists — check everywhere
    local all_paths=()

    # 1. Known system locations
    local known_dirs=(
        /usr/bin /bin /usr/local/bin /sbin /usr/sbin
        /snap/bin
        /opt/homebrew/bin                                    # macOS Homebrew
        /usr/local/opt/coreutils/libexec/gnubin              # macOS GNU coreutils
        /opt/local/bin                                       # MacPorts
        /nix/var/nix/profiles/default/bin                    # NixOS
        /run/current-system/sw/bin                           # NixOS
        /gnu/store/*/bin                                     # Guix (glob won't expand, handled below)
    )

    # 2. Also scan every directory in PATH
    IFS=':' read -ra path_dirs <<< "${PATH:-}"
    for d in "${path_dirs[@]}"; do
        known_dirs+=("$d")
    done

    # 3. Also check what `which -a` finds
    local which_results
    which_results=$(which -a rm 2>/dev/null || true)

    # Deduplicate and collect
    local seen_paths=()
    for candidate in "${known_dirs[@]}"; do
        local p="$candidate/rm"
        # Handle globs (e.g., /gnu/store/*/bin/rm)
        for expanded in $p; do
            [ -e "$expanded" ] || continue

            # Resolve to canonical path for dedup
            local canonical
            canonical=$(readlink -f "$expanded" 2>/dev/null || echo "$expanded")

            # Check if already seen (by resolved path OR original path)
            local is_seen=false
            for s in "${seen_paths[@]:-}"; do
                if [ "$s" = "$expanded" ] || [ "$s" = "$canonical" ]; then
                    is_seen=true
                    break
                fi
            done
            $is_seen && continue

            # Skip if already smartrm
            if "$expanded" --version 2>&1 | grep -q "smartrm"; then
                continue
            fi

            all_paths+=("$expanded")
            seen_paths+=("$expanded" "$canonical")
        done
    done

    # Also add anything from which -a that we missed
    for w in $which_results; do
        local is_seen=false
        for s in "${seen_paths[@]:-}"; do
            if [ "$s" = "$w" ]; then
                is_seen=true
                break
            fi
        done
        if ! $is_seen; then
            if ! "$w" --version 2>&1 | grep -q "smartrm"; then
                all_paths+=("$w")
                seen_paths+=("$w")
            fi
        fi
    done

    if [ ${#all_paths[@]} -eq 0 ]; then
        return
    fi

    # Find unique inodes to detect hardlinks
    local seen_inodes=()
    local primary=""
    local secondary=()

    for p in "${all_paths[@]}"; do
        local inode
        inode=$(stat -c '%i' "$p" 2>/dev/null || stat -f '%i' "$p" 2>/dev/null)

        local is_dup=false
        for seen in "${seen_inodes[@]:-}"; do
            if [ "$seen" = "$inode" ]; then
                is_dup=true
                break
            fi
        done

        if $is_dup; then
            secondary+=("$p")
        else
            seen_inodes+=("$inode")
            if [ -z "$primary" ]; then
                primary="$p"
            else
                secondary+=("$p")
            fi
        fi
    done

    # Output: first line is primary (to backup), subsequent are secondary (just replace)
    echo "$primary"
    for s in "${secondary[@]:-}"; do
        [ -n "$s" ] && echo "$s"
    done
}

do_status() {
    echo "SmartRM Installation Status"
    echo "============================"
    echo ""

    # Check if smartrm binary exists
    if [ -f "$SMARTRM_BIN" ]; then
        info "smartrm binary: $SMARTRM_BIN ($(ls -lh "$SMARTRM_BIN" | awk '{print $5}'))"
    else
        warn "smartrm binary: not built (run 'cargo build --release' first)"
    fi

    # Check backup
    if [ -d "$BACKUP_DIR" ] && [ -f "$BACKUP_MANIFEST" ]; then
        info "backup directory: $BACKUP_DIR"
        echo "  backed up binaries:"
        while IFS='|' read -r original_path backup_path hash; do
            if [ -f "$backup_path" ]; then
                echo "    $original_path -> $backup_path (sha256: ${hash:0:16}...)"
            else
                warn "    $original_path -> $backup_path (MISSING!)"
            fi
        done < "$BACKUP_MANIFEST"
    else
        echo "  no backup found (original rm not replaced)"
    fi

    echo ""

    # Check current rm — scan all locations
    echo "  rm binaries found:"
    local found_any=false
    for rm_path in $(which -a rm 2>/dev/null); do
        found_any=true
        local version
        version=$("$rm_path" --version 2>&1 | head -1 || echo "unknown")
        if echo "$version" | grep -q "smartrm"; then
            info "    $rm_path -> smartrm ($version)"
        else
            echo "    $rm_path -> system rm ($version)"
        fi
    done
    if ! $found_any; then
        warn "    no rm found in PATH"
    fi
}

do_install() {
    require_root

    # Verify smartrm binary exists
    if [ ! -f "$SMARTRM_BIN" ]; then
        die "smartrm binary not found at $SMARTRM_BIN\nRun 'cargo build --release' first"
    fi

    # Verify it works
    if ! "$SMARTRM_BIN" --version >/dev/null 2>&1; then
        die "smartrm binary at $SMARTRM_BIN is not executable or broken"
    fi

    local smartrm_version
    smartrm_version=$("$SMARTRM_BIN" --version 2>&1)
    info "installing $smartrm_version"

    # Check if already installed
    if [ -f "$BACKUP_MANIFEST" ]; then
        die "smartrm is already installed (backup exists at $BACKUP_DIR)\nRun --uninstall first, or --status to check"
    fi

    # Create backup directory
    mkdir -p "$BACKUP_DIR"

    # Find rm paths (handles hardlinks)
    local rm_paths
    rm_paths=$(find_rm_paths)

    if [ -z "$rm_paths" ]; then
        die "no rm binary found on system"
    fi

    # First line is the primary (to backup), rest are secondary (hardlinks/duplicates)
    local primary_rm
    primary_rm=$(echo "$rm_paths" | head -1)
    local secondary_rms
    secondary_rms=$(echo "$rm_paths" | tail -n +2)

    local rm_version
    rm_version=$("$primary_rm" --version 2>&1 | head -1 || echo "unknown")

    echo ""
    echo "Primary rm binary to backup:"
    echo "  $primary_rm ($rm_version)"
    if [ -n "$secondary_rms" ]; then
        echo ""
        echo "Additional rm paths (hardlinks/duplicates, will also be replaced):"
        for s in $secondary_rms; do
            echo "  $s"
        done
    fi
    echo ""
    echo "Original binary will be backed up to: $BACKUP_DIR"
    echo "Original will be chmod 000 (no access) but preserved."
    echo ""

    # Confirm
    read -p "Proceed? [y/N] " -r
    if [[ ! "$REPLY" =~ ^[Yy]$ ]]; then
        echo "Aborted."
        exit 0
    fi

    # Backup the primary rm binary only (secondary are hardlinks to the same file)
    > "$BACKUP_MANIFEST"

    local backup_path="$BACKUP_DIR/rm.original"
    local hash
    hash=$(sha256sum "$primary_rm" | awk '{print $1}')

    cp -a "$primary_rm" "$backup_path"
    chmod 000 "$backup_path"
    info "backed up: $primary_rm -> $backup_path (sha256: ${hash:0:16}...)"

    # Record all paths in manifest (primary + secondary), single backup file
    echo "${primary_rm}|${backup_path}|${hash}" >> "$BACKUP_MANIFEST"
    for s in $secondary_rms; do
        [ -n "$s" ] && echo "${s}|${backup_path}|${hash}" >> "$BACKUP_MANIFEST"
    done

    # Replace all rm paths with smartrm
    for rm_path in $primary_rm $secondary_rms; do
        [ -z "$rm_path" ] && continue
        cp "$SMARTRM_BIN" "$rm_path"
        chmod 755 "$rm_path"
        info "replaced: $rm_path -> smartrm"
    done

    # Also install smartrm under its own name
    if [ ! -f /usr/local/bin/smartrm ]; then
        cp "$SMARTRM_BIN" /usr/local/bin/smartrm
        chmod 755 /usr/local/bin/smartrm
        info "installed: /usr/local/bin/smartrm"
    fi

    echo ""
    info "installation complete"
    echo ""
    echo "  rm is now smartrm (archives instead of deletes)"
    echo "  original rm backed up to $BACKUP_DIR (chmod 000)"
    echo "  to restore original rm: sudo ./install.sh --uninstall"
    echo ""
    echo "  test it:"
    echo "    echo test > /tmp/test.txt && rm /tmp/test.txt && rm list"
}

do_uninstall() {
    require_root

    if [ ! -f "$BACKUP_MANIFEST" ]; then
        die "no backup manifest found at $BACKUP_MANIFEST\nsmartRM does not appear to be installed via this script"
    fi

    echo "Restoring original rm binaries:"
    echo ""

    while IFS='|' read -r original_path backup_path expected_hash; do
        if [ ! -f "$backup_path" ]; then
            warn "  SKIP: backup missing for $original_path ($backup_path)"
            continue
        fi

        # Restore permissions on backup so we can read it
        chmod 755 "$backup_path"

        # Verify backup integrity
        local actual_hash
        actual_hash=$(sha256sum "$backup_path" | awk '{print $1}')
        if [ "$actual_hash" != "$expected_hash" ]; then
            warn "  WARNING: backup hash mismatch for $original_path"
            warn "    expected: $expected_hash"
            warn "    actual:   $actual_hash"
            read -p "  Restore anyway? [y/N] " -r
            if [[ ! "$REPLY" =~ ^[Yy]$ ]]; then
                warn "  SKIP: $original_path"
                continue
            fi
        fi

        # Restore original
        cp -a "$backup_path" "$original_path"
        info "  restored: $original_path (sha256: ${expected_hash:0:16}...)"

    done < "$BACKUP_MANIFEST"

    # Clean up backup directory
    rm -rf "$BACKUP_DIR"
    info "  removed backup directory: $BACKUP_DIR"

    # Keep /usr/local/bin/smartrm (it's still useful as smartrm)
    if [ -f /usr/local/bin/smartrm ]; then
        echo ""
        echo "  /usr/local/bin/smartrm is still available (use 'smartrm' command directly)"
        echo "  to fully remove: sudo rm /usr/local/bin/smartrm"
    fi

    echo ""
    info "uninstall complete — original rm restored"
}

# Main dispatch
case "${1:-}" in
    --uninstall|-u)
        do_uninstall
        ;;
    --status|-s)
        do_status
        ;;
    --help|-h)
        echo "Usage: sudo ./install.sh [--uninstall|--status|--help]"
        echo ""
        echo "  (no args)     Install: replace system rm with smartrm"
        echo "  --uninstall   Restore original rm from backup"
        echo "  --status      Show current installation state"
        echo "  --help        Show this help"
        ;;
    "")
        do_install
        ;;
    *)
        die "unknown option: $1 (try --help)"
        ;;
esac
