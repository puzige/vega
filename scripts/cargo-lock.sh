#!/bin/sh
# Serialize cargo build/test across every worktree of this repository.
#
# WHY THIS EXISTS
#
# All worktrees of this repo share one `target/` directory (see the
# "Shared build cache" section in AGENTS.md). Cargo takes an exclusive lock
# for the *compile* phase, but it releases that lock before running test
# binaries. Two worktrees can therefore have their test suites executing at
# the same time, which produced two real failure modes:
#
#   1. A crate artifact built from one worktree's sources is reused by
#      another whose sources differ, producing compile errors that vanish
#      on a plain rebuild (observed: E0599 on a constant that existed in
#      the source but not in the reused rlib).
#   2. Tests that touch shared global state fail spuriously and pass when
#      re-run (observed: three `trusted_git` tests contending on git's
#      global config, which they do not isolate).
#
# Both are expensive to misdiagnose: they look like real regressions.
# This script makes the whole operation — build AND test — exclusive, so
# concurrent worktrees queue at a boundary you control instead of racing
# somewhere you cannot see.
#
# USAGE
#
#   scripts/cargo-lock.sh test --workspace
#   scripts/cargo-lock.sh test -p vega_ui
#   scripts/cargo-lock.sh build --workspace --all-targets
#   scripts/cargo-lock.sh --status        # who holds the lock right now?
#   scripts/cargo-lock.sh --wait test --workspace   # block until free
#   scripts/cargo-lock.sh --release       # clear a stale lock by hand
#
# Default behaviour when the lock is held is to FAIL FAST with the holder's
# identity, not to block. A silent queue makes an agent look stuck; a clear
# message lets it pick up other work or re-run with --wait deliberately.
#
# The marker lives in the repository's shared git directory
# (`git rev-parse --git-common-dir`), which resolves to the SAME path from
# every worktree, so all of them see one lock.
#
# POSIX-sh compatible.

set -u
(set -o pipefail 2>/dev/null) && set -o pipefail

LOCK_NAME="vega-cargo.lock.d"

die() {
  printf 'cargo-lock: %s\n' "$1" >&2
  exit 1
}

# Resolve the shared git dir. Fails closed: without it we cannot coordinate,
# and silently running unlocked would defeat the point.
common_dir() {
  _cd=$(git rev-parse --git-common-dir 2>/dev/null) || return 1
  [ -n "$_cd" ] || return 1
  # git prints a relative path when run from the main checkout.
  case "$_cd" in
    /*) printf '%s\n' "$_cd" ;;
    *) printf '%s\n' "$(cd "$_cd" 2>/dev/null && pwd)" ;;
  esac
}

# The worktree that owns this shell, for the holder record.
worktree_path() {
  git rev-parse --show-toplevel 2>/dev/null || printf 'unknown'
}

# Read a field from the holder record. Prints nothing when absent.
holder_field() {
  _f=$1
  [ -f "$LOCK_DIR/info" ] || return 0
  sed -n "s/^$_f=//p" "$LOCK_DIR/info" 2>/dev/null | head -1
}

holder_pid() {
  holder_field pid
}

# True when the recorded pid is still alive.
holder_alive() {
  _p=$(holder_pid)
  [ -n "$_p" ] || return 1
  kill -0 "$_p" 2>/dev/null
}

describe_holder() {
  _pid=$(holder_field pid)
  _wt=$(holder_field worktree)
  _cmd=$(holder_field command)
  _started=$(holder_field started)
  printf '  pid:      %s\n' "${_pid:-unknown}"
  printf '  worktree: %s\n' "${_wt:-unknown}"
  printf '  command:  %s\n' "${_cmd:-unknown}"
  printf '  started:  %s\n' "${_started:-unknown}"
}

# Remove the lock, but only if we own it (never steal someone else's).
release_own() {
  [ -d "$LOCK_DIR" ] || return 0
  _owner=$(holder_field pid)
  [ "$_owner" = "$$" ] || return 0
  rm -rf "$LOCK_DIR" 2>/dev/null || true
}

acquire() {
  _attempt=0
  while :; do
    if mkdir "$LOCK_DIR" 2>/dev/null; then
      {
        printf 'pid=%s\n' "$$"
        printf 'worktree=%s\n' "$(worktree_path)"
        printf 'command=%s\n' "$*"
        printf 'started=%s\n' "$(date '+%Y-%m-%d %H:%M:%S')"
      } > "$LOCK_DIR/info"
      # Release on every exit path, including Ctrl-C.
      trap 'release_own' EXIT INT TERM HUP
      return 0
    fi

    # Someone holds it (or a crashed process left it behind).
    if ! holder_alive; then
      if [ "$_attempt" -eq 0 ]; then
        printf 'cargo-lock: clearing stale lock (pid %s is gone)\n' \
          "$(holder_pid)" >&2
        rm -rf "$LOCK_DIR" 2>/dev/null || true
        _attempt=1
        continue
      fi
      die "stale lock at $LOCK_DIR could not be cleared; remove it by hand"
    fi

    if [ "$WAIT_FOR_LOCK" -eq 1 ]; then
      printf 'cargo-lock: waiting for pid %s (%s)\n' \
        "$(holder_pid)" "$(holder_field worktree)" >&2
      sleep 5
      continue
    fi

    printf 'cargo-lock: another worktree is building or testing.\n' >&2
    describe_holder >&2
    printf '\n' >&2
    printf 'Both worktrees share one target/ directory. Running now can reuse\n' >&2
    printf 'the wrong crate artifacts or make shared-state tests fail spuriously.\n' >&2
    printf 'Wait for it to finish, work on something else, or run:\n' >&2
    printf '  %s --wait %s\n' "$0" "$*" >&2
    exit 1
  done
}

show_status() {
  if [ ! -d "$LOCK_DIR" ]; then
    printf 'cargo-lock: free (no build or test running)\n'
    return 0
  fi
  if holder_alive; then
    printf 'cargo-lock: HELD\n'
    describe_holder
    return 0
  fi
  printf 'cargo-lock: stale (pid %s is gone)\n' "$(holder_pid)"
  printf 'Clear it with: %s --release\n' "$0"
}

# ---------------------------------------------------------------- arguments

WAIT_FOR_LOCK=0
MODE=""
# Consume leading options with `shift` so they never reach cargo. A plain
# `for` loop with `break` leaves the already-consumed flags in "$@" and
# passes them through to cargo, which rejects them.
while [ "$#" -gt 0 ]; do
  case "$1" in
    --status)
      MODE=status
      shift
      ;;
    --release)
      MODE=release
      shift
      ;;
    --wait)
      WAIT_FOR_LOCK=1
      shift
      ;;
    -h | --help)
      sed -n '2,45p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      MODE=run
      break
      ;;
  esac
done

COMMON_DIR=$(common_dir) || die "not inside a git repository"
LOCK_DIR="$COMMON_DIR/$LOCK_NAME"

case "$MODE" in
  status)
    show_status
    exit 0
    ;;
  release)
    if [ -d "$LOCK_DIR" ] && holder_alive && [ "$(holder_pid)" != "$$" ]; then
      printf 'cargo-lock: refusing to release a lock held by a live process\n' >&2
      describe_holder >&2
      exit 1
    fi
    rm -rf "$LOCK_DIR"
    printf 'cargo-lock: released\n'
    exit 0
    ;;
  run) ;;
  *)
    die "nothing to do; pass a cargo subcommand, --status, or --release"
    ;;
esac

# ---------------------------------------------------------------- run cargo

[ "$#" -gt 0 ] || die "no cargo arguments given (e.g. 'test --workspace')"

acquire "$@"
printf 'cargo-lock: acquired by pid %s in %s\n' "$$" "$(worktree_path)"

cargo "$@"
_status=$?

# release_own runs again via the EXIT trap; harmless and keeps Ctrl-C paths
# covered even if the trap fired early.
exit "$_status"
