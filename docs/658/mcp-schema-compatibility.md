# MCP schema compatibility

## Contract

Issue #658. Baseline: 68704d46794d5fd221bdbc915354de58600a3a9d.
User approved adding jsonschema; pin 0.60.1 with default features disabled.

Replace the partial keyword whitelist with JSON Schema 2020-12 validation. The input root remains an object schema with `type: object`. Support explicit 2020-12 and default missing dialect to 2020-12; unsupported dialects fail explicitly. Preserve the original schema rather than deleting constraints for a model provider.

Use the library's offline resolver even with default retrieval features disabled. References resolved inside the document work; unresolved external references fail without network/file access. Retain HTTP header extraction and its reachability/type/uniqueness rules. New schema-bearing locations must not silently evade header validation.

Before compilation, bound serialized schema bytes (256 KiB), structural depth (32), total JSON nodes (8192), and total object nodes (512). These conservative structural limits also bound compositions and definitions; do not traverse schema references recursively in the preflight. Limit regex construction/backtracking through library options. Existing catalog count/byte limits remain.

Return bounded tool identifiers and schema paths with fixed failure categories, without echoing arbitrary schema values or configuration. The connection test must distinguish zero advertised tools from all definitions rejected; rejected details are available on demand with a visible rejection count. Do not modify enable state, approvals, credentials, or transport negotiation.

## Verification

Development-only Rust regression cases: default and explicit dialect, nested arrays/objects, nullable types, composition and local references, numeric boundaries, malformed schemas, unsupported dialects, unresolved HTTP/file references, resource limits, and header declarations outside extractable properties. Execute only via Actions; retain test commit and results before removing Rust tests for final delivery. Final required CI must run on the no-Rust-tests head. Independent review required before merge.

Actual Chrome DevTools MCP 1.12.0 tools/list and installed application version remain user integration verification, not established by static source inspection or unit cases.

## Review hardening

HTTP schemas containing references receive a conservative whole-document header inventory after normal extraction. If any `x-mcp-header` marker was not extracted, reject the definition, including markers hidden in annotation data that could be reference targets. This intentionally rejects the ambiguous combination of references and header-shaped annotation data rather than silently treating it as safe. Reference-free annotation data is unaffected; stdio is unaffected. No references are followed by this bounded structural inventory, so cycles and encoded/noncanonical reference targets cannot bypass it.
