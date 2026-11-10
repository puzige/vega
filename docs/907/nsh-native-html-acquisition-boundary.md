# #907 native HTML acquisition boundary

## Status and scope

Author-side remaining implementation capability inventory, **not independent review**, consolidated migration approval, or native acceptance. Production is unchanged. This note answers only whether absent OS HTML acquisition is a migration regression or a preserved pre-existing limitation; it does not audit other paste behavior.

Observed source snapshot: `2026-11-09T06:38:42.460011+00:00`, dirty worktree `vega-907-nsh-full`, HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`. Dirty source identities below, not HEAD alone, bind this inventory. Original migration baseline: `d4beacac2f01d4c09af7545163393e9d43753fe9`, as specified in [nsh-full.md:5](nsh-full.md#用户裁决与范围).

This continuation authorizes only this new document. No OS clipboard access, application launch, runtime configuration/credential access, Rust/vendor/Cargo changes, build/test execution, network access, or Git mutation occurred. Read-only Git object inspection established the original baseline; no fetch/update was attempted under the explicit no-network/no-Git-mutation instruction. Existing documents and historical failures remain untouched.

## Conclusion for implementation closure

**Absence of native OS HTML acquisition is a real capability limitation, but the inspected evidence identifies it as pre-existing, not a capability lost by this migration.** The original GPUI TextInput paste action did not acquire or interpret HTML. The current native NSH action still acquires the pinned GPUI `ClipboardItem`, which has no HTML entry. Preserving opaque string metadata is not HTML acquisition.

Consequently, this absence alone is **not an evidenced blocker to the existing all-UI migration/no-regression acceptance**. It is also **not evidence that native rich HTML paste works**. Do not mark native HTML acquisition as implemented or tested; do not downgrade any originally supported text/image/path behavior, omit a production surface, retain an old GPUI editor as fallback, or declare the entire migration accepted from this finding. [nsh-full.md:7–13](nsh-full.md#用户裁决与范围) still requires the full production switch, unchanged functionality/security contracts, independent review, final required CI, and macOS/Windows/Linux native acceptance.

There is a distinct **NSH schema-to-desktop capability gap**: `PasteEvent.html` describes an HTML representation, but the pinned desktop acquisition path cannot provide it. If “all migration” is intended additionally to require actual browser/office HTML bytes at the native application boundary, that is a concrete missing implementation and an authorization blocker, not something that synthetic tests or the original GPUI editor can satisfy. The precise extension decision is below. This note neither invents that new requirement nor waives it if the user confirms it.

## Acceptance and authorization sources

| Source | Binding meaning for this question |
|---|---|
| [nsh-full.md:7–13](nsh-full.md#用户裁决与范围) | All production UI migrates, GPUI remains a desktop host/controller boundary, and existing functionality/security acceptance is not reduced. “Preserved limitation” does not authorize partial migration. |
| [nsh-full.md:98–105](nsh-full.md#输入桥冲突恢复) | Original-gesture paste payload, ordered delivery to the original controller, no later clipboard reread, CRLF normalization, no local-path text insertion, preservation of available mixed text/images, bounded image/URI handling and original importer validation. Line 105 does not add a requirement to acquire HTML or render formatted clipboard content. |
| [nsh-native-paste-fidelity.md:5–16](nsh-native-paste-fidelity.md#acceptance-fixed-before-implementation) | Explicit Native/Synthetic alternatives, complete original item fidelity, metadata not HTML/business authority, no copied normalized image buffer, existing budgets and transaction fences preserved. OS HTML acquisition is expressly excluded. “Derives normalized text/HTML/URI decisions” at line 9 is not proof that the item contains HTML; the native HTML decision is currently empty. |
| [nsh-editing-backend-contract.md:5–19](nsh-editing-backend-contract.md#production-editing-closure-acceptance-current-continuation) | Native editing closure retains the existing carrier; OS HTML and AX Paste expansion are excluded. AX SetValue is ordinary full-display replacement, not clipboard acquisition. |
| [nsh-authorized-backend-patches.md:9–14,180](nsh-authorized-backend-patches.md) | Original minimal fixed-version reading/native-editing patches, then specifically approved synchronous geometry-conditional source-caret and complete nested-wheel protocol. OS HTML acquisition, Kit/Fast source changes/upgrades and unrelated dependencies remain unauthorized. |
| [NSH ops.rs:502–519](../../vendor/nsh-core/src/ops.rs) | `PasteEvent` can carry plain text, HTML, files and URIs; its HTML field is documented as the formatted representation, empty when none was held. This is the rich payload schema, not evidence of desktop acquisition or a Vega rich-document editor requirement. |

The phrase “rich paste” must distinguish three claims: (1) original GPUI entry/metadata/image/path fidelity, (2) representation of already-supplied HTML in an NSH event, and (3) acquisition of actual native OS HTML. Current implementation evidence supports different boundaries for each; none implies all three.

## Actual current production call chain

Paths below are relative to the repository root unless explicitly marked `CACHE`.

1. **Production entry, not only preview:** `crates/vega_ui/src/conversation_stream/render.rs:363` calls `render_nsh_composer_input`; `conversation_stream/nsh_composer.rs:5–15` calls `TextInput::render_nsh_composer`. `text_input/nsh.rs:700–706` selects the transactional editor. `nsh.rs:746–768` installs the actual adapter hooks using the native input identity. `nsh.rs:1866–1870` sets `PasteBehavior::Report`; its ordinary `on_paste` handler immediately returns for the transactional editor.
2. **Native gesture:** `vendor/nsh-platform-gpui/src/text_input.rs:2677–2681,2705–2709` routes command-V / Shift-Insert into `paste`. At `2904–2953`, the backend captures input identity/registration and a pre-edit snapshot, invokes `paste_begin`, rechecks focus/current registration/session, then performs **one** `cx.read_from_clipboard()` for that accepted transaction at line 2949. It constructs `InputPastePayload::native(item)` and applies the session. This is an application clipboard API call, not a claim that the underlying platform performs only one OS data-format read. Menu eligibility checks are not this transaction's payload source; there is no second read to enrich its payload with HTML.
3. **Locked API acquisition:** Kit's `gpui-fast` feature reexports Fast (`CACHE/gpui-kit-0.7.3/src/lib.rs:100–103,128–130`). `CACHE/gpui-fast-0.1.9/src/app.rs:1708–1711` delegates `read_from_clipboard` to the platform. Fast's `platform.rs:2985–2990,3026–3035` defines `ClipboardItem { entries }`, with only `String(ClipboardString)`, `Image(Image)`, and `ExternalPaths`. There is no native HTML variant or HTML getter in this carrier.
4. **Native carrier:** `vendor/nsh-platform-gpui/src/editing.rs:57–68,116–180` defines `InputPastePayload::{Native,Synthetic}` and `NativeClipboardPaste`, privately owning the original optional `gpui::ClipboardItem`. Text is derived from `item.text()`, absolute external paths become file URLs, files borrow original images. At **156–160, `Native` HTML is always `""`; only `Synthetic` returns `paste.html`**. There is no declared type named `NativeClipboardItem` in this inspected API: the concrete original item is GPUI `ClipboardItem`, wrapped by `NativeClipboardPaste`. “Native clipboard item” must not be mistaken for a richer OS-format carrier.
5. **Synchronous authorization and completion:** `text_input/native_adapter.rs:271–342` validates the current transaction, reserves existing budgets, validates image signatures and computes insertion from `payload.text()` or empty insertion for file URIs. `vendor/.../text_input.rs:2808–2897` rechecks currentness, applies the authorized insertion and completes with the same `Rc` payload. `native_adapter.rs:345–385` requires the same allocation and unique ownership, then queues `NativeDelivery::Paste`; no HTML conversion or clipboard reread occurs.
6. **Authority and original attachments:** `text_input/nsh.rs:981–1002` consumes that delivery through `apply_native_paste`. Its `commit_native_transaction` at `490–576` validates text/selection/mutation, uses `take_original()` for Native attachments, and emits the original item through `ImagePaste`. `nsh.rs:579–596,1364–1385` inserts CRLF-normalized text, not HTML, and does not insert external-path text. `conversation_stream/core.rs:485–491` forwards `ImagePaste` to `paste_images`; `conversation_stream/attachments.rs:117–153` handles image bytes/external paths and ignores string entries. This downstream importer does not recover HTML from metadata.

### Concrete source-predicted observable semantics

These are static consequences of the cited path, **not results from reading the actual OS clipboard or executing a native paste**:

| Available input to the native transaction | Application behavior relevant to HTML |
|---|---|
| Ordinary source publishes text plus native HTML | Whatever text the pinned GPUI platform exposes is eligible for normal text insertion; actual native HTML bytes are not exposed. Formatting is not converted/preserved as rich document content. |
| Source publishes only HTML, with no recognized text/image/path representation | No HTML-derived insertion is available. If the platform returns no recognized entries, the carrier is Native(None); the application cannot reconstruct HTML from that. This is not a claim about what representations any particular browser/office app actually publishes. |
| String metadata contains HTML-looking or non-JSON bytes | Original metadata stays opaque and associated with its original string entry. `ClipboardItem::text()` ignores metadata (`CACHE/gpui-fast-0.1.9/src/platform.rs:3071–3097`). It is not inserted as HTML or trusted as an attachment/business command. |
| Original item includes supported image/path entries | Existing native attachment transaction delivers the original item; HTML is still empty. Available mixed text/images are preserved at the carrier/application boundary, but this does not prove every OS adapter acquires every offered representation together. |
| Synthetic `PasteEvent` includes nonempty HTML | `InputPastePayload::Synthetic` represents that HTML and budgets include its bytes (`text_input/nsh.rs:1388–1412`); insertion still uses text, not HTML. This proves neither OS acquisition nor formatted editing. An HTML-only synthetic event is not an HTML-to-text converter. |

No unconditional fallback, replay, second clipboard read, conversion of opaque metadata, or old-editor fallback is proposed.

## Pinned desktop acquisition boundary

`Cargo.toml:95–106` pins Kit **0.7.3**, Fast/Fast platform **0.1.9**, and NSH **0.4.0** with only the existing local NSH replacements relevant here. The original baseline TOML/lockfile contains the same Kit/Fast versions and registry package checksums. `CACHE/gpui-fast-platform-0.1.9/Cargo.toml:79–116` selects the desktop Fast platform crates. This is inspection of local cached source files selected by package identity, not a fresh package-integrity audit, build-linkage proof, or authorization to edit them.

| Desktop | Exact cached source evidence | HTML boundary |
|---|---|---|
| macOS | `gpui-fast-macos-0.1.9/src/platform.rs:1232–1235` delegates to `Pasteboard::read`; `src/pasteboard.rs:52–91,114–140` reads paths first, then `public.utf8-plain-text`, then supported images. | No `public.html` acquisition in that read path. `zed-text-hash` / `zed-metadata` at lines 47–48,126–140 are app-specific associated metadata validated against text hash, not the HTML pasteboard flavor. The read priority can return text before an offered image; preserving an injected mixed item does not prove simultaneous OS acquisition. |
| Windows | `gpui-fast-windows-0.1.9/src/clipboard.rs:27–46,92–127,235–255` recognizes Unicode text, images and HDROP paths. | No `HTML Format` branch/carrier. `GPUI internal metadata` and text hash are separate registered formats, not HTML. |
| Linux X11 | `gpui-fast-linux-0.1.9/src/linux/x11/clipboard.rs:69–89,1022–1072` requests supported image and plain-text atoms, returning image or string. | `text/html` is only a commented-out atom at line 80, not requested/acquired HTML. |
| Linux Wayland | `gpui-fast-linux-0.1.9/src/linux/wayland/clipboard.rs:16–22,98–134,193–209` uses cached/own contents, otherwise accepted plain-text MIME then image. | No HTML read. An own cached `ClipboardItem` still has only the same three GPUI entry types. |

These format priorities are recorded only to bound the HTML claim; unrelated acquisition differences are not investigated or repaired here.

## Original GPUI behavior: historical bytes, not inference from current metadata tests

Read-only `git show d4beacac2f01d4c09af7545163393e9d43753fe9:crates/vega_ui/src/text_input/state.rs` establishes the original `TextInput::paste` at **629–665**:

- Calls `cx.read_from_clipboard()` once.
- For enabled image paste and image/path entries, inserts available text unless external paths are present, then emits the original `ImagePaste` item.
- Otherwise obtains `item.text()`, normalizes line endings for multiline or flattens newline characters for single-line input, and inserts text.
- Does not read HTML, inspect metadata, convert HTML to text, or preserve formatted document spans.

The current retained old action at `state.rs:631–667` has the same relevant logic. The original baseline `Cargo.toml:95–97` and locked package identities/checksums match Kit 0.7.3 / Fast 0.1.9. Thus both original and current native implementations are bounded by the same HTML-less GPUI carrier; switching rendering to NSH did not remove an original HTML acquisition call. This establishes the source baseline, not that a particular released binary, clipboard producer, or prior native session was tested here.

Original-item fidelity regressions using metadata (including HTML-looking strings) establish preservation of those original bytes, **not** acquisition of OS HTML. Likewise `PasteEvent.html` and synthetic HTML test fixtures establish representation at most. Neither can close the desktop acquisition gap in the NSH field's generic documentation.

## Facts, unknowns and exact decision boundary

**Established here:** native HTML is absent; metadata is not HTML; original GPUI paste used the same carrier without HTML extraction; the fixed desktop source paths inspected do not acquire an HTML representation; the full-migration spec preserves original paste semantics and the explicit approved backend extensions exclude OS HTML acquisition.

**Not established here:** actual clipboard formats from any producer, cross-platform clipboard behavior at runtime, native focused-editor/IME/menu acceptance, package source integrity, final linked executable contents, final-head CI, or independent review. No new test cases were authored or proposed as a substitute for these missing observations. This document does not transfer any previous green run to these runtime claims or resolve the parent's separate keyboard recovery failure.

**Default action:** leave production unchanged and continue the existing full-migration acceptance without claiming HTML acquisition. No new user decision is necessary merely to preserve the evidenced baseline limitation. This is classification, not a waiver of no-regression acceptance or a conclusion that all implementation/gates are complete.

**If actual native HTML is required for closure, the precise user decision is:**

> Must #907 additionally acquire the source's native HTML representation on macOS, Windows and Linux, beyond the original GPUI text/image/path behavior? If yes, authorize a minimal same-version OS acquisition and typed carrier extension, specify whether acceptance requires only faithful HTML-byte delivery or also HTML conversion/formatted editing, and explicitly decide whether any required pinned Fast platform source patch or additional platform dependency is permitted. Kit/Fast versions must remain locked unless separately authorized; the present approval permits neither their source changes nor upgrades. No implementation begins before that decision.

Smallest candidate extension **if that requirement is confirmed**, not an implementation authorization:

1. Capture actual HTML alongside original text/image/path representations in the same gesture-bound acquisition, with a dedicated typed field, not opaque metadata. Carry it through the same immutable transaction and completion identity; preserve all original entry ordering/association and existing fences/budgets/no-reread/no-replay behavior.
2. Wire `Native`'s HTML view to the acquired bytes. Updating the getter or inventing a synthetic event alone is insufficient because no current `ClipboardItem` field contains those bytes. HTML storage must remain inside existing non-image/in-flight accounting; no image/text limits are raised or accepted payload ranges reduced to pay for duplication.
3. Native acquisition necessarily needs an authorized platform-facing seam: macOS HTML pasteboard flavor, Windows HTML clipboard format, and Linux HTML MIME offers. The current locked public carrier does not expose them. Whether a same-version NSH/platform adapter can supply that seam without changing Fast sources or adding dependencies is **not established**; this note does not choose an unapproved bypass or reopen a wider paste audit. A patch to pinned Fast platform sources, if actually necessary, requires explicit additional authorization, not just the prior NSH-patch approval.
4. Keep formatted-document editing / HTML-to-text conversion separate: the current authoritative editor inserts plain text. If those semantics are required, byte acquisition alone cannot satisfy them and they need their own explicit acceptance decision. Do not silently substitute a converter, renderer, dependency upgrade, or plain-text claim for a confirmed HTML requirement.

Conditional blocker report: **[BLOCKED] #907 | Confirmed native HTML acquisition acceptance would exceed the current approved backend scope and locked HTML-less carrier | Original GPUI HTML support and metadata-as-HTML are ruled out by source | Obtain the exact acquisition/delivery semantics and narrow platform/carrier authorization above, then implement and verify that increment before claiming it complete.** Under the presently evidenced baseline-preservation requirement, this conditional blocker is not asserted as a new migration stop.

## Exact source identities

SHA-256 of actual bytes read from this snapshot. `CACHE/` means the local Cargo registry source directory `index.crates.io-1949cf8c6b5b557f`, not a repository path. `BASELINE/` means raw `git show` bytes at `d4beacac2f01d4c09af7545163393e9d43753fe9`. No package-wide integrity or final-head claim follows from hashing these focused files.

| File | SHA-256 |
|---|---|
| `AGENTS.md` | `27d2c38226861c2cf860efcdb9ff70cf15284ffd07a27d7f5296126febb895a0` |
| `docs/vega-exec-guide.md` | `790d59e2a106b8faec06610911f48fb4b8fc91a04079158bf0815f06d02b3fa6` |
| `docs/907/nsh-full.md` | `189b0b262ff1a6935f57e948207a30c0491ce9ef4006e29e391307a8762353f1` |
| `docs/907/nsh-authorized-backend-patches.md` | `e818a07b2a93751a41ca62a17ceb3df18810a0e39a0deeae1b1a1aa8d77babe5` |
| `docs/907/nsh-native-paste-fidelity.md` | `dabcc370a1b618fee8a7e668056e1d115c384d012092d3ecd7822cf35e6537d1` |
| `docs/907/nsh-editing-backend-contract.md` | `36122a3a366d986656f305b6946236638d1008a2747e061eb724b23cf0826fe8` |
| `Cargo.toml` | `1d516f3491e8a782b1b6ef3b7bdc34a92147dba4e9e4dab011c5da521844f2d9` |
| `Cargo.lock` | `948d0839ab76c6256e0af190d84998d3f3505d3d9f24e27f1be0423955529eae` |
| `crates/vega_ui/src/conversation_stream/render.rs` | `64174d622992fd53901ffa16787a42b15914067b331b648ec40a900b96163712` |
| `crates/vega_ui/src/conversation_stream/nsh_composer.rs` | `55d67420c99251e259fcd9879c1722ff2a9cc760111bb9eb579a68c7ecc9f6dd` |
| `crates/vega_ui/src/conversation_stream/core.rs` | `5222be27db179c54076f4a162ca9ea04c0ff04d6ee616b8674e08bbc8cd23270` |
| `crates/vega_ui/src/conversation_stream/attachments.rs` | `a73b8af8278cd4ea35d5db1987045eaa9505655b174541f50cfb754b62b9c437` |
| `crates/vega_ui/src/text_input/state.rs` | `a228a83b55bb6e014f72a984cae2ea56ff964eb9d81e29b8b744c6b3c14c2226` |
| `crates/vega_ui/src/text_input/nsh.rs` | `6c330e1ab0d8695a1ed24b032954353fd955f858fc1e5ff0e16936907428205c` |
| `crates/vega_ui/src/text_input/native_adapter.rs` | `d724f6bb29b56cabe7390d359a293f690aa5a3efaa0cfa3ed0a4c585f9f81ab1` |
| `vendor/nsh-platform-gpui/src/editing.rs` | `f204864f88ce7e7a07c9d8f0af2815fbcd95fb15848fbb717c2faa554150689a` |
| `vendor/nsh-platform-gpui/src/text_input.rs` | `e27b80250ce6d8ec7ab62d60b8e0b84045c2b7e133e8dd4dc17488a2923601e0` |
| `vendor/nsh-core/src/ops.rs` | `85f7da0fa8d793d6399714f824ee5a467a03415179742cddeca107fc5f866e06` |
| `CACHE/gpui-kit-0.7.3/src/lib.rs` | `1d28656a7f04a263b2ed9762947872c162ab940047ef17493a912c7a532c2c9e` |
| `CACHE/gpui-fast-0.1.9/src/app.rs` | `b276f96b16e7bc81c403a595931afea6f6d43040697dea9ac73c7687e210f13f` |
| `CACHE/gpui-fast-0.1.9/src/platform.rs` | `cb761f9a34e1c21edca910ae7d5dcf9d59f59c16ffe864ded7a2a4b005e5321f` |
| `CACHE/gpui-fast-platform-0.1.9/Cargo.toml` | `10b3bbafc3a0acf9df94f9246882fc2a1c3d53145c69aab8ae2bd36b9661ea9e` |
| `CACHE/gpui-fast-macos-0.1.9/src/platform.rs` | `1bf0adb67969e7389063de4d54efa70e9bb0a7ac7331a74ba74245d3bb87d801` |
| `CACHE/gpui-fast-macos-0.1.9/src/pasteboard.rs` | `66744f8c74061c999547400dc9e3dccbc40ddd5c4749c60856ee3d5ce2b77a23` |
| `CACHE/gpui-fast-windows-0.1.9/src/clipboard.rs` | `eb68368598663c85916328d03582435d923e2b6eeae87a7a365a386848b87bf9` |
| `CACHE/gpui-fast-linux-0.1.9/src/linux/x11/clipboard.rs` | `580eb2519d8c38422b3e5baba24a11e6fa5e942392c9a9e74abedfa3fa84c0f3` |
| `CACHE/gpui-fast-linux-0.1.9/src/linux/wayland/clipboard.rs` | `7dab6bf97638ed62a6f6ebb7d6f435c6663631ee759a48b69ca4037cbda54df1` |
| `BASELINE/crates/vega_ui/src/text_input/state.rs` | `925213e1e2e921bb9495eb41bf0ed9fbe5344d7c19dc452ba6c52eaf5f0471eb` |
| `BASELINE/Cargo.toml` | `9f7f8c4e45646c4e02b85dcfefe89a936e09fc2d40b92ea7c7ead64c0446113c` |
| `BASELINE/Cargo.lock` | `9f77d9f93708a68261ef748b0455297f01c0262b9879b1e44ccff6b8ba03d65b` |

## Verification and stop boundary

Performed focused specification/source reads, read-only original-baseline object inspection, actual-byte SHA-256 capture, and documentation-only static checks. No Cargo/rustc invocation, compilation, runtime paste, native/OS acceptance, CI query, test execution/expansion, or independent review is part of this inventory. The new document's final hash is reported after readback; it is deliberately not embedded as a self-referential hash. Stop writes after the document is checked and hashed. Any subsequently changed cited source must be re-inspected before reusing this snapshot's conclusion as current-byte evidence.
