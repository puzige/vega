#!/bin/sh
# Share one Cargo target/ directory across every git worktree of a repository.
#
# Why: each worktree normally gets its own target/, so the first build in a
# fresh worktree recompiles the entire dependency graph from scratch. For a
# workspace with ~900 dependencies that is the dominant cost of any task, and
# it is paid again for every new worktree. Pointing every worktree at one
# target/ makes the second and later worktrees incremental instead.
#
# How: the main checkout keeps the real target/ directory and becomes the
# canonical build cache. Every other worktree gets a `target` symlink to it.
# Cargo follows the symlink and writes into the shared directory.
#
# Usage:
#   cargo-share-target.sh [repo-path]        # default: current repo
#   cargo-share-target.sh --status [repo]    # show current wiring
#   cargo-share-target.sh --unshare [repo]   # restore per-worktree target dirs
#
# Idempotent. Re-run after adding a worktree.
#
# Safety notes:
#   - Cargo's exclusive lock covers only the COMPILE phase; it is released
#     before test binaries run. Two worktrees can therefore test at the same
#     time, which produced real failures: a crate artifact from one worktree
#     reused by another whose sources differ (E0599 on a symbol present in
#     source), and shared-global-state tests failing spuriously. Use
#     scripts/cargo-lock.sh to serialize the whole build+test span.
#   - `target` is added to .git/info/exclude (local, uncommitted) so the
#     symlink never shows up in `git status`. The repo's own .gitignore uses
#     `target/`, which matches directories only and would not hide a symlink.
#   - The main checkout must not be mid-build when converting, because its
#     target/ is moved aside into the shared location.
#
# POSIX-sh compatible.

set -u
(set -o pipefail 2>/dev/null) && set -o pipefail

die() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

# Resolve the primary (main) checkout of the repo that contains $1.
primary_worktree() {
  git -C "$1" worktree list --porcelain 2>/dev/null \
    | awk '/^worktree / { print substr($0, 10); exit }'
}

# Print every worktree path, main first.
all_worktrees() {
  git -C "$1" worktree list --porcelain 2>/dev/null \
    | awk '/^worktree / { print substr($0, 10) }'
}

# True when any cargo/rustc process has this path as its working directory.
is_building() {
  _target=$1
  for _pid in $(pgrep -f 'cargo|rustc' 2>/dev/null); do
    _cwd=$(lsof -a -p "$_pid" -d cwd -Fn 2>/dev/null | grep '^n' | sed 's/^n//')
    case "$_cwd" in
      "$_target" | "$_target"/*) return 0 ;;
    esac
  done
  return 1
}

ensure_exclude() {
  _repo=$1
  _exclude="$_repo/.git/info/exclude"
  [ -d "$_repo/.git" ] || return 0
  if [ -f "$_exclude" ] && grep -qx 'target' "$_exclude" 2>/dev/null; then
    return 0
  fi
  {
    printf '\n# Shared build directory: worktrees symlink target -> the main checkout\n'
    printf '# so every worktree reuses one compiled dependency graph instead of\n'
    printf '# paying a full cold rebuild. Local to this clone; not committed.\n'
    printf 'target\n'
  } >> "$_exclude"
  printf '  excluded: %s\n' "$_exclude"
}

show_status() {
  _repo=$1
  _primary=$(primary_worktree "$_repo")
  printf 'repo:    %s\n' "$_repo"
  printf 'primary: %s\n\n' "$_primary"
  printf '%-52s %s\n' "WORKTREE" "target"
  all_worktrees "$_repo" | while read -r _wt; do
    if [ -L "$_wt/target" ]; then
      _dest=$(readlink "$_wt/target")
      printf '%-52s -> %s\n' "$_wt" "$_dest"
    elif [ -d "$_wt/target" ]; then
      _size=$(du -sh "$_wt/target" 2>/dev/null | cut -f1)
      printf '%-52s dir (%s)\n' "$_wt" "${_size:-?}"
    else
      printf '%-52s (none)\n' "$_wt"
    fi
  done
}

share() {
  _repo=$1
  _primary=$(primary_worktree "$_repo")
  [ -n "$_primary" ] || die "not a git repository: $_repo"

  _shared="$_primary/target"

  # The main checkout must own the real directory; it is the build cache.
  if [ -L "$_shared" ]; then
    die "$_shared is already a symlink; run --unshare first"
  fi
  if [ ! -d "$_shared" ]; then
    printf 'note: %s does not exist yet; it will be created by the first build\n' "$_shared"
  fi

  ensure_exclude "$_primary"

  _changed=0
  for _wt in $(all_worktrees "$_repo"); do
    [ "$_wt" = "$_primary" ] && continue

    if [ -L "$_wt/target" ]; then
      _dest=$(readlink "$_wt/target")
      if [ "$_dest" = "$_shared" ]; then
        printf '  ok (already shared): %s\n' "$_wt"
      else
        printf '  relink: %s (%s -> %s)\n' "$_wt" "$_dest" "$_shared"
        rm -f "$_wt/target" && ln -s "$_shared" "$_wt/target"
        _changed=1
      fi
      continue
    fi

    if [ -d "$_wt/target" ]; then
      # An existing per-worktree build dir. Moving it into the shared cache
      # would mix fingerprints from different source trees, so discard it.
      # The shared cache already holds the compiled dependency graph, so the
      # next build here is incremental anyway.
      if is_building "$_wt"; then
        printf '  SKIP (build in progress): %s\n' "$_wt"
        continue
      fi
      _size=$(du -sh "$_wt/target" 2>/dev/null | cut -f1)
      printf '  replace dir (%s) with symlink: %s\n' "${_size:-?}" "$_wt"
      rm -rf "$_wt/target"
    else
      printf '  link: %s\n' "$_wt"
    fi

    ln -s "$_shared" "$_wt/target"
    _changed=1
  done

  printf '\n'
  show_status "$_repo"

  if [ "$_changed" -eq 1 ]; then
    printf '\nNext build in each worktree is incremental against the shared cache.\n'
  else
    printf '\nAlready wired; nothing to do.\n'
  fi
}

unshare() {
  _repo=$1
  _primary=$(primary_worktree "$_repo")
  [ -n "$_primary" ] || die "not a git repository: $_repo"

  for _wt in $(all_worktrees "$_repo"); do
    if [ -L "$_wt/target" ]; then
      printf '  unlink: %s\n' "$_wt"
      rm -f "$_wt/target"
    fi
  done
  printf '\nEach worktree will rebuild its own target/ on the next build.\n'
}

ACTION=share
REPO=""

for _arg in "$@"; do
  case "$_arg" in
    --status) ACTION=status ;;
    --unshare) ACTION=unshare ;;
    -h | --help)
      sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) REPO="$_arg" ;;
  esac
done

if [ -z "$REPO" ]; then
  REPO=$(git rev-parse --show-toplevel 2>/dev/null) \
    || die "not inside a git repository; pass a repo path"
fi

case "$ACTION" in
  status) show_status "$REPO" ;;
  unshare) unshare "$REPO" ;;
  share) share "$REPO" ;;
esac
