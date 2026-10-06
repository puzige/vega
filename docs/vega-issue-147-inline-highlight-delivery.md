# Issue #147 内联代码选区高亮交付记录

## Freeze

- verified_at_utc: 2026-10-06T00:38:59Z
- verified_at_local: 2026-10-06 08:38:59 CST
- git_head: `feat/147-inline-highlight-fix` (branch reference; raw commit OID omitted)
- baseline_source_sha256:
  - `render_rows.rs`: `df435466c2de8eb068c44c1d2619de6f721ebc08b070d68af5bdc378218e5ae7`
  - `selection.rs`: `21e2c196a50b4a87b6ba28940ac0ddfb99802a9441c5c875c715bccf106d4856`
- candidate_production_diff_sha256: `ef99f822ffb9a329db4a8d9afcf7bfbb4a13f87417722008a85d12f655c72c92`
- contract: Issue #147, `docs/vega-issue-147-markdown-selection.md` SHA-256 `d3b54b8d4acbe4b361b12b183a941b13d17eac35a81267a27686cff8a643fbbe`
- frozen_test_source_sha256: `crates/vega_ui/src/conversation_stream/tests/issue147_markdown_selection.rs` SHA-256 `af225d01e9d5c344f2a3e97ec61d372f92002e4de66067d80543acdfa658d1f6`; unchanged during implementation
- os_arch: Darwin arm64
- rustc: `rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`
- cargo: `cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`
- git: `git version 2.55.0`

## Results

| requirement | evidence class | exact command | result | duration | bounded footer/hash |
|---|---|---|---|---|---|
| M147-6d complete inline code, Light and Dark | GPUI scene test | `cargo nextest run -p vega_ui issue147_inline_highlight` | Initial business RED: selected text and Cmd+C matched `inline_code`, but final selected background was the later code background in both themes. After the paint-order fix, both cases PASS at all three sample points. | RED 0.045s; first GREEN 0.070s | RED log SHA-256 `f52eb871d451190d55e82c4ed4884ed09eeaf624fceb11c420ba78d1e6050efe`; GREEN log SHA-256 `a94a002d764f070753e36e99bbdffc610a877e42753a9c266899c36e4b063047` |
| M147-6e partial inline code, Light and Dark | GPUI scene test | `cargo nextest run -p vega_ui issue147_inline_highlight` | Initial business RED at the selected-background samples. After the fix, `line_c` selection and clipboard match exactly; selected range shows selection color at all three samples, while unselected `in` and `ode` each retain code background at all three samples. | RED 0.045s; first GREEN 0.070s | Same RED and first-GREEN logs above |
| M147-1b ordinary CJK/English/emoji paragraph control, Light and Dark | GPUI scene test | `cargo nextest run -p vega_ui issue147_inline_highlight` | Both controls PASS before and after the fix: exact selected text and clipboard, with selection color at all three samples. | RED 0.045s; first GREEN 0.070s | Same RED and first-GREEN logs above |
| Original candidate compilation | focused Nextest build | `cargo nextest run -p vega_ui issue147_inline_highlight` | Compile failed before tests: E0432 for `gpui_kit::ResultExt`, and E0599 for `.log_err()` at shape and paint sites. This was a compile failure, not a test result. | 0.20s | Log SHA-256 `1d7721362db1e5f330f3e71b6e8fd96bd7f5ef800bd7896ee4c0517e5fe2093d` |
| Repaired candidate build and focused regression | focused Nextest | `cargo nextest run -p vega_ui issue147_inline_highlight` | PASS; 6 passed, 553 skipped. The test build compiled `vega_ui`; this is the first GREEN after the compile-only failure. | build 7.97s; tests 0.070s | Nextest run `90ed0ade-fa5a-4e21-8f66-1fd15f0ce40b`; log SHA-256 `a94a002d764f070753e36e99bbdffc610a877e42753a9c266899c36e4b063047` |
| Formatting | formatter check | `cargo fmt --all -- --check` | exit 0 | 2.2s | no output |
| Whitespace | diff check | `git diff --check` | exit 0 | <0.1s | no output |

The baseline test run selected all six frozen cases. Its first run produced 2 PASS and 4 FAIL; a saved immediate confirmation run produced the same result and retained the bounded raw log. The four code cases failed at the final background-layer assertion after their selected-text and clipboard assertions had passed. Both ordinary-paragraph controls passed. No frozen test, assertion, or fixture was changed.

The code background was painted after the projected selection, so it covered the selection color while the text-selection and clipboard state remained correct. The candidate paints styled run backgrounds first, then projects the selection, then paints glyphs. This matches the native Light/Dark symptom and explains why earlier selection/copy GREEN checks did not detect it: those checks asserted selected text and clipboard, not the final background layer. The owned GPUI scene now passes the frozen paint-order and color-sampling assertions; this does not substitute for the separately scoped native macOS visual recheck.

## Residuals

- ACCEPTED: no new dependency, public API, schema, configuration, or code comment was added.
- NOT RUN: native desktop acceptance; this implementation task was limited to the owned GPUI scene and did not launch or install Vega.
- PARTIAL: the original official v0.1.56 native Light/Dark complete-code and Light partial-code highlight observations remain FAIL evidence ([Issue #147 comment](https://github.com/puzige/vega/issues/147#issuecomment-6005988118)). They were not rechecked after this change; the result here is limited to the frozen visual regression.
- LIMIT: if `shape_text` fails, the error is logged and the standard styled-text painter is used. The focused scene covers the normal successful shaping path, not an injected shaping failure.
- PARTIAL: this is only the inline-highlight follow-up; Issue #147 remains OPEN/In progress. Other Issue #147 native interaction and rendering cases are outside this change.
