# A7 integrated native acceptance

Status: PARTIAL PASS · 2026-09-17 · Primary owner: Codex

CPA credential, `hy3` text and read-only tool use pass in the installed native
app. `glm-5.3-flash` remains blocked by external CPA quota; the draft/Pricing
repair is covered by mounted production-window E2E but was not repeated
manually on the final installed build.

## Integrated candidate

- Branch: `feat/a7-usable-e2e` at `586d633` before this report update
  (A7-01/A7-02 plus runtime failure, tool-fragment and Pricing-draft fixes).
- `cargo fmt --all -- --check`: PASS.
- `./scripts/cargo-lock.sh test --workspace`: PASS, including doctests;
  existing load-sensitive ignored tests remain ignored.
- `./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`:
  PASS.
- `./scripts/cargo-lock.sh xtask package`: PASS; signed candidate at
  `dist/Vega.app`; `codesign --verify --deep --strict` PASS.
- First candidate executable SHA-256:
  `c0d26c89e4d277d83830b502be07b0a4d4a576bdf6345b59bf2e65db56b8d8f5`.
  The previous installed executable was
  `62928924b8a37cc2ae4e9bece2d8dabd1c660112a2fcffe56ccfe83e3d895112`.
- Final installed executable SHA-256:
  `c3ebf1a2c9d1a85412807c7633aef93a1a4bdb711ab94aaff80505624c9b069a`.
  Its signature and candidate/install hash equality were verified. The
  previous installed A7 bundle is recoverable at
  `/tmp/vega-a7-final.jgZIaq/Vega.app.previous`.
- On the final integration commit, `./scripts/cargo-lock.sh test --workspace`,
  `cargo fmt --all -- --check`, and
  `./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`
  all exited 0. `./scripts/cargo-lock.sh xtask package` exited 0.

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

## Second native pass: working text model and uncovered defects

7. Direct Pi requests with the imported CPA credential returned `PI_OK` for
   `hy3` and `deepseek-v4.1-flash`. Vega's configured/priced
   `deepseek-v4-flash` is a different ID; a direct Pi request for that ID
   returned `unknown provider for model`. Switched Vega's default model to
   `hy3` through Settings → General.
8. Vega's first `hy3` submission was redirected to Settings → Pricing because
   `hy3` was not priced. The route cleared the exact unsent draft and left a
   durable empty `hy3` thread (zero messages). Via Settings → Pricing, added
   `hy3` as a custom model with all four USD-per-million rates at `0` as a
   **temporary local estimate only**: CPA accepted the tested `hy3` request
   with zero account balance, but its long-term tariff is not verified. No
   config file was manually edited.
9. Retyped `Reply with exactly VEGA_E2E_OK.` in the Vega UI and submitted.
   The model replied `VEGA_E2E_OK` in the conversation. This is the first
   complete native text-message success on the installed build.
10. In the same task on the first candidate, requested a read-only inspection
    of the project's `README.md`. Vega rendered nine failed/corrupt tool
    cards and no final answer. The durable tool calls have empty tool names, `{}` inputs and
    `rejected` status (`run_mode` / unavailable tool). Tool-use E2E is not
    passing in that build. No project files were changed by this test.

## Final installed native regression

11. Integrated the fixes, ran the combined workspace tests/strict lint/format
    and packaged a signed app. Quit the first candidate, backed it up, then
    installed the final candidate. The default `hy3`, local CPA credential
    and zero-rate Pricing entry persisted; no key was exposed.
12. From the visible Vega new-task composer, sent a read-only request to
    inspect the selected project's `README.md`. The app rendered the correct
    first heading `R13 Alpha workspace` and a green `read · 已完成` tool card.
    SQLite confirmed one `read` tool call with `success` status and a
    read-only approval source; the project Git worktree remained clean.
13. Reopened the earlier `glm-5.3-flash` failed task. The conversation now
    visibly states that the reply failed and advises checking provider
    status/quota before retry. The specific live quota diagnosis is covered
    by production tests, not claimed from this historical row because the
    original provider body was intentionally not persisted.

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
- Credential import, first-message submission, a real text reply, a real
  read-only tool call and persisted-failure visibility are verified on the
  installed build. `glm-5.3-flash` remains unavailable until its CPA quota
  changes. Pricing repair passed mounted-window E2E (zero row before price
  repair, exact draft retained on return, exactly one row after resubmit),
  but final native manual repetition was not performed.
- The temporary `hy3` zero-rate local Pricing entry should be replaced with
  actual CPA rates if its billing changes; it is not a verified tariff.
  Historical failed/empty test tasks were not deleted. No remote push or
  merge was performed.
