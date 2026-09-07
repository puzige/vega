# R7 Provider / models UI SDD v0.2

## Freeze

- Scope: Settings Provider form, its existing atomic config save, the existing
  app model-catalog reload path, and focused Settings tests.
- Excluded: thinking semantics, Diff refresh cadence, timeline/DDL, pricing
  storage/controller redesign, dependencies, timeouts, performance criteria,
  real credentials, user config, and provider/network calls.
- The existing pricing gate remains authoritative. The Settings page keeps its
  existing `添加自定义` editor and rate fields; R7 only preserves the route so
  a user can open it, enter all required rates, submit once, and see the
  controller projection.

## Native acceptance correction

The first native pass at 960×628 exposed a Settings-only layout defect: the
Models input had no independent frame, and after inserting a second line the
lower line was clipped into the helper copy below it. This correction is
limited to the Settings Models field. Its wrapper has a visible background and
border, reserves at least two complete body-text rows, and lets the viewport
grow with content through four rows. The model text remains bounded by the
existing input cap and is never discarded: when more than four visual rows are
present, the four-row viewport follows the caret so the tail stays visible.
The shared `TextInput` measurement remains line-height based; the Composer and
commit-message callers keep their existing row behavior and bare framing.

The native acceptance seam is the same real Settings window path used by the
Provider focus test. With one, two, and four model lines, the Models frame must
remain independently visible, its lower text row must remain inside the frame,
and the following helper copy must begin below the frame. With five lines, the
frame remains at the four-row viewport while the caret-facing tail remains
visible and the full draft remains in the input entity. This is a layout
contract only; model parsing, keyboard actions, config/key seams, event
ordering, and all security boundaries remain unchanged.

## Accepted input contract

The Provider form has a two-row minimum, four-row viewport multiline models
field. Each non-empty line is one model ID. Leading and trailing Unicode
whitespace is removed, empty lines are ignored, and the remaining ID is retained byte-for-byte,
including case and `/`, `-`, and `.`. At least one valid ID is required, so a
blank list cannot silently save a provider with no selectable model. The parser uses the already frozen
pricing model-ID grammar: the first byte is ASCII alphanumeric; later bytes
are ASCII alphanumeric, `.`, `_`, `:`, `/`, or `-`; `..`, `//`, and a trailing
`/` are rejected. An ID is 1..=200 UTF-8 bytes, the list contains at most 1,000
IDs, and the submitted text is at most 256 KiB. Duplicate IDs are rejected by
exact, case-sensitive comparison. Validation errors identify the line and
kind (duplicate, invalid, too long, too many, or input too large) without
showing credentials or internal capability terms.

The parser runs before Keychain or config I/O. A rejected submission leaves
name, URL, key draft, models draft, and the focused input unchanged. A Keychain
or config failure follows the same rule. The success path clears all four
drafts only after the atomic config save succeeds; clearing does not move focus
away from the field that received the submit action.

The candidate `AppConfig` is cloned before mutation. A config write failure
leaves both the draft and the prior in-memory config authority unchanged; a
successful Keychain write may remain as an orphan if the later config write
fails because Keychain and config have no cross-store transaction. The error
copy states that the save did not complete and never claims both stores rolled
back.

## Save and update contract

New providers require a non-empty name, base URL, and key. Their normalized
models list is persisted exactly as entered after validation. An existing
provider can be opened from its row; the editor loads its name, URL, and
newline-separated models, while the key input remains empty. Submitting the
same name replaces the URL and model list with the validated draft and keeps
the existing `key_ref` when the key field is empty. A supplied key updates the
Keychain and uses the provider name as the reference, preserving the existing
masked-key behavior. Renaming an edited provider is treated as a new provider
and therefore still requires a key. No key is ever loaded into or rendered by
the form.

After each successful Settings config save, the view emits one typed
`SettingsSaved` event. The app handler invalidates and starts the existing
worker-side model-catalog read, which applies only uniquely configured models
that are present in the Ready pricing authority. The event never calls the R1
thread-model acknowledgement or changes the current stream's displayed model.
The persisted `defaults.model` remains the default for a later new task; the
currently open task keeps its durable model selection.

## Acceptance matrix

| Requirement | Observable contract |
|---|---|
| Preserve IDs | mixed case and IDs such as `OpenAI/GPT-4.1-mini`, `claude-3.5-sonnet`, and `vendor/model-v1.2` round-trip exactly |
| Normalize lines | surrounding whitespace and blank lines disappear; internal punctuation/case remains |
| Reject safely | duplicate, invalid grammar, 201-byte ID, 1,001st ID, and >256 KiB input keep every draft and perform zero I/O |
| New provider | owned config reload contains the submitted model list and the expected key reference; no key value is serialized |
| Same-name update | edit loads the old list; changed list replaces it; empty key preserves the old reference; supplied key updates it |
| Focus/draft | validation, Keychain, and save errors retain input and focus; success clears the form only after save; models, edit rows, and Save are reachable by Tab, with Enter/Space on row/action controls and Cmd+Enter on the form |
| App projection | a successful save reloads the current task catalog and default-model source while the current thread model and R1 acknowledgement state stay unchanged |
| Pricing route | existing AddCustom editor accepts model plus four rates through the UI/controller projection; R7 supplies no rates and changes no pricing gate |

Tests use an owned temporary config/store, a fake Keychain backend where an
I/O seam is required, and `MockProvider` only at the provider boundary. They
must never read the user's profile, Keychain, or remote API. Cargo execution
is assigned to the test owner; this implementation turn performs static
checks and ordinary formatting only.
