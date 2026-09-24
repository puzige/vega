# Shared Cargo cache for PR Check and master CICD

## User decision

2026-09-24: Keep the PR check as one sequential, unsharded job, while allowing it to restore the Cargo cache populated by the trusted master packaging workflow. Both workflows use the exact same rust-cache `shared-key`, retaining the existing key `vega-master-build` so the cache populated by the latest master run remains reusable. Only master saves; PR runs restore without saving.

Rename the master packaging workflow file from `.github/workflows/master-build.yml` to `.github/workflows/cicd.yml`. Set its GitHub Actions display name to `master`.

## Scope

- Configure `Swatinem/rust-cache@v2` in both `.github/workflows/pr-check.yml` and `.github/workflows/cicd.yml`.
- In both workflows set `shared-key: vega-master-build`.
- Put cache setup after Rust 1.98.0 installation and before Cargo build/lint/test commands.
- In both workflows use `save-if: ${{ github.ref == 'refs/heads/master' }}`. This evaluates true for the master push workflow and false for pull request refs, so PR checks consume but do not write the shared cache.
- Keep the master workflow's `push: branches: [master]` trigger, `cargo xtask package` command, artifact upload, timeout, permissions, and concurrency behavior.
- Keep the PR required check name `check (fmt, clippy, test)`, one `macos-latest` job, sequential fmt → clippy → workspace test, and all test coverage. No sharding, matrices, nextest archives, retries, or extra jobs.
- Update active repo guidance and record that Issue #175's former no-PR-cache decision is superseded by this explicit user decision.

The same `shared-key` is the common cache namespace; rust-cache also derives its effective key from the platform/toolchain and Cargo environment/manifest hashes. An unchanged master cache should match PR checks on the same Rust toolchain and runner platform. When a PR changes Cargo inputs, the action may restore a compatible prefix cache, rebuild affected dependencies, and—because `save-if` is false—not publish a PR cache.

## Acceptance matrix

| ID | Check | Expected result |
|---|---|---|
| C1 | Compare PR and master workflow cache configuration | Both use `Swatinem/rust-cache@v2` and exactly `shared-key: vega-master-build` |
| C2 | Evaluate save condition for `pull_request` ref | False; PR restores but does not save |
| C3 | Evaluate save condition for `refs/heads/master` | True; trusted master packaging run owns cache updates |
| C4 | Inspect renamed workflow | `cicd.yml` exists, top-level `name: master`, push-to-master trigger preserved; old `master-build.yml` is absent |
| C5 | Inspect PR check topology | One required job named `check (fmt, clippy, test)`; sequential fmt, clippy, and workspace tests remain unchanged and unsharded |
| C6 | Cloud PR check | Required check succeeds on the PR |
| C7 | Cloud master package | `cargo xtask package` and artifact upload succeed after merge |

## Implementation plan

1. Add the same cache action and shared key to the PR workflow with the master-only save condition.
2. Rename the master packaging workflow to `cicd.yml`, set its display name to `master`, and retain the existing cache key and master-only save condition.
3. Update `AGENTS.md`, `docs/vega-exec-guide.md`, `.agents/skills/vega-kanban-delivery/SKILL.md`, and the historical #175 spec note.
4. Parse/check both YAML workflows and verify the cache expressions and required PR topology; rely on cloud PR check and post-merge package run for the full verification.

## Non-goals

- Changing the PR check commands, required check context, runner, test concurrency, or test coverage.
- Making PR runs write a cache or changing the existing `vega-master-build` cache namespace.
- Changing release triggers, release workflow, bundle contents, or product code.
- Adding a second workflow/job, sharding, or runtime-specific optimizations.
