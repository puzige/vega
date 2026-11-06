# #894 Skill frontmatter compatibility

Baseline: `16df62bdb939543fe144c32c0bfdea5fa84807a4`. Scope approved by the user: accept `allowed-tools` strings and string lists, report the actual invalid field, preserve permission behavior, and never edit user Skill files/configuration/data.

## Acceptance before implementation

| Input / action | Observable result | Evidence plan |
| --- | --- | --- |
| Existing string, including whitespace / empty string | Exact original string retained | Actions parser regression |
| String list | Join entries with one space in original order, preserving each entry's bytes | Actions parser regression |
| Empty list / empty or whitespace-only list entries | Empty list becomes `Some("")`; empty entries are retained by joining, consistent with existing empty strings; no permission interpretation | Actions parser regression |
| Missing or null `allowed-tools` | `None`, preserving optional-field behavior | Actions parser regression |
| Map, nested list, boolean, number (including signed NaN and infinity in every case-insensitive spelling accepted by the locked parser), null list item | Reject with `invalid_allowed_tools`; no scalar coercion; quoted numeric-looking strings remain strings | Actions parser regression |
| Explicit core-tagged non-string values and aliases to them | Actual recognized field diagnostic, including aliases whose definition is in an unknown extension field; top-level alias keys resolve the same recognized field as scalar keys | Actions parser regression |
| Missing / non-string / invalid `name` | Name-field diagnostic; valid names still must match directory | Actions parser regression |
| Missing / invalid / empty / overlong `description` | Description-field diagnostic | Actions parser regression |
| Invalid `license` / `compatibility` types or invalid compatibility length | Respective field diagnostic | Actions parser regression |
| Absent metadata / string map / nested or non-string metadata | Default empty map / accept / reject with metadata diagnostic | Actions parser regression |
| Malformed YAML, duplicate keys, merge, forbidden tags, budgets | Existing rejection remains, accurate generic category when no field is safely known | Actions security regression |
| Discovery and settings diagnostic projection | Stable safe code maps to concise EN/ZH messages naming the real field; no raw YAML/value/path in errors | Static chain review + Actions discovery regression |
| Permission consumers / user files | No changes; `allowed_tools` remains inert declaration, not a grant or tool filter | Full diff and consumer inspection |

## Implementation plan

1. Keep bounded event preflight and deserializer budgets/options. Track recognized-field context and scalar type hazards in the preflight, including anchors/aliases, before typeless decoding can erase non-finite/explicit-tag provenance. Decode recognized raw values without implicit conversion, validate them with field-specific safe errors, normalize only `allowed-tools` lists into existing `Option<String>`. Ignore unknown top-level fields as before. Use strict string-key decoding to reject non-string metadata keys.
2. Add static error variants/codes for name type/missing, description, license, compatibility and allowed-tools. Keep existing invalid-name checks and metadata code; generic deserializer failures become invalid-format rather than misleading metadata errors. Do not expose parser exception text.
3. Map codes through existing settings projection and localized diagnostics. No new UI component or permission logic; nested metadata remains unsupported.
4. Add focused temporary development regressions executed only in existing Actions. Preserve reachable test source and actual results, remove tests and exclusive wiring before final required CI. Nonimplementer review binds the stopped exact version; final-head verification and review of any subsequent delta precede squash merge.

## Boundaries and verification

No dependency, manifest, workflow, migration, consent, linked-source safety, frozen snapshot or user Skill changes. Local work is static/diff/format only; no Vega execution, compile, test or package. Required CI and release run must match exact SHAs. Merge/release are not user acceptance; #894 stays open / In review until manual acceptance.

Sources: [Agent Skills](https://agentskills.io/specification) defines an experimental space-separated string; [Claude Code](https://code.claude.com/docs/en/skills) also supports a YAML list. This is a compatibility extension, not adoption of Claude's permission grant behavior.
