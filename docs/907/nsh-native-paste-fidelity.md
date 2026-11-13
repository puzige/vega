# Native paste fidelity: approved editing increment

## Acceptance fixed before implementation

Scope remains NSH 0.4.0, Kit 0.7.3 and Fast 0.1.9, with only the five assigned Rust files and this document writable. Parent coordinates compilation, exports and independent review. No Core/Fast, dependencies, registry, HTML OS acquisition, AX actions or importer redesign.

Revision: the previous allowance to reject otherwise valid multi-image pastes because original and normalized buffers together exceed 16 MiB is withdrawn. It was a functional regression, not an acceptable safety restriction. Preserve count 4, per-image 8 MiB, total image 16 MiB, non-image storage 2 MiB, queue 64 and cumulative budgets without increasing any limit.

- Read once. Native and Synthetic are explicit alternatives in one immutable Rc carrier. Native privately owns the original optional ClipboardItem and derives normalized text/HTML/URI decisions from that same item. It does not construct an incomplete PasteEvent or duplicate image bytes. Synthetic owns a genuinely complete PasteEvent.
- Expose a temporary borrowed file view that couples each file's description with its bytes. Native files directly borrow the corresponding original image; no parallel descriptor/data arrays or zip association. Opaque metadata is neither HTML nor business authority.
- Preserve all original entries, ordering, text/metadata association, None, empty and arbitrary non-JSON metadata, image effective bytes/id/format, and external paths. Spare image Vec capacity and buffer pointers are not identity semantics.
- Before authorization retention, uniquely owned native image buffers may be tightened with into_boxed_slice().into_vec(). Check actual capacity before reserve. This operation can transiently reallocate; do not claim a single-copy peak or allocator RSS bound. The authorized queue retains one image allocation per original image, not original plus normalized bytes. Metadata, original and normalized strings/paths, carrier allocations and before/after/insertion headroom remain checked and charged to the independent existing 2 MiB non-image budget.
- Reserve before retaining authorization state. Completion must carry the same Rc allocation, uniquely owned, sealed with Weak identity. No equivalent replacement, COW substitution, changed tuple or Native/Synthetic swap. No original cloning or clipboard reread. Refusal/cancellation/queue discard releases payload and budget; no fallback or replay.
- Preserve secure/composition/focus/mount/register/serial, native-menu and mutation fences, account-before-completion, attachment-only explicit serial with mutation None. Preserve every old regression assertion; update the double-buffer test to assert the corrected single-buffer behavior rather than weakening safety checks.
- Authoritative attachment commit moves the complete original item to existing ImagePaste. Text-only originals remain alive through authorized commit and then release. No new persistence. Synthetic preserves its normalized attachment reconstruction.
- The legacy importer already copies/decodes image bytes. That pre-existing downstream lifetime and copy are outside this scope; the adapter budget covers authorization through authoritative delivery, not importer completion.

## Required development verification

- Actual Native carrier -> hook -> completion -> adapter -> authority/ImagePaste acceptance for two 8 MiB and four 4 MiB images, preserving metadata independently of the full image budget.
- Over-capacity native image Vecs tighten before reserve; effective bytes/id/format remain unchanged and the retained image charge equals actual capacity.
- Count 5, per-image over 8 MiB, aggregate over 16 MiB, invalid MIME and cumulative overflow remain rejected. Exact boundaries remain accepted. Cancellation, rejection, commit and queue discard release cumulative usage.
- Mixed and multiple strings, metadata None/empty/non-JSON, empty clipboard, HTML non-pollution and native attachment-only mutation/serial behavior remain covered.
- Equal-value replacement, changed normalized data, changed metadata and Native/Synthetic substitution fail closed. Weak sealing and unique ownership remain asserted.
- Existing mutation, native-menu, composition, focus and stale-registration regressions remain unchanged except signature adaptation.

## Delivery and validation record

Implemented `InputPastePayload::{Native(NativeClipboardPaste), Synthetic(PasteEvent)}` with private native original/text/uris, and `InputPasteFile::{Native(&Image), Synthetic(&PastedFile)}` exposing coupled name/mime/bytes/capacity. InputPaste borrows `&Rc<InputPastePayload>`; completion moves that Rc. No separate original Rc remains. Native HTML is always empty. Native construction consumes the once-read item, tightens only images with spare capacity using `mem::take(...).into_boxed_slice().into_vec()`, then derives text and URIs. The application reads actual capacity for checked reserve; normalized_storage counts the carrier header/Rc counters and derived allocations, while original_storage counts entries and original strings/metadata/paths. Native images are charged once from original_storage; Synthetic images once from complete PastedFile data. The original item moves into ImagePaste without reconstruction. Legacy no-hook engine PasteEvent construction remains complete and unchanged.

The capacity conversion consumes the unique Vec, discards excess capacity through Box<[u8]>, then returns a Vec with slice-length capacity. It preserves bytes/id/format but may reallocate transiently. The reservation begins only after conversion and validation; it lasts through pending authorization and the bounded NativeDelivery queue until authoritative delivery/discard. This is retained-allocation accounting, not a peak-RSS promise. Legacy importer copies remain outside this boundary.

Parent wiring required in the existing `nsh_platform_gpui::host_regressions` export list (lib.rs was not edited):

- `native_paste_through_hooks`: development helper accepting optional original item and an identity-to-InputHooks factory; returns an `impl FnOnce()` release handle. Keep that handle alive until authoritative commit, then invoke it so teardown cannot retire focus before delivery.
- `native_paste_full_image_budget_uses_original_borrowed_files`: add a host test wrapper. Checks two 8 MiB/four 4 MiB images, real original over-capacity conversion, borrowed image identity, unchanged bytes/id/format and metadata, same Rc completion, empty HTML and attachment-only serial/mutation.

Retain existing exports/wrappers for `native_paste_preserves_original_entries_and_opaque_metadata` and `native_paste_none_and_refusal_do_not_fabricate_original_or_edit`; the latter now also distinguishes Some(empty item) from None. Existing mutation/menu/composition/secure/focus tests remain present. Synthetic direct session tests explicitly construct Synthetic, not a purported native original.

New automatically discovered adapter tests:

- `native_full_image_budget_and_metadata_are_independent_and_release_inflight`
- `native_full_image_reservation_releases_on_cancel_and_queue_discard`
- `native_carrier_cow_image_replacement_is_not_authorized_by_equal_text`
- `carrier_reservations_keep_existing_sixty_four_limit_and_release`

The former `original_storage_counts_capacities_paths_and_both_image_buffers_once` is corrected to `original_storage_counts_capacities_paths_and_single_image_buffer_once`: it checks one 8 MiB native charge, two simultaneous 8 MiB charges accepted, third rejected, release/reuse, paths and oversized metadata capacity. Existing carrier substitution, metadata cumulative/refusal/release and queued discard tests are adapted to the unified seal, retaining refusal/no-extra-delivery assertions and adding COW Native/Synthetic replacement and nonunique-owner cases.

New automatically discovered application test: `native_full_budget_carrier_hooks_adapter_authority_deliver_original_images`. It uses the actual host field, registration, session hook/completion, real NativeAdapter and authority bridge to emit ImagePaste for two 8 MiB/four 4 MiB over-capacity original images with opaque metadata. It checks full original item equality (including image id/format), capacity, serial, mutation None, empty HTML and no extra delivery. It requires the helper export above. Existing native attachment/text and selected-mention mutation/undo tests remain present. A focused scan found the sole production apply_native_paste call within the assigned nsh.rs and adapted it; no production caller outside these five files was changed.

Executed scoped `rustfmt --edition 2024 --config skip_children=true` and final `--check` for exactly the five assigned Rust files, both exit 0. Tool: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Six-file final-newline/trailing-whitespace checks passed. Static inspection followed the once-read native path, original allocations, capacity conversion, before-retain budget, Weak/unique seal, queue, real mutation fences and authority/ImagePaste endpoint. Formatting and authored tests do not establish successful type checking or execution.

Parent must add the helper export/wrapper, compile and execute all existing/new regressions, perform independent review and final-head gates. No local build, Cargo check, Clippy, test execution, CI, Git, network or native launch ran. Cargo/rustc versions were not queried. Host regressions do not establish OS clipboard availability or real native focus/IME acceptance; HTML OS acquisition and AX action routing remain outside scope. Development Rust tests remain subject to pre-master cleanup policy. Source HEAD/fetch state is parent-supplied; exact dirty SHA256 identities are in the final delivery. This leaf stops writing after delivery.
