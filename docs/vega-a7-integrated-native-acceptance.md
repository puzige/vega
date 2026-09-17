# A7 integrated native acceptance

Status: IN PROGRESS · 2026-09-17 · Primary owner: Codex

## Integrated candidate

- Branch: `feat/a7-usable-e2e` at `4ecec90` (A7-01 + A7-02).
- `cargo fmt --all -- --check`: PASS.
- `./scripts/cargo-lock.sh test --workspace`: PASS, including doctests;
  existing load-sensitive ignored tests remain ignored.
- `./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`:
  PASS.
- `./scripts/cargo-lock.sh xtask package`: PASS; signed candidate at
  `dist/Vega.app`; `codesign --verify --deep --strict` PASS.
- Candidate and now-installed executable SHA-256:
  `c0d26c89e4d277d83830b502be07b0a4d4a576bdf6345b59bf2e65db56b8d8f5`.
  The previous installed executable was
  `62928924b8a37cc2ae4e9bece2d8dabd1c660112a2fcffe56ccfe83e3d895112`.

## Real native UI, no credential mutation

1. Gracefully quit the previous app and launched the signed candidate by
   its explicit bundle path. Process inspection confirmed the candidate,
   not `/Applications/Vega.app`, was running.
2. Opened a project-bound new task in the real window and entered
   `A7 preflight test: reply PONG.`. Before submission the user's existing
   database contained 67 threads and 18 messages.
3. Submitted twice with `⌘↩` while CPA remained disabled. Both attempts
   retained the editable draft and showed a specific
   `设置 → Providers` repair instruction. The database remained 67 threads
   and 18 messages after each attempt. No new empty task was created.
4. Navigated to Settings → Providers. The explicit
   `从 Pi Agent 导入凭据` action is visibly present. It has **not** been
   clicked: importing creates persistent Vega access to the user's CPA
   service, so action-time confirmation is pending.
5. After candidate checks, quit the candidate and copied the previous
   `/Applications/Vega.app` into the recoverable backup at
   `/tmp/vega-a7-install.fpYtll/Vega.app.previous`; its executable SHA-256
   and code signature matched the original. Replaced only the app bundle
   with the signed candidate, then verified installed signature and SHA-256.
   Launched `/Applications/Vega.app`; process path and visible Pi import
   button confirm the installed copy is running. The database still had
   67 threads and 18 messages after installation.

The earlier historical empty thread from the old build remains untouched;
this task does not perform data cleanup or migration.

## External provider status and remaining acceptance

- Read-only Pi metadata showed current `cpa` matches Vega's selected base
  URL and `glm-5.3-flash` model. A direct Pi request reached CPA but returned
  HTTP 400 `insufficient_user_quota` with balance 0. The separate local Pi
  `opencode-go` credential returned HTTP 401 `CreditsError` with insufficient
  balance. No raw credential was printed or included here.
- Pending explicit user confirmation: click Vega's Pi import action, verify
  success/enablement and that no key appears in UI or normal config.
- Pending external balance or another working provider: real Vega model reply
  and safe tool action. A successful local import by itself is **not** a
  live-provider pass.
- The candidate is installed with a recoverable previous-app backup. No
  remote push or merge was performed. Credential import is still pending
  action-time confirmation; no key has been transferred to Vega.
