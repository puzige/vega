#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareEntryKind {
    Permissions,
}

pub fn retained_entry(
    error: &vega_conversation::types::ComputerUseError,
    dismissed: bool,
) -> Option<PrepareEntryKind> {
    if dismissed {
        return None;
    }
    error
        .needs_permissions()
        .then_some(PrepareEntryKind::Permissions)
}
