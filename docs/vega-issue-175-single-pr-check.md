# Issue #175 — Single unsharded PR check

Status: implementation specification; acceptance is pending the cloud PR check.

## User goal

On 2026-09-24 the user chose maintenance simplicity over shorter CI duration. The current PR gate has parallel quality and archive-build jobs, nextest artifact transport, a shard matrix, and an aggregate check. Replace that topology with one straight-through Cargo job that is easy to inspect and maintain. Do not split tests.

## Scope

Update `.github/workflows/pr-check.yml` so that it:

- Runs only for `pull_request` targeting `master`.
- Has exactly one macOS job whose visible check context is `check (fmt, clippy, test)`; master branch protection requires this exact name.
- Keeps the 60-minute job timeout, `contents: read` permission, concurrency cancellation for superseded runs, and pinned Rust 1.98.0 with `rustfmt` and `clippy` components.
- Runs these commands sequentially in the same job:
  1. `cargo fmt --all -- --check`
  2. `cargo clippy --workspace --all-targets -- -D warnings`
  3. `cargo test --workspace --no-fail-fast -- --test-threads=1`
- Uses Cargo directly for the complete workspace suite, including unit, integration, and doc tests. A command failure fails the job and blocks the required check. Do not retry automatically or suppress failures.
- Has no cache action, `workflow_dispatch`, throughput inputs, nextest installation, archive build/upload/download, matrix/shard configuration, or aggregate job. Longer duration is accepted.

`master-build.yml` continues to package the master build after pushes to `master`; `release.yml` remains tag-driven and unchanged. If the old PR/master shared Cargo cache is removed, the master build cache must remain self-contained and must not alter packaging or release triggers.

`.config/nextest.toml` may remain for developers who use nextest for targeted local runs. The PR workflow no longer installs or invokes nextest and does not read this configuration.

## Non-goals

- Do not modify product source, application tests, release behavior, branch protection, or master-build packaging behavior.
- Do not reduce test coverage, skip or ignore tests, add retry behavior, add a selector, or preserve the old shard topology behind manual inputs.
- Do not run workspace tests or workspace Clippy locally. The cloud PR check is the full-suite verification gate.

## Acceptance criteria

1. Workflow parsing/lint validation succeeds.
2. Static inspection confirms one job with the exact required context, only the PR-to-master trigger, and the three commands in order.
3. Static inspection finds no workflow dispatch, matrix/shard, nextest archive, artifact transport, cache action, or aggregate check in `pr-check.yml`.
4. Active repository guidance describes the single unsharded Cargo job. Historical #123/#140 decisions are marked as superseded for PR-gate topology while their prior measurements and test evidence remain intact.
5. The cloud PR check passes on the final tree. Local workspace-wide tests and Clippy are not part of this task's verification.

## Implementation and verification plan

1. Commit this specification before implementation.
2. Simplify the PR workflow and make any necessary cross-workflow cache comment/key update without touching release triggers or packaging steps.
3. Update the active CI descriptions in `AGENTS.md`, `docs/vega-exec-guide.md` §7, `.agents/skills/vega-kanban-delivery/SKILL.md`, and other directly affected active docs. Add a supersession notice to the #123 and #140 specs; do not rewrite their historical run evidence.
4. Leave `.config/nextest.toml` intact and document its local-only role for this PR-gate topology.
5. Validate workflow syntax with `actionlint` or an available YAML parser and run static topology/order checks. Do not run workspace cargo tests or Clippy locally; rely on the cloud PR gate for those checks.
