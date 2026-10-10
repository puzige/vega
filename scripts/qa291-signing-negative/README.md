# QA291 isolated CI signing rejection

Status: PREPARED, business cases NOT RUN. This is a branch-only verification entry, not a PR/master gate. The main agent reviews and owns commit/push and all remote runs. The production release workflow and signing source are unchanged.

The branch `feat/qa291-signing-negative-ci` has one isolated push workflow at `.github/workflows/qa291-signing-negative.yml`. It uses `contents: read`, checkout `persist-credentials: false`, no secret references, no signing configuration, no Release/tag/version reservation, no publish or artifact upload step, no shared cache writes. Other branches, tags, PRs and master do not trigger it. A first push of the reviewed branch can run this workflow without merging it into master; a new workflow_dispatch entry would require default-branch registration. See [GitHub workflow event documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows).

## Frozen entry and cases

The standalone Rust executable imports the actual `xtask/src/sign_update.rs` using `#[path]`. Its `include_str!` still reads the unchanged repository public key. `workspace_root` is the only crate adapter and points to a fresh owned runner directory. There is no production public API or signing algorithm change. The driver removes the private-key environment variable without reading it before other environment access. It runs one thread, has no asynchronous runtime, reads no key/config file, and accepts no key argument.

| ID | Input | Production assertion | Additional assertions |
|---|---|---|---|
| SIG07-CI-MISSING | Key environment variable absent | Actual process exit 1; exact `update signing key is missing` | workspace_root 0 calls; owned directory empty; no signing output |
| SIG07-CI-WRONG | Public fixed all-zero seed encoded in an Ed25519 PKCS8 v1 fixture | Actual process exit 1; exact `update signing key does not match repository public key` | Original pin preserved; workspace_root 0 calls; owned directory empty; fixture bytes absent from logs |

The zero seed is deliberately public test data and cannot authenticate a production update. It is constructed in memory rather than read from a private-key file. There is no valid-key or successful-signing case. A production refusal returns a real nonzero process status; each workflow case step records failure with `continue-on-error`. The final step requires both failures and verifies their exact captured outcomes and raw stdout/stderr hashes. `continue-on-error` alone is not a pass.

`freeze.json` hashes production signing source, pin, production Cargo.lock, production release script/workflows and executable fixture sources. Before build and each business operation, the runner checks those hashes and checks every dependency's package/version/source/checksum against the production lock. Its separate Cargo.lock contains 40 existing production dependency identities. Dependencies may be fetched from the public registry in the clean GitHub runner; the compile command itself uses `--offline --locked`. Local preparation also skips the fetch. Child environments are explicitly whitelisted; they never inherit credential variables. Signing processes receive only PATH/TMPDIR before the driver sets public fixture data. HOME is never changed.

## Exact execution sequence for the main agent

After reviewing and pushing the test branch, the workflow executes these commands once in a fresh runner-owned evidence directory:

```sh
python3 scripts/qa291-signing-negative/runner.py build --evidence "$RUNNER_TEMP/qa291-signing-negative"
python3 scripts/qa291-signing-negative/runner.py case --case missing --evidence "$RUNNER_TEMP/qa291-signing-negative"
python3 scripts/qa291-signing-negative/runner.py case --case wrong-public --evidence "$RUNNER_TEMP/qa291-signing-negative"
python3 scripts/qa291-signing-negative/runner.py verify --evidence "$RUNNER_TEMP/qa291-signing-negative"
```

Each case command is expected to exit 1. Source/setup/assertion failures are not expected-negative evidence. Build reservation, compile records, per-case reservation through fresh directory creation, original stdout/stderr, outcome metadata and final verification are retained under RUNNER_TEMP. Reusing the same build or case directory is refused; there is no automatic retry. The final CI log prints the exact production error, zero-call/zero-output outcome and raw hashes. Runner files are ephemeral after the job; the main agent must retain the GitHub logs/run metadata and their hashes externally.

## Formal release reruns: reviewed feasibility, not executed here

At preparation time, [master run 37885010453](https://github.com/puzige/vega/actions/runs/37885010453) completed successfully at the fixed commit identified by v0.1.63. Release 407517009 is published, not prerelease, and has the four required uploaded nonempty assets. The fixed current `prepare` reuses that SHA's existing stable tag, returns complete=true for this published release, and performs no POST. All subsequent build/sign/upload/publish steps require complete != true and therefore skip while this state remains unchanged.

[v0.1.3](https://github.com/puzige/vega/releases/tag/v0.1.3) is a published two-asset historical release, ID 396214285. Its tag tree does not contain the public pin. Importantly, the workflow and script at that historical tag were inspected directly: the historical script's ASSETS contains the ZIP and SHA-256 sidecar and already has the published same-SHA early-completion branch. An existing `release.yml` workflow_dispatch at `--ref v0.1.3` checks out that tag and runs this historical script, so current complete metadata produces complete=true and skips downstream steps. This would exercise the historical workflow source, not current production's allow_legacy implementation; the latter remains the independent UNIT evidence.

Main-agent-only remote candidates, after fresh preflight and approval:

```sh
gh run rerun 37885010453 --repo puzige/vega
gh workflow run release.yml --repo puzige/vega --ref v0.1.3
```

Neither command has been executed by this fixture. Both formal workflows retain `contents: write`; they are conditionally read-only, not permission-restricted like the isolated negative workflow. Fresh preflight must establish the single matching published release, tag commit identity, uploaded nonempty assets and original fixed workflow source. Keep before/after tag SHA, Release ID/tag/draft/prerelease/updated_at, asset IDs/names/states/sizes/digests/updated_at, latest identity, attempt/run metadata and complete=true log. Assert every downstream step is skipped. Metadata drift can invalidate the read-only premise; this plan does not claim an atomic read-only guarantee for a production workflow with write permission.

## Preparation records and limits

Fresh owned records are under `/private/tmp/vega-qa291-ci-negative-n45zmmuv`. The earlier `/tmp/vega-backlog-consolidate.OKOf5x` root is missing, so old evidence was not modified, reconstructed or relabeled.

The first offline generate-lockfile operation selected five newer cached transitive versions; the package-identity check rejected them. The first two `--offline --locked` compile attempts exited 101 because a mechanically extracted lock still contained workspace feature-union dependencies; a no-deps metadata operation did not normalize it. Those generated lock files, rejection records, compile stdout/stderr and command/exit/hash records are retained. Full offline Cargo metadata resolved the subset while preserving all production package identities; the subsequent locked offline compile exited 0. Compilation is not a business PASS. Final runner-only changes add binary/hash evidence; final compilation remains preparation only.

SIG07 evidence from this entry proves actual production signing refusal on a GitHub runner and an isolated workflow that has no publish step. It does not inject faults into the live release workflow or prove a production publish job with missing production secrets. SIG08 feasibility here is static plus read-only metadata inspection, not a rerun PASS. Installation, rollback, UI, app/helper execution, production secrets and real publication remain outside this fixture.
