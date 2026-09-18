# R71 — Read-only Git must work inside Vega's bash Seatbelt

Status: user-reported E2E defect, 2026-09-18. This is a narrow correction to the S5-T24 workspace-write profile: its default `deny file-write*` unintentionally denies Git's read/write open of `/dev/null`, including for read-only commands. It does **not** authorize Git mutations from bash or relax the project, temporary-root, `.git`, or actual-gitdir fences.

## Evidence and scope

The installed Vega Agent reports `git` failing with `could not open '/dev/null' for reading and writing: Operation not permitted` and resorts to parsing `.git` files. A local reproduction with the same Seatbelt baseline makes `/usr/bin/git ... rev-parse --short HEAD` exit 128 with that exact error; appending `(allow file-write* (literal "/dev/null"))` makes it exit 0. A read-only `git --no-optional-locks ... status --short --branch` also succeeds with that exact exception. This is an OS policy defect, not an Agent Loop, model, or Git repository defect.

## Contract

1. The production `vega_tools::sandbox` profile keeps `(allow default)` plus baseline `(deny file-write*)`; the only new writable path is the **literal** `/dev/null` character device. Do not allow `/dev`, `/private`, `/private/tmp`, symlink targets, arbitrary device paths, or a Git-specific bypass of `sandbox-exec`. The project root and per-call private temp allowances remain scoped as in S5; `.git` entry and resolved gitdir remain explicitly denied after the project allowance.
2. Read-only Git commands run through the real production bash tool in an owned temporary repository. This includes `git rev-parse --short HEAD` and a status form that avoids optional locks. The tool result must be successful, not a shell fallback that manually reads `.git` metadata.
3. Shell redirection to `/dev/null` is permitted. Writes to a sibling path outside the project/private temp and writes to the project `.git` or a worktree's external gitdir still fail in the same real Seatbelt path. A read-only Git fix must not convert mutation commands to success.
4. Retain pre-spawn hardlink scanning, fail-closed sandbox self-test, bounded output, per-call temp lifecycle, permission gate, and execution cancellation. No new dependency or API surface. Test the exact production profile, not a separately copied approximation.

## Acceptance

- E2E-REAL: production bash tool against an owned temp Git repository, with read-only Git command success and the expected commit/branch result. Include a worktree `.git` indirection case if the existing test infrastructure permits it without widening the fence.
- E2E-REAL security negative: the same profile rejects outside-root, `.git`, and actual-gitdir writes while allowing `/dev/null`; verify outside sentinels and Git refs remain unchanged. No user's repository is a mutation target.
- `cargo fmt --all -- --check`, `./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`, `./scripts/cargo-lock.sh test --workspace`, `git diff --check`. Then package, signature check, and a native Vega smoke test. Report a limitation if a screenshot's merge/worktree cleanup request still needs a separately authorized trusted Git workflow; R71 must not make bash mutate `.git`.

## Change record

- 2026-09-18: Initial spec from native screenshot and direct Seatbelt reproduction. Narrow `/dev/null` literal exception added to S5 policy; all other fences retained.
