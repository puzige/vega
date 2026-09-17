# A7 integrated native acceptance

Status: IN PROGRESS · 2026-09-17 · Primary owner: Codex

## Integrated candidate

- Branch: `feat/a7-usable-e2e` at `f9e12ab` before the follow-up
  acceptance update (A7-01 + A7-02).
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

## Real native UI

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
4. After candidate checks, quit the candidate and copied the previous
   `/Applications/Vega.app` into the recoverable backup at
   `/tmp/vega-a7-install.fpYtll/Vega.app.previous`; its executable SHA-256
   and code signature matched the original. Replaced only the app bundle
   with the signed candidate, then verified installed signature and SHA-256.
   Launched `/Applications/Vega.app`; process path and visible Pi import
   button confirm the installed copy is running. The database still had
   67 threads and 18 messages after installation, before credential import.
5. Navigated to Settings → Providers. The explicit
   `从 Pi Agent 导入凭据` action was visibly present. After the user's
   explicit direction to use the local Pi Agent credential, clicked it in
   the installed app. The UI confirmed `已从 Pi Agent 导入凭据；供应商已启用`,
   showed CPA `已启用` and API Key `已存储`, and did not display the key.
   The owner-only Vega credential file has mode `0600`.
6. On the installed app after import, submitted the existing `hi` draft
   with the selected `glm-5.3-flash` model. The preflight no longer blocked
   submission: the UI created a task and displayed the user message. The
   database became 68 threads and 20 messages. The assistant message ended
   with status `failed` and zero content, with no visible error in the
   conversation UI. This is a real provider/runtime failure after submission,
   not a missing-credential preflight failure.

The earlier historical empty thread from the old build remains untouched;
this task does not perform data cleanup or migration.

## External provider status and remaining acceptance

- Read-only Pi metadata showed current `cpa` matches Vega's selected base
  URL and `glm-5.3-flash` model. A fresh direct Pi request using the same
  credential reached CPA but returned HTTP 400 `insufficient_user_quota`:
  balance 0, required 110. The earlier separate local Pi `opencode-go`
  credential returned HTTP 401 `CreditsError` with insufficient balance.
  No raw credential was printed or included here.
- Vega's Providers `glm-5.3-flash` connection test reported a 15-second
  timeout; the direct Pi quota response took about 29 seconds. The test's
  timeout therefore does not distinguish this upstream quota failure.
- Credential import and first-message submission are verified. A real model
  reply and safe tool action still require external CPA balance or another
  working provider. The current silent failed-assistant UI is being handled
  as a separate follow-up defect; it is not a live-provider pass.
- The candidate is installed with a recoverable previous-app backup. No
  remote push or merge was performed.
