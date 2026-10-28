# Remove obsolete Full Access authorization reset entry

## Scope and acceptance

User requests continuing #695: remove the settings button labelled Reset full-access authorizations now that mode is the authority.

- Idle/completed settings show no reset entry, confirmation copy, or empty section spacing.
- Permission mode selection remains available and unchanged.
- Existing in-flight/startup recovery projections retain their progress, cancellation and error recovery controls. Do not hide blocking recovery errors.
- Completing recovery must not focus the removed reset entry.
- Preserve reset services, revision/epoch invalidation, admission fences and independent capability approvals. No configuration/database mutation or replacement global revoke feature.

## Implementation and validation

Return an empty element for idle/completed reset projections before constructing the section; focus the permission picker on completion. Do not expose a new Prepare action. Existing confirmation belongs only to an already-started recovery flow.

Local validation: format and diff checks only. No local builds/tests/app execution. Independent review and final-head required CI precede merge; keep #695 open/In review for user acceptance. No development Rust tests are necessary for this narrowly scoped projection change. CI Rust empty-set results are not functional coverage.
