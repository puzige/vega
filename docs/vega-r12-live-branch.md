# R12 B — bounded live sidebar branch projection

## Contract (2026-09-06, before implementation)

The sidebar shows a read-only current HEAD projection, never the registration-time `git_default_branch`. The new conversation worker receives only the already-loaded registered project ID/path projection. One worker, one replaceable pending request and one result slot are owned by each ProjectsBlock. Reload/selection invalidates the generation and clears cached labels immediately. Results must match generation and exact ID/path; removed or hidden targets cannot accept late results.

A 100 ms weak UI poll consumes results. Every 2 seconds it requests a render-only visibility probe; only an expanded ProjectsBlock actually rendered since that probe submits the next batch. Rendering records visibility only; it performs no filesystem or Git IO. This covers startup, reload, selection and external/terminal checkouts without window/sidebar orchestration edits. Cadence is a target, not a performance claim.

Filesystem work is read-only, off the UI thread, and limited to 128 projects per batch with round-robin continuation for larger lists, 4 KiB per file, 128 path components, and a cooperative 250 ms batch deadline. There is no Git process, mutation, dependency, runner-timeout change or schema change. Kernel filesystem calls may exceed the cooperative deadline on a stalled filesystem; no replacement workers accumulate. The UI expires an unanswered batch after 1 second, clears suffixes and invalidates its generation even when a kernel call remains blocked. Cancellation and generation are checked between reads and before publication.

Every path component is opened descriptor-relative with O_NOFOLLOW, directory checks, and O_NONBLOCK; HEAD/pointer reads require regular files and reject oversize data. Normal repositories read only `.git/HEAD`. Linked worktrees accept only a `.git/worktrees/<entry>` directory below the fixed owner `.git` directory (the owner need not be separately registered), and require its `gitdir` backlink to the exact requesting root's `.git` before reading HEAD. A symlink, invalid pointer, non-local ref, malformed HEAD, missing path or read error yields Unknown (no stale branch). This fixed topology plus reciprocal backlink authorizes only that worktree metadata relation; it never recursively discovers owners or reads arbitrary pointed-to files. Detached HEAD displays `detached`; a directory with no `.git` has no suffix. Branch labels reject control characters and Git-invalid ref grammar.

Existing registration-time detection and database values remain unchanged. This read-only service is not a substitute for branch-switch permissions or mutation authority.

## Evidence plan

Production worker + file-backed migrated owned DB/repositories and actual Git checkout, normal/worktree/detached/non-Git/missing/error paths. GPUI production ProjectsBlock reload/selection + real service output must drive the exact suffix helper used by render. Narrow safety tests cover rejected external pointers, symlinks/FIFO/oversize inputs, cancellation and superseded result guards. Retain first failure logs and explicit accepted R11 runner residuals; native CUA belongs to integration owner.
