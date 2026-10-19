# Issue 63 · A2-13 explicit image attachments

Freeze: 2026-09-19. Source: GitHub puzige/vega#63 (Ready/P0) and
vega-features.md A2-13. Main agent architecture decision; no unrelated cards.

## Product contract

R1. Composer accepts explicit image paste, external image-file drop, and an
"添加图片" entry in its existing + menu using the native file picker. PNG,
JPEG and WebP only. No implicit clipboard reads, URL fetching, workspace
scanning, OCR substitute, video, PDF, generated images or new configuration.
Text paste and @file references retain their existing semantics.

R2. Imported images appear as removable thumbnails before sending and in the
user turn afterward. Image-only turns are valid. Pending import disables
submission; import/validation failure retains the existing draft and displays
a content-free actionable error. A mixed batch is accepted atomically or not
at all. No disk IO or image decoding on the UI render/event thread.

R3. Each turn permits <=4 images, <=8 MiB encoded bytes per image, <=16 MiB
encoded total, dimensions <=8192 per axis and <=16,000,000 pixels. Validate
actual format, decode validity and decoded output allocation (<=64 MiB), not
extension alone. Set the image crate's allocation limit to 64 MiB as an additional
defense, but do not claim it caps total process memory: JPEG/WebP decoders do not
strictly enforce that limit for all internal workspace. Encoded size, dimensions,
pixel count and decoded output checks remain mandatory before full decoding.
Empty, malformed, animated, oversized and unsupported files
fail closed; do not silently rescale or discard. Limits apply to clipboard,
picker, drop and programmatic entry points. Reject non-regular files with
nonblocking/no-follow file open and bounded read (limit+1); do not recurse.

R4. Draft images and in-flight imports belong to the originating draft/entity
generation. Switching route, clearing/replacing draft or removing an image
cannot attach a stale completion to another task. Freeze text and image vector
once per submission. Only the durable MessageStarted acknowledgment consumes
that frozen draft; rejected preflight preserves it, and late acknowledgment
must not clear newer edits. Provider failure after durable start keeps the
submitted turn visible. No images leak between threads.

R5. Persist validated encoded bytes with the owning user message in the same
SQLite transaction before MessageStarted. Add migration 0007 with an ordered
attachment table, message foreign key and cascade lifecycle; existing messages
and migrations remain valid. No copying into user projects. Rehydrating after
route switch/restart shows the same images. Batch page reads, not N+1 queries.
Bound loaded attachment bytes per history page/request to 32 MiB; exceeding
that limit fails visibly rather than truncating/removing attachments silently.
Retain existing user text separately and preserve chronological ordering.
Existing exact schema-version/table-inventory assertions must be updated for
migration 0007 and its image_attachments table; retain exact equality and all
other expected tables rather than weakening the regression checks.

R6. Headless runtime owns a validated immutable image value; expose that same
value through vega_conversation::types (the existing FrozenReasoning re-export
pattern) so UI never imports runtime or store directly for image operations.
Conversation facade owns explicit file import and store adapters. Custom Debug
must report metadata/counts only, never image bytes/base64/local paths.

R7. OpenAI-compatible Chat Completions serializes an image-bearing user message
as ordered content parts: optional text part followed by image_url parts whose
url is a MIME-correct base64 data URL. Text-only wire remains a string. Preserve
images on subsequent agent/tool rounds and later history reconstruction; never
inject images into tool/system/assistant messages. No provider capability is
inferred from model name, and no automatic provider/model switching. An upstream
vision rejection follows existing sanitized error handling, preserving the
durable turn. Native live acceptance requires an actually vision-capable model;
mock transport alone is not proof of live model support.

Protocol reference: https://developers.openai.com/zh-Hans/api/docs/guides/images-vision
(Chat Completions image_url data URL). Existing provider credentials/permission
gates stay unchanged; explicit image selection authorizes only that attachment
read and its submission to the user's selected provider, not arbitrary files.

## Frozen backend/UI seam

- `vega_conversation::types::ImageAttachment`: Clone + Eq, private immutable
  validated representation, `from_bytes(Vec<u8>) -> Result<Self, _>`,
  `bytes() -> &[u8]`, `mime_type() -> &str`, `width()/height() -> u32`.
- `vega_conversation::attachments::import_images(&[PathBuf])` performs bounded
  synchronous file import/validation for background execution; same atomic
  count/total validation for clipboard vectors via `validate_images(&[ImageAttachment])`.
- New agent entry `run_thread_task_with_images_and_reasoning` is the existing
  `run_thread_task_with_pricing_and_reasoning` signature with `images: Vec<ImageAttachment>`
  appended. Existing API delegates with empty images, preserving old callers.
- `HistoryEntry::UserImages { seq, images: Vec<ImageAttachment> }` immediately
  follows owning UserText; omit empty UserText for image-only turns if needed.
  No change to existing UserText constructor shape.
- UI/app owner freezes images on ComposerSubmitted and PendingAgentRun, consumes
  them only on matching acknowledgment, renders UserImages live and on hydration.
  Backend owner implements all facade/store/runtime pieces above.

## Dependencies and ownership

Architect approves direct use of already locked `image = =0.25.10` with minimal
png/jpeg/webp features and `base64 = =0.22.1` for bounded validation and wire
encoding. Backend owner alone edits root Cargo.toml/Cargo.lock and backend
crate manifests. GPUI's existing image representation is used for UI rendering.
If actual codec animation detection/API makes R3 impossible, report the specific
constraint before changing the contract. No new crates beyond these approvals.

Main: spec, review, integration, full gates, native acceptance. Backend executor:
runtime/conversation/store plus manifests and backend delivery evidence. UI/app
executor: vega_ui, vega application, vega_theme and UI delivery evidence. Same
feature worktree, disjoint files; all cargo invocations take cargo-lock.sh.

## Acceptance

1. Actual production composer handlers: paste, drop, picker, remove, image-only,
   mixed text+image, invalid batch, pending import, draft/route races and failed
   preflight preserve expected state. Test actual wiring, not just helper APIs.
2. Production conversation/store/provider request chain in owned temporary DB:
   valid image arrives byte-exact in a recorded HTTP body, text-only unchanged;
   tool rounds and reopen/next turn retain it; wrong-thread access excluded;
   transaction rollback leaves neither orphan attachments nor half user turns.
3. Codec/count/size/dimension/redaction and unsupported image tests; bounded
   history behavior and additive migration compatibility.
4. `cargo fmt --all -- --check`, locked strict workspace/all-target clippy,
   full workspace tests, build/package. Preserve first failures and truthful
   classification; serial full suite allowed for known shared-state tests.
5. Native installed app: explicit harmless owned image attached from UI,
   thumbnail/removal visible, send to an available vision-capable provider,
   reply grounded in that image, restart/route recovery. No manual config edits.
   If no such configured provider exists, leave live acceptance outstanding and
   report it; do not label the card Done solely on simulated service evidence.

No pushes, changes to user credentials, force/reset, installation while a user
run is active, unrelated cleanup, or deleting user-owned drafts/files.

## Architecture clarification

2026-09-19: image 0.25.10 documents allocation limits as non-strict across codecs.
R3 therefore specifies a hard decoded-output bound, not a false total-process
64 MiB guarantee. No new subprocess decoder/sandbox architecture is introduced
by this card; codec internal allocation remains a documented residual alongside
the encoded/dimension/pixel bounds. Executors must retain these checks and test
dimension/header rejection before any full decode.
