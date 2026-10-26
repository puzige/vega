# Issue #69 — Environment toggle selection

Status: implementation contract. Source: [Issue #69](https://github.com/puzige/vega/issues/69).

The wide Environment rail opens by default. Closing it through the header toggle
currently leaves a selected-looking fill after the pointer leaves, unlike its X.
R45 §3.1–3.2 requires `bg_active` to describe a rendered surface, not focus.
Keep pointer hover as `bg_hover`; use the existing `accent` keyboard focus ring
(design guidelines §8) independently of selection. The rail/overlay handlers,
breakpoint, geometry, keyboard activation and persistence remain unchanged.
An Environment slot disabled because a right dock replaces it must not claim
that the Environment is rendered.

## Acceptance matrix (frozen before implementation)

| ID | Precondition / operation | Observable expectation | Layer / evidence | Status |
|---|---|---|---|---|
| A1 | Wide default rail; click header toggle; move pointer away | Rail absent; selected fill absent even while toggle retains focus | Production render/painted quads + native screenshot | Production PASS; native pending |
| A2 | Click same header toggle again | Rail present; selected fill present | Production render/painted quads + native screenshot | Production PASS; native pending |
| A3 | Close open rail with X | Rail and selected fill absent | Production render/painted quads + native screenshot | Production PASS; native pending |
| A4 | Narrow window; header opens then closes overlay | Overlay and selected fill track each other | Production render/painted quads + native screenshot | Production PASS; native pending |
| A5 | Keyboard focus / Enter on Environment toggle | Accent focus border remains discernible with no false selected fill; activation works | Production render/painted quads | PASS |
| A6 | Right dock replaces rail; click disabled Environment | No selected fill; no activation; three shell slots retain geometry | Production render/painted quads + existing R45 tests | Production PASS; native pending |
| A7 | Other shell controls | Existing bottom/right action, hover, selected and keyboard contracts preserved by shared control | Existing R45 production tests | PASS (8 R45 tests) |
| N/A | Persistence, service errors, provider/network | No persistence or external service change | Not applicable | N/A |

## Plan

1. Add a production-root regression that observes actual painted slot quads after
   click/mouse movement, preserving its failure before implementation.
2. Separate shared shell focus border from selected fill; gate Environment
   selection with its real project/right-dock render preconditions.
3. Run focused production regressions and formatting with persistent raw logs;
   main agent owns native E2E, review and integration. No new dependencies/API.

Rollback: revert this task's component/render changes and regression together.
