# Authorized fixed-version NSH backend patches

## Current authorized AX replay repair provenance

This section and `nsh-backend-current-manifest.json` supersede earlier current-state counts, hashes and authorization boundaries below. The user explicitly authorized a minimal AX repair inside fixed NSH 0.4.0 while retaining semantics/actions, with real screenshot/operation/recheck before any layout overlay and **no merge or release**. See [AX replay repair and validation](nsh-ax-replay-fix.md) for exact review, regression, dependency distinction and native limitations.

Only `nsh-platform-gpui/src/access.rs` changes production bytes relative to `de594c84ab176a7a7dd28df5b1774280bf620bf5`. Its final SHA-256 is `a86b0b861c75ce9b8e6ae235f2b2e780fb134d4ab17283e5d2c4bb6e63429024`. Original upstream tests remain; new development tests and exclusive wiring were removed only after verified remote archive commit `48ee1bd67b6723f3c60576e2e0a8995e52259657`, tag `evidence/907-ax-development-12-tests`.

The exact full-file inventory verifier was run after cleanup: **231 current files, 20 delta files, zero missing originals or symlinks**. The extra delta compared with the preceding 19-file production inventory is the newly authorized original `src/access.rs`; the six production additions are unchanged. Original cached archive maps, manifest hashes, VCS metadata and all fixed pins were verified. Root Cargo.toml, Cargo.lock and the temporarily adjusted upstream text-test path match the base byte-for-byte.

| Identity | SHA-256 |
| --- | --- |
| Combined backend tree | `16b001fdce721729a75dc71eeace994f1ed9287177be4366aaa266930e961448` |
| Combined backend delta | `67aa0366217fbba724cb87fd526cf9da76a00a3a3653a4fd0e78bb3aba56dfeb` |
| Current JSON manifest | `a08e648e85af9d54684569fba2cb8fceb74058dcd0c64155975dd62b8d853697` |

Historical verifier snippets below retain their snapshot-specific counts; do not run their old hard-coded 19-file assertion against this new authorized 20-file delta. This is a provenance change, not an acceptance relaxation. Development tests executed 12/12 under the separately archived temporary vendor-test graph; final production Clippy/build/fmt and Python checks used the restored production graph. Final workspace Nextest is **0 discovered / 0 executed, no Rust tests**, not behavioral success. Native AX activation after the repair, pixels, OS actions and the requested visual loop remain unverified because desktop preflight denied screen-recording permission. No overlay, merge, release or installation was performed.

## Current development-only native text observation provenance

Observed UTC `2026-11-09T10:57:13.997769+00:00`. **This section and the current JSON manifest supersede all current-state identities/counts and verifier results in the historical ledger below.** Parent-provided dirty HEAD remains `0b5ea426abfd8d4358489e916273d89474fb190f`; no Git command was run. `observed_origin_master` in the manifest is retained as the preceding historical observation, not a fresh #922 ancestry claim. Parent-reported #922 / `2a29adf9` imports and ordinary fixture changes are outside this vendor-only inventory. This lane writes only this document and `nsh-backend-current-manifest.json`; no integrated-validation document, vendor, application, Cargo or lockfile writes.

Compared with previous manifest SHA-256 `5083c6a877934019ad3405d5db560857495e305f43244ea41a6ac280c177b41c`, exactly **three vendor paths** differ: **two existing files changed and one file added**, not three changed existing files plus a fourth new file. Core is unchanged; no other vendor delta, removed path, missing original, symlink or nonregular entry was found.

| Vendor-relative path | Current SHA-256 |
| --- | --- |
| `nsh-platform-gpui/src/app/integration_support.rs` | `ad96f234d393ec4e283bc44bab60335b018033ac1efbf492efb18dd3e54cc4e7` |
| `nsh-platform-gpui/src/app/text_integration_support.rs` | `d9a90d8d5a2a9558113288d37afecb320b9e353560d4bffeecdf5c1688f3ec4c` |
| `nsh-platform-gpui/src/lib.rs` | `a509bd210b55681f08df36848e5f16f2396d9298f46b11571bb778298d0a58fc` |

All three path changes are **development-only**: `lib.rs` gates the support module and export with existing `vega-host-regressions`; `integration_support.rs` wires/re-exports its new sibling under that gate. The new file and existing support file belong to development-only added files; the new export remains a development-owned portion of mixed `lib.rs`. There are now **10 development-only added files**, **6 capability added files**, and the same 14 changed platform originals. No new production capability, feature or dependency is authorized or claimed. Contract/source limitations: [`nsh-current-text-observation.md`](nsh-current-text-observation.md).

Source inspection observes `AppView::integration_text_observation` copying current nonselectable `DrawnText`, actual native `TextLine` records, node id, native parent-relative frame and content bounds; readiness, membership, shared-borrow and layout checks are present. It does not substitute historical events or issue an engine command. This is static observation, **not independent behavior/security review**. Error absence, pending-projection arrival, painted pixels, clipping/occlusion and cross-snapshot identity are not certified. The accessor is a development seam **not yet compiled by this lane**; historical native/compiled passes below predate these bytes and do not validate them.

| Scope | Original | Current | Unchanged originals | Changed originals | Added | Missing |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| nsh-core | 180 | 180 | 179 | 1 | 0 | 0 |
| nsh-platform-gpui | 45 | 61 | 31 | 14 | 16 | 0 |
| Total | 225 | 241 | 210 | 15 | 16 | 0 |

| Current identity | SHA-256 |
| --- | --- |
| Core complete tree | `dd6cc201bddb9508db77d0470c1b1f12dda9e32fa8e6f1de452feb8fd804afa1` |
| Core delta | `93756724e4b15be0527844ddc7af302153ea1ba90c100a614dd45efc9b9fda8d` |
| Platform complete tree | `d2654a80a9cc336403342eeba8098ef79c7b48e1c42769bdfe6bdb2f320104ee` |
| Platform delta | `6fa7b18c134020c0e8c53bfa9ebdd8d7564f122b7ed65c2d3980d02fe6ae2b9b` |
| Combined complete tree (241 entries) | `5b5662c032ef28ea8ef968ffc8958fa788fad95a7639ac37796af11c89dd0f61` |
| Combined delta (31 entries) | `de99c2770034c8aa72a04a03a53cbc835ccf693a2ef158efb9eabd94721263ab` |
| Current manifest at publication | `73318b7f0de0621ea9537f6704fdbec7a89fc75940c5073ed1ed3b9681cc64fc` |

The exact algorithm remains `hashlib.sha256(json.dumps(sorted(entries)).encode()).hexdigest()` over `(relative_POSIX_path_string, SHA256_hex_string)` pairs: lexical string-pair sorting, Python default JSON whitespace/encoding, no compact JSON or pathlib component sorting. Package paths are root-relative; combined paths include `nsh-core/` or `nsh-platform-gpui/`. All recursively enumerated regular files, including hidden/development files, are included.

Both original manifests retain hashes `6e0856cdbc345291557ff5dfe3d3e0d14e78cb29a742b791f067d93e78ca34fd` (core) and `612172f1c99ce52e34ad9fce8ef545a2c1eda190d0544e2b9210b66d6e325037` (platform). Allowed local cache archives retain SHA-256 `171f17b402bedb0cead975d3abcefbf0e7963583fbdbf1dbcb03ef73ba11191d` and `1e3828245c26e734567b3e6058398c0e4d5809063636460f990a0eaf8a0f74c3`; their full 180/45 entry maps exactly match those manifests, with no archive links. Package VCS metadata remains `e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`. NSH `0.4.0`, Kit `0.7.3`, Fast/platform `0.1.9` pins match the current lockfile. No network authenticity lookup or Kit/Fast source audit is claimed.

### Current read-only verifier

Run from the selected worktree with Python **3.15.2**; only the repository and allowed `.cargo/registry/cache/*/nsh-*-0.4.0.crate` archives are read. No Git/Cargo/network/native execution:

```bash
python3 - <<'PY'
import hashlib, json, pathlib, tarfile, tomllib
H = lambda data: hashlib.sha256(data).hexdigest()
A = lambda files: H(json.dumps(sorted(files.items())).encode())
m = json.loads(pathlib.Path('docs/907/nsh-backend-current-manifest.json').read_text())
combined, delta = {}, {}
for crate, record in sorted(m['packages'].items()):
    root = pathlib.Path(record['root'])
    paths = list(root.rglob('*'))
    assert not root.is_symlink() and not any(p.is_symlink() for p in paths)
    assert all(p.is_file() or p.is_dir() for p in paths)
    files = {p.relative_to(root).as_posix(): H(p.read_bytes()) for p in paths if p.is_file()}
    original_path = pathlib.Path(record['original_manifest'])
    original = json.loads(original_path.read_text())
    assert H(original_path.read_bytes()) == record['original_manifest_sha256']
    assert files == record['files']
    changed = {p: h for p, h in files.items() if p in original['files'] and h != original['files'][p]}
    added = {p: h for p, h in files.items() if p not in original['files']}
    missing = sorted(set(original['files']) - files.keys())
    assert sorted(changed) == record['changed_original_files']
    assert sorted(added) == record['added_files']
    assert missing == record['missing_original_files'] == []
    assert changed | added == record['delta_files']
    assert record['counts'] == dict(original=len(original['files']), current=len(files), unchanged_original=len(original['files'])-len(changed)-len(missing), changed_original=len(changed), added=len(added), missing_original=len(missing))
    assert A(files) == record['tree_sha256']
    assert A(changed | added) == record['delta_sha256']
    assert A(original['files']) == record['original_tree_sha256']
    dev = set(record['development_only_added_files'])
    production = set(record['capability_added_files'])
    assert not dev & production and dev | production == set(added)
    assert json.loads((root/'.cargo_vcs_info.json').read_text()) == record['vcs_info']
    assert record['vcs_info']['git']['sha1'] == m['upstream_commit']
    assert tomllib.loads((root/'Cargo.toml').read_text())['package']['version'] == record['version']
    archives = sorted((pathlib.Path.home()/'.cargo/registry/cache').glob(f'*/{crate}-0.4.0.crate'))
    assert archives
    for archive in archives:
        assert H(archive.read_bytes()) == record['archive_sha256'] == original['archive_sha256']
        with tarfile.open(archive) as tar:
            assert not any(x.issym() or x.islnk() for x in tar.getmembers())
            archived = {x.name.split('/', 1)[1]: H(tar.extractfile(x).read()) for x in tar.getmembers() if x.isfile()}
        assert archived == original['files']
    combined.update({f'{crate}/{p}': h for p, h in files.items()})
    delta.update({f'{crate}/{p}': h for p, h in (changed | added).items()})
    print(crate, record['counts'], 'archive-map=exact')
    print('tree', A(files), 'delta', A(changed | added))
assert len(combined) == m['combined']['current_file_count'] == 241
assert len(delta) == m['combined']['delta_file_count'] == 31
assert A(combined) == m['combined']['tree_sha256']
assert A(delta) == m['combined']['delta_sha256']
print('combined-tree', A(combined), 'combined-delta', A(delta))
lock = tomllib.loads(pathlib.Path('Cargo.lock').read_text())
for name, version in m['pinned_packages'].items():
    packages = [p for p in lock['package'] if p['name'] == name]
    assert len(packages) == 1 and packages[0]['version'] == version
print('pins=unchanged; VCS metadata=exact; symlinks/missing originals=0')
print('manifest-sha256', H(pathlib.Path('docs/907/nsh-backend-current-manifest.json').read_bytes()))
print('PASS provenance only; no compilation, behavior/security review or native acceptance')
PY
```

### Current verification results and handoff

Executed the exact current command above through Python `subprocess.run(['bash', '-c', command])`: **exit 0**, `verifier_exit 0`. Actual stdout:

```text
nsh-core {'original': 180, 'current': 180, 'unchanged_original': 179, 'changed_original': 1, 'added': 0, 'missing_original': 0} archive-map=exact
tree dd6cc201bddb9508db77d0470c1b1f12dda9e32fa8e6f1de452feb8fd804afa1 delta 93756724e4b15be0527844ddc7af302153ea1ba90c100a614dd45efc9b9fda8d
nsh-platform-gpui {'original': 45, 'current': 61, 'unchanged_original': 31, 'changed_original': 14, 'added': 16, 'missing_original': 0} archive-map=exact
tree d2654a80a9cc336403342eeba8098ef79c7b48e1c42769bdfe6bdb2f320104ee delta 6fa7b18c134020c0e8c53bfa9ebdd8d7564f122b7ed65c2d3980d02fe6ae2b9b
combined-tree 5b5662c032ef28ea8ef968ffc8958fa788fad95a7639ac37796af11c89dd0f61 combined-delta de99c2770034c8aa72a04a03a53cbc835ccf693a2ef158efb9eabd94721263ab
pins=unchanged; VCS metadata=exact; symlinks/missing originals=0
manifest-sha256 73318b7f0de0621ea9537f6704fdbec7a89fc75940c5073ed1ed3b9681cc64fc
PASS provenance only; no compilation, behavior/security review or native acceptance
```

Additional Python static assertions exited **0**: historical ledger reconstructed byte-for-byte to prior document SHA-256 `31ea12f4054129d211377ab242467508243843fc5f30551959a7d81e3fd99feb`; prior increment retained in manifest history; two changed existing paths/one added path; 10 development-only/6 capability additions; exact existing feature-gate/module/export wiring; final newline, no trailing whitespace or merge markers in the two authorized documents. The final command/readback checks are provenance/static evidence only.

One publication attempt failed at shell parsing (`unmatched` backtick) because the outer heredoc delimiter collided with the embedded verifier delimiter. State readback confirmed both original document/manifest hashes unchanged before the corrected unique-delimiter publication. No source or acceptance change resulted; no failed build/test was retried.

**READY / stopped applies to provenance publication only.** Parent owns compilation/strict linting, existing mounted-fixture coverage, independent review, final-head CI and user/native acceptance; none is claimed by this lane. Preserve all test/support seams now. Only after genuine recoverable original source/patch/full-SHA plus actual validation evidence have been archived, remove all development tests/seams, export/feature and harness wiring as applicable, retain authorized production behavior, then re-inventory and rehash the final source. Cleanup is later, not performed or declared complete here. Any resumed vendor write invalidates this snapshot until reverified.

---

## Historical ledger notice

Everything below is preserved from the previous document, including its then-current headings, 240/30 counts, commands and results. It describes earlier snapshots only; the current verifier above is authoritative for this publication.

## Latest compiled native-composition provenance

Observed UTC `2026-11-09T08:40:58.370934+00:00`. This supersedes current identities in the historical entries below. Compilation exposed missing imports for the existing `VK_SPACE` constant and an unused removed shortcut closure; parent added the two imports and explicitly dropped the removed closure, without changing the routing decisions or regression assertions. Only `src/input.rs` and `src/text_input/host_tests.rs` changed from the preceding three-file snapshot. `src/events.rs` remains `7be330b4044795c3dfdf71b957e5a01420dd7c95f154affd6532b399b6ae6148`.

| Current identity | SHA-256 |
| --- | --- |
| `nsh-platform-gpui/src/input.rs` | `e5de4dba419888effc2bfe7af57401e2892e1ce5c5dd404ac7c089150f3aacef` |
| `nsh-platform-gpui/src/text_input/host_tests.rs` | `98bb7764c80e023cb9a5bfa32a670a7525dd3b4d64f85eb54a67228d5ccacd63` |
| Platform complete tree | `87d7808e29d34d8ecc4127dd94ed60ddc1f27f03fd2ecccd1da3f4d0cf22bb77` |
| Platform delta | `2d6e2dcf1b9f669d03eff5a53bf5af718c90d4589411b23262189201c43d6a39` |
| Combined complete tree | `981fb7e1f80f461007f04cbe5d3d33c42d4cb809267a41e47aa323771676edc4` |
| Combined delta | `07cc9147b814bbb0e815b61782b40a6cf965b930d1fda0d26c914bcf88333cf8` |
| Current manifest at publication | `5083c6a877934019ad3405d5db560857495e305f43244ea41a6ac280c177b41c` |

Counts/classifications remain **240 current files / 30 delta entries**, with unchanged package versions, original manifests and upstream archives. The manifest preserves preceding increments and full per-file maps. The original verifier below remains the provenance acceptance command.

`target/nsh-closure-ui-signal-build.json` / `.stderr` records all eleven UI development targets compiled successfully in **33.06s**, with existing linker `__eh_frame` size warnings retained. Actual once-only test-artifact execution (`target/nsh-closure-mounted-execution.json`) passes native editing **36/36**, including the expanded marked-key case, geometry **24/24**, and real command-palette mounted **3/3**. Separate current UI/time/rename/preview failures are preserved in the integrated validation document; none is represented as an all-green version. No watchdog timeout occurred. These are compiled-snapshot development results, not final-head CI, independent review, OS candidate delivery or native acceptance. All development support remains pending genuine archival/removal.

## Historical native-composition provenance increment

Observed UTC `2026-11-09T07:37:31.598952+00:00`. The current machine-readable manifest supersedes the historical snapshot below. Parent fetched the unchanged task branch and observed mainline `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`; #916/#920 remain dirty-byte integration, not ancestry. HEAD remains `0b5ea426abfd8d4358489e916273d89474fb190f`.

Exactly three vendor files changed from the prior manifest; no path was added/removed and all other vendor bytes, pins and original archive identities remain unchanged:

| File under `nsh-platform-gpui/` | Current SHA-256 |
| --- | --- |
| `src/input.rs` | `b61dd1f487fd5863af811d4bc0130cdf442cb583453ad8c968f7083b92b251a8` |
| `src/events.rs` | `7be330b4044795c3dfdf71b957e5a01420dd7c95f154affd6532b399b6ae6148` |
| `src/text_input/host_tests.rs` | `81a07509bac52becac0f60042afbe14d7c5f92ba985b3af1453e70494efb2d6c` |

The authorized same-version native-editing correction checks actual composition before held/direct/replayed dispatch and shares classification with the GPUI host. Candidate/navigation/editing keys and text bypass application dispatch; their host path stops propagation without preventing OS default delivery. Eligible non-editing primary-modified shortcuts retain existing shortcut/keymap/menu routing; releases clear repeat tracking. The existing native-host case retains its assertions and adds real-keymap/Shift/repeat/unrelated-shortcut checks. Exact scope, key classification and limits are in [the native editing contract](nsh-editing-backend-contract.md#current-superseding-native-composition-routing-contract) and [palette follow-up](nsh-command-palette.md). No OS HTML acquisition, Kit/Fast/Core source change or version upgrade occurs in this increment.

Counts remain core **180 files / 1 changed original / 0 added**, platform **60 files / 14 changed originals / 15 added**, combined **240 files / 30 delta entries**. The existing default-JSON sorted POSIX-path hash convention is unchanged:

| Scope | Delta SHA-256 | Complete-tree SHA-256 |
| --- | --- | --- |
| core | `93756724e4b15be0527844ddc7af302153ea1ba90c100a614dd45efc9b9fda8d` | `dd6cc201bddb9508db77d0470c1b1f12dda9e32fa8e6f1de452feb8fd804afa1` |
| platform | `ac8924195c84d5cabf26822bb00910da1c4a376c90d0e33600dc584df6e95510` | `c275480f10d58d7093008d5904905e534b158fce33022b3db0534d40a424f0ec` |
| combined, vendor-relative | `de86afa010749cb997625c8f37d147784e0413421169ff78e68e9c094bc4ca86` | `000e759fb21b9bc66d708b6fe8727158af73ae333fe6436a5e1c6752efe78078` |

Python re-enumerated every file, rejected symlinks/missing paths/unexpected changes, preserved development/production classifications, re-read the written manifest and confirmed original cached archive file maps exactly **180/180** and **45/45**. Manifest SHA-256 at publication: `cdbc61af66f9a151e0aed996f58c70c4268923d5cbaeb010a96dbbd41613c34e`. The verifier below reads the current manifest and remains applicable; its historically printed hashes belong to the older snapshot.

This is provenance validation only. **The three-file IME increment is not yet compiled or executed**; the earlier 36/36 result predates it. Actual palette-controller mounting, GPUI propagation/default behavior, OS candidate delivery, consolidated independent review and final-version gates remain separate. All development support is retained; no final archive/removal is claimed. Historical `runtime pending` text below refers to its own mount-presence snapshot, which later passed geometry/source regressions recorded in the integrated validation document; those results do not certify this new IME delta.

## Historical provenance implementation snapshot — fourth support lane

This section and [`nsh-backend-current-manifest.json`](nsh-backend-current-manifest.json) supersede **current-state** descriptions and hashes in the historical entries below. This is implementation-support documentation, **not independent review**, behavior verification, security clearance, final cleanup, or release acceptance. Historical test/review results below remain tied to their own snapshots and are not transferred to this one.

### Source identity, authority and observation boundary

- Observed branch `feat/907-nsh-full`, dirty worktree `vega-907-nsh-full`, HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`. HEAD alone does not identify these uncommitted vendor bytes; use the content hashes below.
- Parent reports the remote task branch was fetched and already up to date. Read-only `git rev-parse origin/master` returns `f3688f5ae0b7becf1f46434c7b7d732e3a3c759a` (#916). Parent now reports recovery UI/service changes applied as dirty working-tree bytes, not Git ancestry. This lane did not fetch, rebase, merge, or mutate Git, and does not assert complete master integration.
- Original upstream NSH commit remains `e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`, package versions `nsh-core` / `nsh-platform-gpui` **0.4.0**. Their original `.cargo_vcs_info.json` records `crates/nsh-core` / `crates/gpui/nsh-platform-gpui` respectively. Workspace `[patch.crates-io]` selects `vendor/nsh-core` and `vendor/nsh-platform-gpui`; neither is a new upstream release.
- Authorization covers minimal reading selection/native editing patches, plus the expressly approved **same-version synchronous source-caret actuator conditional on the current geometry lease** and **complete nested-wheel consumed/remainder/gesture-owner protocol**. It excludes OS HTML clipboard acquisition, Kit/Fast source changes/upgrades and unrelated backend expansion. Existing native/synthetic clipboard payload representation is not authorization for a new OS HTML acquisition path: `InputPaste::html()` returns an empty string for `Native`, and the existing synthetic payload field for `Synthetic`.
- The parent resumed ownership of backend `src/render.rs` for the mount-presence geometry ABA increment below and owns formatting/compilation/regressions. This fourth lane writes only this ledger and its existing manifest; source, tests, Cargo files, other docs, compilation and Git mutations remain outside its scope. No further workspace source writes are part of this followup. Consolidated independent review waits until the remaining implementation lanes finish.
- The prior provenance snapshot’s read-only TOML inspection confirmed Kit **0.7.3**, Fast / Fast platform **0.1.9** and the two local NSH packages **0.4.0**. That snapshot’s comparison with `HEAD:Cargo.lock` found **0 removed package identities**, the same **14 added development-support package identities** listed historically below, and only the two intended local NSH replacements lose an existing source/checksum. This is a snapshot observation, not a dependency/security review; other writers own Cargo and it must be rechecked after final cleanup/integration. No Fast/Kit source-tree integrity check was repeated in this lane.

### Mount-presence geometry ABA increment — parent implementation, provenance only

Observed current snapshot UTC: `2026-11-09T04:39:46.565817+00:00`. Against the preceding manifest (`2026-11-09T03:12:07.060802+00:00`), exactly **one vendor file changed**: `nsh-platform-gpui/src/render.rs`; no inventory path was added or removed. Previous file SHA-256: `fd837ce64f1b094ec46691dfba58fa4649488155157d2a66dcdb6681793e40c9`. Current file SHA-256: `b602deb11d26198704390ac10eb5969c66046ded6676c8e570a04d362c9e003d`. Core bytes, the other 59 platform files, original manifest hashes and package VCS records match the preceding snapshot. Original archive evidence below is retained for that matching identity; the verifier also rechecks local archive equality without network access.

Actual `render.rs` source at lines 706–714 adds `SurfacePresence(Weak<Surface>)`; its `Drop` upgrades the weak reference and synchronously calls `reading::geometry_invalidated`. At lines 751–775, `SurfaceElement::prepaint` uses its existing `GlobalElementId` in `window.with_element_state::<Rc<SurfacePresence>, _>`: matching `Surface` identity (`Rc::ptr_eq`) reuses the retained presence; a fresh presence invalidates geometry and stores a downgraded Surface reference. This records the parent’s mount-presence capability within the approved same-version conditional source-caret geometry ABA fence, not a new backend/API authorization. It adds no files and changes no Cargo/dependency bytes relative to the preceding vendor snapshot. The prior geometry/scroll integration support and feature-gated runtime/surface/geometry/opacity diagnostics remain inventoried below; this increment does not clean or archive them.

Parent reports a fifth native-geometry source case failed after four passed, motivating this implementation. **Runtime behavior of the new bytes remains unverified here, pending parent compile/regression results.** Earlier passes do not validate the new geometry lifetime fence. The parent-reported workspace six-case GREEN is a separate workspace result, not backend validation. No independent behavior/security review, safety clearance, native acceptance, or assertion that the ABA failure is resolved is made by this provenance lane.

### Exact inventory and hashes

All regular files recursively under each vendor package root are included, not only Rust/tracked/production files. Paths are POSIX strings relative to the package root. No original file is missing, no symlink is present, and no file is excluded. New development support remains included in the identities.

**Exact existing convention:** `hashlib.sha256(json.dumps(sorted(entries)).encode()).hexdigest()` where each entry is `(relative_POSIX_path_string, file_SHA256_hex_string)`. Use Python's default `json.dumps` spacing/encoding and lexicographic **string-pair** sorting, not pathlib component sorting or compact JSON. The JSON manifest's own pretty formatting is not the aggregate input. Full-tree entries include every current file; delta entries include changed originals and added files. Missing originals are recorded separately (zero here), not silently encoded as empty hashes. Each file's full SHA-256, every changed/new path and the original-manifest file SHA are retained in the machine-readable manifest.

| Package | Original files | Current files | Unchanged originals | Changed originals | Added | Missing |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| nsh-core | 180 | 180 | 179 | 1 | 0 | 0 |
| nsh-platform-gpui | 45 | 60 | 31 | 14 | 15 | 0 |
| Total | 225 | 240 | 210 | 15 | 15 | 0 |

| Scope | SHA-256 of original entries | SHA-256 of current delta entries | SHA-256 of complete current entries |
| --- | --- | --- | --- |
| core | `44bfd6ef774edc79f9f2c41132c7fd796fb29e3042ba543b1b2d27e409d27f37` | `93756724e4b15be0527844ddc7af302153ea1ba90c100a614dd45efc9b9fda8d` | `dd6cc201bddb9508db77d0470c1b1f12dda9e32fa8e6f1de452feb8fd804afa1` |
| platform | `74b07bb3fdc6bf7f352620062330c2635758df98feffffd304b494830afaccd7` | `7d2aa298815379997fd995022c82da53332c2dd2df3a8ced34b6862942043d59` | `b75c595111f85bf3ca54c1ee9f2127d8e1835b8bf082dd38642533f7a1d1d022` |

For the additional **combined** identity only, paths are relative to `vendor/`, prefixed `nsh-core/` or `nsh-platform-gpui/` to avoid collisions; the same exact hash algorithm applies. Complete tree: **240 entries**, `26bc85dccf7707e3115ca058b1c4d97227e8bc3ae9bfd20c7b50b49656c65da5`. Delta: **30 entries**, `abb60aadf4f2030c5d23dd184afb6caa5be6db8efe2fc3ec20e80409f4e4f3d3`. These combined values are explicitly a different path scope from the per-package historical convention, not replacements for its values.

| Original input | SHA-256 |
| --- | --- |
| Cached `nsh-core-0.4.0.crate` | `171f17b402bedb0cead975d3abcefbf0e7963583fbdbf1dbcb03ef73ba11191d` |
| `nsh-core-original-files.json` bytes | `6e0856cdbc345291557ff5dfe3d3e0d14e78cb29a742b791f067d93e78ca34fd` |
| Cached `nsh-platform-gpui-0.4.0.crate` | `1e3828245c26e734567b3e6058398c0e4d5809063636460f990a0eaf8a0f74c3` |
| `nsh-platform-original-files.json` bytes | `612172f1c99ce52e34ad9fce8ef545a2c1eda190d0544e2b9210b66d6e325037` |

Both local cached archives were hashed and read without extraction or mutation. Archive regular-file maps match the original manifests exactly: **180/180** and **45/45**, with no missing or extra manifest entry. This establishes local provenance consistency; no network/upstream authenticity lookup was performed.

### Current patch inventory (source inspection, not verified behavior)

| Area | Actual files and source surface |
| --- | --- |
| Selection core | Changed core `src/selection.rs`: exact endpoint read/install seam. All other 179 core files match original hashes. |
| Reading / rendering | Changed platform `src/app.rs`, `src/selectable_view.rs`, `src/text.rs`, `src/render.rs`, `src/system.rs`, `src/surface.rs`; new `src/reading.rs`, `src/reading/geometry.rs`, `src/reading/opacity.rs`. Source contains selection snapshot/listener/install, native geometry/currentness, copy interception and fenced opacity publication. Rendering/geometry code is part of the auditable delta, not asserted unchanged. |
| Native editing | Changed `src/text_input.rs`; new `src/editing.rs`; shared AppView/Surface/system wiring above. Source contains field identities/hooks, composition/key policy, native edit/history/paste transaction and original clipboard payload seams. Historical omissions/limitations below are not treated as a current capability verdict. |
| Conditional source-caret actuator | `AppView::scroll_reading_anchor`, `ReadingGeometry::scroll_anchor`, and changed `src/scroll.rs` contain the synchronous geometry-conditioned scroll application path. Current source includes busy/currentness/geometry checks and native application reporting. Application source/owner/follow/intent revalidation remains a caller contract, not proven by documenting this API. |
| Nested wheel protocol | Changed `src/input.rs`, `src/events.rs`, `src/scroll.rs`, `src/native.rs`, `src/surface.rs`, shared AppView/render/lib wiring; new `src/events/scroll_bridge.rs`, `src/scroll_scope.rs`. Public source includes `ScrollOutcome`, `ScrollRemainder`, `ScrollRoute`, `ScrollScope`, route registration, remainder consumption and gesture-owner query. Native component scope/prepaint integration is explicitly included; the older “no wheel patch” statements below are historical, not current. This ledger does not certify protocol completeness in execution. |
| Shared exports / development feature | Changed `src/lib.rs` and platform `Cargo.toml` connect production APIs plus gated host exports and `vega-host-regressions = []`. The platform Cargo delta against its archive is this feature declaration; current workspace dependency wiring is parent-owned. |
| Development integration / diagnostics | Changed `src/rt.rs`, gated sections in Surface/geometry/opacity/lib, and the new support/host files listed below. These bytes are included in current hashes, not omitted as temporary work. |

The **14 changed platform originals** are exactly: `Cargo.toml`, `src/app.rs`, `src/events.rs`, `src/input.rs`, `src/lib.rs`, `src/native.rs`, `src/render.rs`, `src/rt.rs`, `src/scroll.rs`, `src/selectable_view.rs`, `src/surface.rs`, `src/system.rs`, `src/text.rs`, `src/text_input.rs`. The 15 added files comprise the six capability files named below and the nine development-only files below.

### Development-only inventory — preserve now, archive/clean before final delivery

The following **nine added files** are development-only under `vega-host-regressions`, not production capability expansion:

- `src/app/integration_support.rs`: runtime batch/draw/repaint counters and diagnostics, existing `integration_scroll_alpha`, and gated scroll support module wiring.
- `src/app/scroll_integration_support.rs`: `IntegrationScrollObservation`, `integration_reading_scroll`, `integration_position_reading_scroll`. These observe actual mounted offsets/inset-inclusive maxima/clipped viewport using current geometry/snapshot, and set up actual native scroll via the existing command path; they are development integration support, not the production source-caret actuator and not replacement wheel-consumption logic. See [`nsh-scroll-new-capabilities-backend-observation.md`](nsh-scroll-new-capabilities-backend-observation.md).
- `src/input/scroll_host_tests.rs`.
- `src/reading/anchor_host_tests.rs`.
- `src/reading/geometry_host_tests.rs`.
- `src/reading/host_tests.rs`.
- `src/reading/opacity_host_tests.rs`.
- `src/selectable_view/host_tests.rs`.
- `src/text_input/host_tests.rs`.

These are two support modules plus seven host modules. The remaining **six** added files are production capability modules: `src/editing.rs`, `src/events/scroll_bridge.rs`, `src/reading.rs`, `src/reading/geometry.rs`, `src/reading/opacity.rs`, `src/scroll_scope.rs`.

Mixed production files also carry cleanup obligations: platform Cargo feature; lib module/re-exports/host exports; host module declarations in input/reading/selectable_view/text_input; Surface event observer and draw/repaint/prepaint/paint/flush spans; `rt.rs` batch/job/flush instrumentation and aggregate diagnostic stages; geometry capture/currentness spans plus gated `matches_snapshot`; opacity-currentness span and retained-publication accessor. These are source-observed gated diagnostics, not a claim about runtime cost or safety. Remove only the development-owned pieces after recoverable source/patch/full-SHA and actual validation evidence are archived; retain the authorized production geometry/opacity/scroll behavior. This lane does **not** remove, disable, rename, or hide any test/support byte. Parent must re-inventory Cargo/lockfile/application harness wiring and recompute final identities after cleanup.

### Commands, results and limitations

For this increment, `shasum -a 256 vendor/nsh-platform-gpui/src/render.rs`, Python `hashlib.sha256(path.read_bytes())`, and the final verifier observed the same current file SHA above across successive reads; `stat -f '%m %Sm %z %N' vendor/nsh-platform-gpui/src/render.rs` reported `1794199087 Nov  9 12:38:07 2026 34791 vendor/nsh-platform-gpui/src/render.rs`. This is observed byte stability, not proof that a parent formatting command ran or that later writes cannot occur. Any later source change requires rereading and rehashing.

A preliminary read-only inventory diagnostic mistakenly compared file paths against the original JSON container rather than its `files` map; its delta/count output was discarded. Inspection showed container keys `archive_sha256`, `crate`, `files`, `version` (plus platform `vcs_commit`), and the corrected `original['files']` comparison below retains the exact original convention. An ambiguous multi-match manifest edit was rejected atomically; inspection confirmed no changes before the targeted JSON update. Neither observation fault changed source bytes or acceptance.

Commands run from the selected worktree. Read-only source location used `find vendor -maxdepth 4 -name Cargo.toml -print`, focused `rg -n`, `sed -n`, and Python `tarfile`/`difflib` against local archives. Initial broad output previews were narrowed before relying on details. A wildcard dependency-location command was rejected by zsh (`no matches found: vendor/*/*/Cargo.toml`); explicit paths and TOML parsing replaced that observation attempt. No source/build operation was retried.

- `git rev-parse HEAD`, `git branch --show-current`, `git rev-parse origin/master`: exit 0; exact values above.
- `git show HEAD:Cargo.lock` plus `tomllib` name/version/source/checksum comparison: exit 0; results above. This compares package identities and source/checksums, not all dependency edges or feature resolution.
- `python3 --version`: exit 0, **Python 3.15.2**. No Cargo/rustc command was executed; historical toolchain values below were not re-measured.
- The following exact read-only final verifier checks manifest/source equality, original archive equality, symlink absence, classifications/counts, and all aggregate hashes. Its stdout/result is recorded after the command. This is provenance validation, not a product test.

```bash
python3 - <<'PY'
import hashlib, json, pathlib, tarfile
H = lambda data: hashlib.sha256(data).hexdigest()
A = lambda files: H(json.dumps(sorted(files.items())).encode())
m = json.loads(pathlib.Path('docs/907/nsh-backend-current-manifest.json').read_text())
combined, delta = {}, {}
for crate, record in sorted(m['packages'].items()):
    root = pathlib.Path(record['root'])
    paths = list(root.rglob('*'))
    assert not any(p.is_symlink() for p in paths)
    files = {p.relative_to(root).as_posix(): H(p.read_bytes()) for p in paths if p.is_file()}
    original_path = pathlib.Path(record['original_manifest'])
    original = json.loads(original_path.read_text())
    assert H(original_path.read_bytes()) == record['original_manifest_sha256']
    assert files == record['files']
    changed = {p: h for p, h in files.items() if p in original['files'] and h != original['files'][p]}
    added = {p: h for p, h in files.items() if p not in original['files']}
    missing = sorted(set(original['files']) - files.keys())
    assert sorted(changed) == record['changed_original_files']
    assert sorted(added) == record['added_files']
    assert missing == record['missing_original_files'] == []
    assert changed | added == record['delta_files']
    assert record['counts'] == dict(original=len(original['files']), current=len(files), unchanged_original=len(original['files'])-len(changed)-len(missing), changed_original=len(changed), added=len(added), missing_original=len(missing))
    assert A(files) == record['tree_sha256']
    assert A(changed | added) == record['delta_sha256']
    assert A(original['files']) == record['original_tree_sha256']
    dev = set(record['development_only_added_files'])
    production = set(record['capability_added_files'])
    assert not dev & production and dev | production == set(added)
    archives = sorted((pathlib.Path.home()/'.cargo/registry/cache').glob(f'*/{crate}-0.4.0.crate'))
    assert archives
    for archive in archives:
        assert H(archive.read_bytes()) == record['archive_sha256'] == original['archive_sha256']
        with tarfile.open(archive) as tar:
            archived = {x.name.split('/', 1)[1]: H(tar.extractfile(x).read()) for x in tar.getmembers() if x.isfile()}
        assert archived == original['files']
    combined.update({f'{crate}/{p}': h for p, h in files.items()})
    delta.update({f'{crate}/{p}': h for p, h in (changed | added).items()})
    print(crate, record['counts'], 'archive-map=exact')
    print('tree', A(files), 'delta', A(changed | added))
assert len(combined) == m['combined']['current_file_count'] == 240
assert len(delta) == m['combined']['delta_file_count'] == 30
assert A(combined) == m['combined']['tree_sha256']
assert A(delta) == m['combined']['delta_sha256']
print('combined-tree', A(combined), 'combined-delta', A(delta))
print('PASS provenance only; no behavior/security review')
PY
```

The exact verifier above was rerun for this increment via the read-only wrapper below: **exit 0**, `verifier_exit 0`. The scoped whitespace check also exited **0**, with no output. No Cargo/rustc, build/test, network or Git mutation command was executed.

```sh
python3 - <<'PY'
import pathlib, subprocess
s = pathlib.Path('docs/907/nsh-authorized-backend-patches.md').read_text()
command = s.split('```bash\n', 1)[1].split('\n```', 1)[0]
r = subprocess.run(['bash', '-c', command])
print('verifier_exit', r.returncode)
raise SystemExit(r.returncode)
PY
git diff --check -- docs/907/nsh-authorized-backend-patches.md docs/907/nsh-backend-current-manifest.json
```

Actual verifier stdout:

```text
nsh-core {'original': 180, 'current': 180, 'unchanged_original': 179, 'changed_original': 1, 'added': 0, 'missing_original': 0} archive-map=exact
tree dd6cc201bddb9508db77d0470c1b1f12dda9e32fa8e6f1de452feb8fd804afa1 delta 93756724e4b15be0527844ddc7af302153ea1ba90c100a614dd45efc9b9fda8d
nsh-platform-gpui {'original': 45, 'current': 60, 'unchanged_original': 31, 'changed_original': 14, 'added': 15, 'missing_original': 0} archive-map=exact
tree b75c595111f85bf3ca54c1ee9f2127d8e1835b8bf082dd38642533f7a1d1d022 delta 7d2aa298815379997fd995022c82da53332c2dd2df3a8ced34b6862942043d59
combined-tree 26bc85dccf7707e3115ca058b1c4d97227e8bc3ae9bfd20c7b50b49656c65da5 combined-delta abb60aadf4f2030c5d23dd184afb6caa5be6db8efe2fc3ec20e80409f4e4f3d3
PASS provenance only; no behavior/security review
```

Source stability must be rechecked if a backend writer resumes. Compilation, Clippy, behavioral regressions (including eleven production-body wheel cases), consolidated independent review, final archive/cleanup, final-head required CI and OS/user acceptance remain **unrun/unestablished in this lane**, owned by the parent. The dirty vendor snapshot is not yet a durable committed archive merely because hashes are recorded.

---

## Historical ledger — earlier snapshots, not current-state claims

### Authorization and ownership

The user approved minimal, auditable source patches on the currently fixed NSH version for core reading selection and native editing input, integrated from a project-fixed source. Kit and Fast must not be upgraded, and existing functionality and safety acceptance must not be weakened. Dependency changes beyond these two capability packages require separate approval. The user has now separately selected **“批准两项（推荐）”** for (1) exact nested-wheel consumption/remainder and gesture ownership and (2) synchronous source-caret anchoring conditioned on the current geometry lease. This narrow extension retains the same fixed NSH version, forbids Kit/Fast upgrades or source modifications, and does not reduce functionality/security acceptance. It does not authorize OS HTML clipboard acquisition or unrelated dependencies.

Worktree: `vega-907-nsh-full`; development base `0b5ea426abfd8d4358489e916273d89474fb190f`. The parent fetched origin and confirmed master `34555cec9c0c4c4c1f7a528d739752f414e53edf` remains an ancestor before changes. Existing uncommitted reading-status/timeline/navigation/recovery changes are retained, not attributed to this patch.

Four application writers have disjoint scopes: reading/menu/source mapping; text-input/composer adaptation; sidebar non-input presentation; tool/timeline scroll composition. The parent centrally coordinates dependency sources, manifests, lockfile, shared Surface and shared parent wiring. For the newly approved scroll work only, disjoint backend author scopes are delegated: wheel input/events/Surface versus AppView/reading geometry/scroll actuator; a third lane owns body source-anchor production wiring and a fourth owns real-host nested-scroll regression fixtures. Shared exports/render/manifests remain parent-owned. Writers supply exact backend contracts in their scoped documents. Cargo builds are coordinated by the parent.

## Current continuation provenance check

The parent fetched origin again before this continuation. `origin/master` and `merge-base(HEAD, origin/master)` both remained `34555cec9c0c4c4c1f7a528d739752f414e53edf`; the dirty task tree was preserved without reset/rebase. HEAD remains `0b5ea426abfd8d4358489e916273d89474fb190f`.

A read-only TOML comparison against that HEAD found zero removed lockfile packages and no changed existing package version/checksum except the intentional removal of registry source/checksum for the two local NSH 0.4.0 packages. Kit is 0.7.3; Fast and Fast platform are 0.1.9. Fourteen additions belong to enabled development test-support: atomic 0.5.3, convert_case 0.11.0, crossbeam-queue 0.3.13, fixedbitset 0.5.7, futures-concurrency 7.7.1, pollster 0.2.5, postage 0.5.0, proptest 1.11.0, proptest-macro 0.5.0, quick-error 1.2.3, rand_xorshift 0.4.0, rusty-fork 0.3.1, unarray 0.1.4 and wait-timeout 0.2.1. These are not production dependency expansion; final test cleanup must remove exclusive support and audit the resulting lockfile again.

The cached core archive SHA-256 is `171f17b402bedb0cead975d3abcefbf0e7963583fbdbf1dbcb03ef73ba11191d`; all 180 archive-file hashes exactly match `nsh-core-original-files.json`. The platform archive below and all 45 file hashes exactly match `nsh-platform-original-files.json`. Neither imported tree is missing an original file. Core currently modifies only original `src/selection.rs`. Platform currently modifies original Cargo.toml and app/events/input/lib/render/rt/scroll/selectable_view/surface/system/text/text_input Rust files, plus separately auditable new files. The previously omitted `src/rt.rs` delta only brackets existing batch execution with diagnostics under the development-only `vega-host-regressions` feature; it does not change job ordering, queue policy or flushing. Its helper `src/app/integration_support.rs` also contains the integration scrollbar-alpha query. These diagnostics remain development support to archive and remove during genuine final test cleanup, not a production runtime capability expansion.

The fixed Fast 0.1.9 `TestAppContext::build_with_text_system` interface used in integration-test planning is present in the original cached archive, not a local registry edit: `src/app/test_context.rs` archive and local hashes both `73b6a83396db51e73cc7e0dcd9ecfe60b0a79940bb49084ba54d5c1e71d2e476`; `src/platform/test/platform.rs` both `4692cb57df966235f6ba8d702c9dba044bff6a4dce6ea7bad61de0c212729011`. No Fast source or feature was changed by this check.

### Latest backend content identity and native-focus increment

A fresh read-only archive/manifest comparison again matched all original 180 core files and 45 platform files, with zero originals missing. Core currently has 180 files and platform 55. SHA-256 identities below use `json.dumps(sorted(entries)).encode()` with `(relative POSIX path, current file SHA-256)` pairs; delta entries include changed originals and new files. These are development snapshot identities, not final reviewed or cleaned release identities:

| Scope | Current delta SHA-256 | Complete tree SHA-256 |
| --- | --- | --- |
| core | `93756724e4b15be0527844ddc7af302153ea1ba90c100a614dd45efc9b9fda8d` | `dd6cc201bddb9508db77d0470c1b1f12dda9e32fa8e6f1de452feb8fd804afa1` |
| platform | `536a88f57ea286fb7e4ad5485dd908d3b7f092bfcad20a785bb8624fecc98303` | `d14125c603bc34a97fac1ea4fa18987f59bb205c42293065facccf350952ade3` |

The latest authorized native-editing correction is in `text_input::window_focus`: after a real inactive-to-active transition updates the input's active state, it clones the current hook outside the RefCell borrow and reports focus gain with the current native identity. Repeated unchanged focus does not notify; loss still retires hooks. The added host regression `host_focus_gain_notifies_current_registration_once_and_loss_retires_it` checks one gain, no duplicate, blur retirement and stale-focus rejection. Explicit exports and the application host runner execute it. The latest integrated run executed 36 host regressions and all 5 main-thread reading/editing cases successfully, including clipboard, recovery, history, AX and submit. This is process-local development evidence, not native OS acceptance. Source/output context is in `nsh-reading-editing-integrated-validation.md`; original failed diagnosis logs remain retained locally.

The repeated lockfile comparison still finds zero removed packages, only the same 14 development-support additions and the two intentional local-source replacements. Kit 0.7.3 and Fast 0.1.9 remain unchanged. No wheel consumed/remainder protocol, source-anchor scrolling seam or OS HTML acquisition was added.

## Fixed source and audit route

Prefer the smallest necessary crate set. Begin with `nsh-platform-gpui` 0.4.0 only, extracted from the cached original crates.io archive into `vendor/nsh-platform-gpui`. Preserve its license and original source. Do not modify Cargo's registry/cache or replace the text renderer. Introduce another NSH crate only when a concrete reading/editing contract requires it; record its exact original archive identity first.

Original platform archive SHA-256: `1e3828245c26e734567b3e6058398c0e4d5809063636460f990a0eaf8a0f74c3`, matching Cargo.lock. Original VCS commit: `e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`; VCS path `crates/gpui/nsh-platform-gpui`. Unmodified core/base/engine archives also match their existing lockfile checksums. Path overrides must retain NSH 0.4.0 and all preexisting package versions; Kit remains 0.7.3 and Fast/platform remain 0.1.9. No new transitive package or toolchain change is authorized by this document.

## Acceptance before implementation

### Reading

- Preserve the actual platform text shaping, geometry, built-in cross-text drag selection and highlighting. Never estimate character positions or paint former prepared GPUI runs as an NSH body.
- Expose a native snapshot tied to the current surface generation, selectable area, text-node identities, exact attributed content and selected UTF-16 ranges. Detached, replaced or changed content invalidates a prior snapshot; callers still map ranges through their retained source and enforce reading/copy authority.
- Host-controlled copy must intercept every relevant built-in copy path before any clipboard write, with fail-closed consumption when no current trusted source mapping exists. Context-menu takeover must not leave a competing visible native menu.
- Preserve read-only preview copy/selection/link behavior separately from mutation permission. A selected display string alone never grants clipboard authority.

### Editing

- Native composition wins synchronously before key holding, keymaps, direct dispatch or replay. Candidate-navigation/commit/cancel keys must not be stolen or queued for later execution.
- A host input policy can inspect current native field identity and modifier-bearing key intent; consume only an explicitly handled action, with no implicit business replay.
- Preserve ordinary Enter/newline, modifier submission, rename confirmation/cancellation and palette navigation semantics; native focus, undo/redo, caret/selection and candidate-window geometry remain acceptance requirements.
- Secure inputs must never expose their contents through new hooks. Host policy must be opt-in; unconfigured consumers retain existing behavior except the composition-safety correction.

## Validation and delivery

Verify extraction byte-for-byte against the original archive and keep a manifest of original file hashes. Audit the exact source delta separately from the original imported code. Run local task-related compile, strict Clippy and behavioral regressions after writers stop; do not treat the prior 218-pass suite as validation of new patches. Independent nonimplementer review and latest-head CI remain required. Native macOS/Windows/Linux IME, selection, focus, AX and scrolling acceptance remain separate and unproven.

No release, installation, automatic update, dependency upgrade, new renderer, dormant route activation, acceptance reduction or scroll-platform patch is part of this authorization. Development Rust tests and exclusive support must be archived and removed before final master delivery according to the existing lifecycle; do not discard them before obtaining recoverable evidence.

## Development checkpoint: implemented backend seam

The original archive manifest is `nsh-platform-original-files.json`. The workspace path override now selects `vendor/nsh-platform-gpui` at version 0.4.0, excluded from workspace membership. Cargo.lock changes only this package's source/checksum removal for its path identity; no package version was upgraded. The original font engine/layout/painter remain unchanged except copy interception and snapshot access in the text module.

Implemented public APIs: `AppView::{set_reading_handler, reading_snapshot, reading_snapshot_is_current, input_identity, set_input_hooks}`; types reside in `reading.rs` and `editing.rs`. Reading takeover consumes keyboard/menu/primary requests before built-in clipboard writes, including failure to form a current snapshot. System `SetClipboard` commands are cancelled while reading takeover is installed, preventing built-in code copy from bypassing application source authorization. Production code actions must use a custom action id and perform original-source copy through the trusted application controller; cancellation alone is not completed code-copy functionality.

Editing policy runs synchronously before key holding/keymaps and again on replay. Composition removes waiting key intents, not waiting text; the native event boundary stops host bubbling without `prevent_default`. Atomic edit observations use the native shown text and UTF-16 caret during composition, preserve reversed selections otherwise, carry a checked native serial, and exclude secure input. Focus loss, removal, stop and noneditable/secure configuration retire hooks. Rebinding requires a current exact mount/focus identity; application current-root authority is still mandatory. Native paste interception, compatibility echo suppression, acknowledged programmatic projection and application integration remain unfinished.

Actual local toolchain: Cargo 1.101.0 (huacnlee 0.102.0-9), rustc 1.98.0. `cargo check -p nsh-platform-gpui --offline --locked` passed after formatting. Existing unused workspace `tracing-subscriber` warning remains unrelated.

Six backend development regressions executed/passed, zero ignored or filtered: marked candidate keys bypass business shortcuts/held keys while waiting text remains; reversed UTF-16 and actual marked text atomic observation with secure-input exclusion; stale focus/recycled-node policy rejection; fail-closed detached copy takeover scoped to its Surface; distinct content-object/selection-ABA invalidation; actual mounted two-text select-all and keyboard-copy snapshots with content replacement/removal invalidation. The last test uses an empty real-layout payload for select-all; it does not establish pointer hit testing, font shaping or native paint acceptance.

Harness evidence: ordinary testing of the excluded dependency was rejected by Cargo; temporarily making it a workspace member resolved unused video optional dependencies and failed offline on missing gstreamer metadata, then membership was reverted. `cargo rustc ... -- --test` encountered upstream package tests referencing a gallery font absent from the crate archive and missing GPUI test-support. No upstream assertion was removed or ignored. A development-only `vega-host-regressions` feature exposes the same backend behavior assertions to `crates/vega_ui/tests/nsh_native_host.rs`. `cargo rustc -p nsh-platform-gpui --offline --locked --lib --message-format=json -- --cfg 'feature="vega-host-regressions"'` built the library; a standard `rustc --test` harness linked against that exact rlib using actual Cargo artifact/link paths and executed six tests. Raw build records: `target/nsh-host-build.jsonl`, stderr and `target/nsh-native-host-regressions`. The temporary feature, modules, harness and dev-dependency must be archived/removed at final delivery, not kept conditionally in master.

The combined `cargo check -p vega_ui --offline --locked` currently failed on reading-menu ambiguous IntoElement and sidebar private Pixels.0 access; application writers own these repairs. The prior 218-pass batch does not validate this integration. Strict Clippy, final-version regression rerun, independent review, new-head CI and three-platform native acceptance are pending.

Frozen backend-only delta identity before independent review: `8da3113f306a141a257954efb693e41b03f9fdebf65ed6e27d5aa9b49c7d3a10`, computed SHA-256 over JSON of sorted `(relative path, current SHA-256)` entries differing from the original manifest, including new backend files. Parent stops writing this backend scope for review; concurrent application writers remain disjoint.

## Paste and visual-row backend increment

The previous incremental independent review closed the held-key owner, reentrant hook retirement and queued clipboard ABA findings at backend delta `dac1872d5ab81a71f1b68a0dfc55032d726972670444e5792397dd5496a6a3bc` (static closure, not native acceptance). Its nine regressions were actually executed/passed by the parent.

Twelve-test backend delta: `e65dbc5f01b1bdd7be1ed556d72966f571eef91fdbad8bbc0b5344a75c9c1dac`, SHA-256 of `json.dumps(sorted(entries)).encode()` where entries are changed `(relative POSIX path string, SHA-256)` pairs versus the original manifest. The initial `611481df6382775d3ea98f05e402e2a7130cf66d5f3d549fc513c0e36f48322f` used pathlib Path component sorting before string conversion, not the declared string-pair sort. Parent reproduced both identities from exactly the same 15 file/hash pairs: this was a version-recording algorithm fault, not a source mutation; the twelve-test source bytes remain unchanged. Parent stops backend writes for incremental independent review. Added files/bytes remain development-only work on base `0b5ea426abfd8d4358489e916273d89474fb190f`; no new commit/push/CI is claimed.

Paste begins synchronously on its original keyboard/menu path, closes the application barrier, and captures exact native tuple, focus identity and registration token before asynchronous clipboard access. A revocable session rechecks these before reading and after the borrowed-payload callback, applies only accepted normalized insertion, and delivers one actual transaction serial plus the original payload. Attachment-only completion is explicit. Dropped/stale sessions notify cancellation once; no ordinary edit observer or engine Paste duplicate is emitted for a completed takeover. Secure/unconfigured inputs retain exclusion/original behavior. Current-layout key context uses actual visual line edges and viewport scroll; edit layout invalidation precedes the native report. Application barrier/payload-budget adaptation is still being implemented separately.

Actual commands: `cargo check -p nsh-platform-gpui --offline --locked -j 2` exited 0 (3.10 s); `cargo rustc -p nsh-platform-gpui --offline --locked -j 2 --lib --message-format=json -- --cfg 'feature="vega-host-regressions"'` exited 0 (5.44 s). Toolchain remains Cargo 1.101.0 / huacnlee 0.102.0-9 and rustc 1.98.0. Standard rustc harness linked the actual emitted rlib using compiler artifact/native link paths and executed **12 tests: 12 passed, 0 failed, 0 ignored, 0 filtered** (0.01 s). Existing nine assertions retained; added single-transaction/reversed-selection/original-payload/attachment-only, stale/rebinding cancellation, and soft-wrap/current-scroll context cases. The soft-wrap test injects a native Layout fixture; it verifies query behavior, not actual shaping or native pixels. `git diff --check` exited 0. Raw records: `target/nsh-host-build.jsonl`, `.stderr`, `target/nsh-host-link-command.txt`, `target/nsh-host-link.stderr`, `target/nsh-host-test-results.txt`. These replace earlier counts only for this backend snapshot, not the concurrent application tree.

Incremental review at `e65dbc5f…c1dac` found two product defects: successful completion can synchronously retire focus before engine count/ChangeText accounting; new-hooks-only inputs omit image capability in native menu enablement. Corrective contract: commit unavoidable engine accounting before external completion, retain focus-sensitive caret/reveal afterward without ordinary edit duplication; include nonsecure editable current paste hooks in native menu payload eligibility. Add completion-retirement/old-count rejection and new-hooks-only image-menu regressions. The previous twelve-pass result remains evidence for its frozen version, not proof these findings were covered.

Corrected frozen backend delta: `819a92e66fc8c546c2dc3c0277c3a67b9f1490d4d8e1735f4d114b041b39bbc5` under the string-pair algorithm above. Both findings implemented: `account_text_edit` emits count/ChangeText before external completion; `finish_edit` alone is focus-conditional afterward. Native menu payload capability includes registered nonsecure editable hooks. Added two regressions with real menu items and an actual image ClipboardItem, plus completion callback blur checking native count, original ChangeText payload and rejection of an old count engine value. A development-feature-only observer records the real Surface event boundary without replacing its production dispatch; archive/remove it with test wiring at final delivery. Actual same-backend standard harness: **14 executed / 14 passed / 0 failed / 0 ignored / 0 filtered** (0.01 s). First compile exposed the test's `new_image` needing a borrowed Image; corrected that harness call, not product assertions. Production-feature `cargo check -p nsh-platform-gpui --offline --locked -j 2` exited 0 in 1.25 s; `git diff --check` exited 0. Previous raw target records now contain this fourteen-test version. Parent stopped vendor writes for affected-increment review. The independent reviewer confirmed this exact identity at start/end, closed both findings, and found no new issue in their affected contracts; this is static closure, not native acceptance.

Remaining: opaque GPUI string metadata is not representable in existing PasteEvent; the existing native mapping currently carries text/image bytes/local URIs. Parent inspected the old production `text_input/state.rs::paste`: it consumes text/image/file entries, not opaque string metadata. That establishes the old consumer boundary but does not prove preservation in the new native conversion; full opaque metadata fidelity remains unclaimed. Native AX paste routing, app integration/Clippy/regressions, independent increment review, final head CI and three-platform acceptance are not established by these results.

## Continuous selection observation increment

Approved reading package continuation: add one exact-area listener per AppView, registered with the currently mounted area identity and an `Rc<dyn Fn(Option<ReadingSnapshot>)>`. Deliver initial/current actual snapshots, empty/cleared selection as None, and revoke with None on detach/replacement/Surface stop. Snapshot collection and user callbacks run only after releasing native selection/model/registration borrows. Cache publication before callback to prevent unchanged recursive notifications; reentrant replacement must not reinstall old callbacks. This observer never requests copy or writes a clipboard. App integration must still revalidate source/root/mount when accepting notifications. Endpoint installation is a separate still-pending increment; this listener alone does not preserve source-trim anchors or selection direction.

Reading writer recovery: original agent `01M6W3C65DCGWDFMCWX51XXWS1` failed/released without a delivery declaration, no unknown effects in its checkpoint. Existing menu/controller/Thinking-owner code was preserved and handed to replacement `01M6W60AGJ7YG1Q7HQB2VCJK1R`; main body/source mapping remains in progress, not delivered. Tool writer reported an additional wheel consumption/remainder handoff dependency gap outside the two approved packages; no wheel patch has been made.


## Final post-cleanup source provenance (supersedes preceding current-state records)

Observed UTC `2026-11-10T00:47:04.189967+00:00` with Python **3.15.2**. **This appended section and the refreshed current manifest supersede every preceding current-state identity/count and verifier result, including the opening development-only observation section.** All preceding document bytes remain unchanged as historical evidence; their compiled/test/native results apply only to their recorded source snapshots. This is final-source provenance, **not implementation or independent behavior/security review**.

Source is **STOPPED** per parent at HEAD `c5cd4ffeceda9665dace2602f1d2429fbf9849b1` (branch `feat/907-nsh-full`). Read-only Git object comparisons verified that all current files in both vendor packages exactly match HEAD, not an earlier development tree. The documentation refresh makes the worktree documentation-dirty; it does not change source. Development source remains archived at `ef3dddd6791e3607e40b1b1bd8adb60b48979752` with evidence tag `evidence/907-nsh-development-ef3dddd6791e`; the local tag resolves exactly to that commit. The remote tag is parent-provided evidence, not re-queried over the network. No fetch/update, Git mutation, source/Cargo/lockfile write, network, build or test was performed under this documentation-only authorization. Other source reviewers and parent final gates remain independent.

Compared with prior manifest SHA-256 `73318b7f0de0621ea9537f6704fdbec7a89fc75940c5073ed1ed3b9681cc64fc` and that exact archived vendor map: core is byte-identical; platform has **12 retained files modified, 10 development-only files removed and 0 files added**. The archived platform has exactly **32** `#[cfg(feature = "vega-host-regressions")]` regions; the final packages contain **zero occurrences** of that feature. These are static counts, not executed tests. The manifest records every removed path and the preceding/current hashes of the affected paths. Cleanup scope: [inventory §8](nsh-final-cleanup-inventory.md#8-vendor-boundary-owner-v).

Platform `Cargo.toml` and `src/rt.rs` now exactly match the upstream archive, reducing changed platform originals from **14 to 12**, not 14. No upstream original file is missing. The **six production capability additions** remain: `src/editing.rs`, `src/events/scroll_bridge.rs`, `src/reading.rs`, `src/reading/geometry.rs`, `src/reading/opacity.rs`, `src/scroll_scope.rs`. Three of these are mixed files with development regions removed; retained means present, not byte-identical to the development snapshot. The ten development additions listed in §8 are absent. This inventory does not certify production-behavior equivalence or complete original-test acceptance.

| Scope | Original | Current | Unchanged originals | Changed originals | Added | Missing |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| nsh-core | 180 | 180 | 179 | 1 | 0 | 0 |
| nsh-platform-gpui | 45 | 51 | 33 | 12 | 6 | 0 |
| Total | 225 | 231 | 212 | 13 | 6 | 0 |

| Final source identity | SHA-256 |
| --- | --- |
| nsh-core complete tree | `dd6cc201bddb9508db77d0470c1b1f12dda9e32fa8e6f1de452feb8fd804afa1` |
| nsh-core delta | `93756724e4b15be0527844ddc7af302153ea1ba90c100a614dd45efc9b9fda8d` |
| nsh-platform-gpui complete tree | `0177d081d33f90724a9f220546400d2c6fe219b5627522da6b60badf5b26577c` |
| nsh-platform-gpui delta | `aac7fc3a1ed7d6a23448d9cf387c187b4633f27e70748bdddfbe5227e16a04b2` |
| Combined complete tree (231 entries) | `fb8603eb20796b571b3a38865904f22e052649b495ecadbfc4443ae068e74998` |
| Combined delta (19 entries) | `942a2d3955cb383da4a1fc414b8e72f7307257c291350b513e82bb369fc877a5` |
| Current manifest at publication | `e7dcc6c51c217d48e323a2a57fa6296b3ec070ca07ff6df05718ace2469a8ba9` |

Exact algorithm: `hashlib.sha256(json.dumps(sorted(entries)).encode()).hexdigest()` over `(relative_POSIX_path_string, SHA256_hex_string)` pairs, lexically sorted as strings using Python default JSON serialization. Package paths are package-root-relative; combined paths include `nsh-core/` or `nsh-platform-gpui/`. Every regular file, including hidden files, is included; missing originals are tracked separately, with no symlinks/nonregular entries.

Both original manifests remain byte-identical: core `6e0856cdbc345291557ff5dfe3d3e0d14e78cb29a742b791f067d93e78ca34fd`, platform `612172f1c99ce52e34ad9fce8ef545a2c1eda190d0544e2b9210b66d6e325037`. Cached original archive SHA-256 values remain `171f17b402bedb0cead975d3abcefbf0e7963583fbdbf1dbcb03ef73ba11191d` and `1e3828245c26e734567b3e6058398c0e4d5809063636460f990a0eaf8a0f74c3`. Their **complete 180/45 regular-file maps** exactly match the respective original manifests; no archive links were found. Both normalized Cargo manifests and `Cargo.toml.orig` now match the originals. Package VCS metadata remains `e626a8dd8f8d8b277b9507cfd5ac74d0e67020f3`. NSH `0.4.0`, Kit `0.7.3`, Fast/platform `0.1.9` pins match the current lockfile. Original manifests, cached archives, historical ledger and pins were not rewritten. `observed_origin_master` is retained only as a historical observation; no fresh remote/ancestry, upstream network authenticity or Kit/Fast source audit is claimed.

### Final read-only provenance verifier

The earlier verifier's hardcoded **241/31** development counts are historical and do not apply to the cleaned source. Run this replacement from the selected worktree; it reads only repository files and the allowed cached original archives and performs no Git/Cargo/network/native execution:

```bash
python3 - <<'PY'
import hashlib, json, pathlib, tarfile, tomllib
H = lambda data: hashlib.sha256(data).hexdigest()
A = lambda files: H(json.dumps(sorted(files.items())).encode())
m = json.loads(pathlib.Path('docs/907/nsh-backend-current-manifest.json').read_text())
cleanup = m['snapshot_increment']
assert m['head'] == 'c5cd4ffeceda9665dace2602f1d2429fbf9849b1'
assert len(cleanup['removed_development_added_files']) == 10
assert cleanup['archived_development_gate_count'] == 32
assert cleanup['current_development_gate_count'] == 0
for rel in cleanup['removed_development_added_files']:
    assert not (pathlib.Path('vendor') / rel).exists()
combined, delta = {}, {}
for crate, record in sorted(m['packages'].items()):
    root = pathlib.Path(record['root'])
    paths = list(root.rglob('*'))
    assert not root.is_symlink() and not any(p.is_symlink() for p in paths)
    assert all(p.is_file() or p.is_dir() for p in paths)
    files = {p.relative_to(root).as_posix(): H(p.read_bytes()) for p in paths if p.is_file()}
    original_path = pathlib.Path(record['original_manifest'])
    original = json.loads(original_path.read_text())
    assert H(original_path.read_bytes()) == record['original_manifest_sha256']
    assert files == record['files']
    assert not record['development_only_added_files']
    assert not record['mixed_development_cleanup_files']
    assert files['Cargo.toml'] == original['files']['Cargo.toml']
    assert files['Cargo.toml.orig'] == original['files']['Cargo.toml.orig']
    assert not any(b'vega-host-regressions' in p.read_bytes() for p in paths if p.is_file())
    changed = {p: h for p, h in files.items() if p in original['files'] and h != original['files'][p]}
    added = {p: h for p, h in files.items() if p not in original['files']}
    missing = sorted(set(original['files']) - files.keys())
    assert sorted(changed) == record['changed_original_files']
    assert sorted(added) == record['added_files']
    assert missing == record['missing_original_files'] == []
    assert changed | added == record['delta_files']
    assert record['counts'] == dict(original=len(original['files']), current=len(files), unchanged_original=len(original['files'])-len(changed)-len(missing), changed_original=len(changed), added=len(added), missing_original=len(missing))
    assert A(files) == record['tree_sha256']
    assert A(changed | added) == record['delta_sha256']
    assert A(original['files']) == record['original_tree_sha256']
    dev = set(record['development_only_added_files'])
    production = set(record['capability_added_files'])
    assert not dev & production and dev | production == set(added)
    assert json.loads((root/'.cargo_vcs_info.json').read_text()) == record['vcs_info']
    assert record['vcs_info']['git']['sha1'] == m['upstream_commit']
    assert tomllib.loads((root/'Cargo.toml').read_text())['package']['version'] == record['version']
    archives = sorted((pathlib.Path.home()/'.cargo/registry/cache').glob(f'*/{crate}-0.4.0.crate'))
    assert archives
    for archive in archives:
        assert H(archive.read_bytes()) == record['archive_sha256'] == original['archive_sha256']
        with tarfile.open(archive) as tar:
            assert not any(x.issym() or x.islnk() for x in tar.getmembers())
            archived = {x.name.split('/', 1)[1]: H(tar.extractfile(x).read()) for x in tar.getmembers() if x.isfile()}
            assert len(archived) == len([x for x in tar.getmembers() if x.isfile()])
        assert archived == original['files']
    combined.update({f'{crate}/{p}': h for p, h in files.items()})
    delta.update({f'{crate}/{p}': h for p, h in (changed | added).items()})
    print(crate, record['counts'], 'archive-map=exact')
    print('tree', A(files), 'delta', A(changed | added))
assert len(m['packages']['nsh-platform-gpui']['capability_added_files']) == 6
assert len(combined) == m['combined']['current_file_count'] == 231
assert len(delta) == m['combined']['delta_file_count'] == 19
assert A(combined) == m['combined']['tree_sha256']
assert A(delta) == m['combined']['delta_sha256']
print('combined-tree', A(combined), 'combined-delta', A(delta))
lock = tomllib.loads(pathlib.Path('Cargo.lock').read_text())
for name, version in m['pinned_packages'].items():
    packages = [p for p in lock['package'] if p['name'] == name]
    assert len(packages) == 1 and packages[0]['version'] == version
print('pins=unchanged; VCS metadata=exact; symlinks/missing originals=0')
print('manifest-sha256', H(pathlib.Path('docs/907/nsh-backend-current-manifest.json').read_bytes()))
print('PASS provenance only; no compilation, behavior/security review or native acceptance')
PY
```

### Final verification boundary and stopped handoff

The replacement verifier is the final-file static check; its actual execution result is recorded below. The separate read-only archive/HEAD comparison established the exact 10 removed paths, 12 modified retained paths, 32 archived gates and current HEAD file mapping. Neither check executes original or development tests. **No original full-test acceptance, native/OS acceptance, CI success, workspace test discovery/execution count, merge approval or independent review is established here.** Prior raw validation ledger text and historical manifest increments remain untouched; they are not promoted to acceptance of this final source. Parent owns final review, final-head required CI and user/native acceptance. This documentation lane stops after publication.

Executed the exact appended verifier through `subprocess.run(['bash', '-c', command])` with Python **3.15.2**: **exit 0**. Both archive maps were exact; final counts and all six tree/delta identities matched the tables above; pins and VCS metadata were exact; missing originals and symlinks were zero. `git --no-optional-locks diff --check -- docs/907/nsh-backend-current-manifest.json docs/907/nsh-authorized-backend-patches.md` exited **0**. Read-only changed-path enumeration found only these two authorized documentation files. The preceding **61,752 document bytes** remain an exact prefix with SHA-256 `dbbbda3512476ad427e52e88a6e94a2817723ee61d7253da3792248aefb0bdb6`; previous manifest increments are preserved as historical records. The first verifier execution exited **1** because the newly added duplicate-entry check called `TarFile.getmembers()` after archive closure; only that appended check's indentation was corrected to keep it inside the open archive context. No assertion was removed or weakened, and no source or archive repair was needed. **STOPPED** after this documentation/provenance publication; final review and parent gates remain independent.
