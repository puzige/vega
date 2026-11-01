# MCP five-dialect tool definitions

## Scope and authority

Issue #834; baseline `1f93cbac` on origin/master. This supersedes only the dialect support scope of [#658](../658/mcp-schema-compatibility.md), not its historical evidence. MCP [2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic#json-schema-usage) and [2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic#json-schema-usage) require 2020-12, default to it when undeclared, permit explicit other dialects, and require explicit unsupported errors. No protocol upgrade is part of this work.

## Dialect contract

| Dialect | Versioned URI path on json-schema.org | Validator |
| --- | --- | --- |
| Draft-04 | `/draft-04/schema` | Draft4 |
| Draft-06 | `/draft-06/schema` | Draft6 |
| Draft-07 | `/draft-07/schema` | Draft7 |
| 2019-09 | `/draft/2019-09/schema` | Draft201909 |
| 2020-12 | `/draft/2020-12/schema` | Draft202012 |

Accept exactly HTTP or HTTPS for these hosts/paths, with no fragment or one empty trailing `#`. Missing root declaration defaults to 2020-12; child schemas inherit that root dialect. Reject non-string, unknown, custom, unversioned and malformed declarations with bounded fixed categories, never echo their values. No keyword inference or schema rewriting. Keep `type: object` input roots and preserve the original JSON passed to the model provider.

Support same-dialect embedded resources and local references, including old `id` versus `$id` and 2019-09 recursive references as implemented by pinned jsonschema 0.60.1. Cross-dialect embedded declarations are explicitly unsupported in this delivery, including declarations without a resource ID. Explicit vocabulary customization is unsupported. A declaration is checked at schema-bearing locations, not ordinary annotation data. When any reference occurs, conservatively inventory the entire JSON document for dialect/vocabulary declarations as well as header markers: arbitrary JSON Pointer targets can be schema resources. In that case incompatible declarations even in annotation data are rejected rather than interpreted with the wrong dialect. This inventory never follows a reference.

## Traversal and HTTP safety

Use one dialect-aware schema child inventory for dialect checks and header extraction. Include properties, patternProperties, definitions, composition, items, additionalProperties and not; old tuple items/additionalItems and object/boolean schema dependencies (not property dependency arrays); Draft-06+ contains/propertyNames; Draft-07+ conditionals; 2019-09+ $defs/dependentSchemas/unevaluated/contentSchema; 2020-12 prefixItems and single-schema items. Retain legacy definitions where supported by the dependency. Pinned jsonschema also compiles legacy `dependencies` in 2020-12; inventory its schema values in every dialect, including default 2020-12, rather than allowing this compatibility extension to evade dialect/vocabulary/header preflight. Property-dependency arrays remain data, not schema nodes.

Only direct property chains can yield mirrored headers; composition, arrays, definitions and dependency schemas cannot. For Draft-04/06/07 a schema object containing `$ref` cannot yield headers from its ignored sibling constraints. A marker in such an object is rejected, not extracted. Case-insensitive uniqueness, scalar type and valid name requirements remain. Reference inventory includes `$ref`, `$dynamicRef` and `$recursiveRef`; any marker not accounted for by extraction rejects an HTTP definition. Conservative inventory may reject reference-bearing annotation data; reference-free annotations remain data. Stdio does not perform HTTP header extraction.

Keep offline resolver, disabled default dependency features, regex budgets, 256 KiB schema/catalog schema-byte limits, depth 32, 8192 JSON nodes, 512 objects and existing catalog limits. No network/file retrieval, no permissive fallback for unresolved references. Errors stay bounded and redacted; partial/all rejection behavior stays intact. No changes to enable state, approvals, credentials, OAuth, transport or connection-test lifecycle.

## Verification and non-goals

Development-only regressions exercise actual catalog admission and original JSON preservation for all five URI families/default, and effective validator semantics using valid/invalid instances: exclusive boundaries, old IDs and local refs, tuple/additionalItems, dependencies, recursive refs, prefixItems, and ref siblings. They also cover nested declarations, mixed/unknown/custom dialect rejection, annotation handling, header safety (including old positions and recursive inventory), external refs, malformed definitions, depth/size/object/node/regex limits and partial/all rejection diagnostics. No real network/process E2E gate.

The discovery validator is not retained for per-call parameter validation. No new per-call schema validation, outputSchema feature or model-provider conversion/strict adaptation is included. Provider API acceptance of all keywords is not promised. Actual Exa refresh and model/tool integration are user hand checks on the delivered version.

Local work is static/format checks only. Tests run only in Actions on a draft PR, with recoverable full source commit and run evidence retained before removing Rust tests and exclusive support under repository policy. Independent non-writer review and full required CI on the final no-Rust-tests head precede merge. CI/merge is not user acceptance; keep Issue open and In review until the user verifies it.

## Implementation plan

1. In `crates/vega_mcp/src/wire.rs`, centralize dialect selection, build options and schema child classification; adapt schema preflight and HTTP extraction without changing transport/call dispatch.
2. Add focused development tests in the same private module to exercise catalog admission and effective semantics without expanding production API or adding dependencies.
3. Update #658 with a supersession link; record static checks, cloud results, exact test source and independent review in this directory/Issue/PR. Remove development tests only after successful evidence is retained; validate final head separately.
