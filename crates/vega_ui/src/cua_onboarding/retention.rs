#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareEntryKind {
    Permissions,
}

pub fn retained_entry(
    error: &vega_conversation::types::ComputerUseError,
    dismissed: bool,
) -> Option<PrepareEntryKind> {
    let _ = (error, dismissed);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use vega_conversation::types::ComputerUseError;

    #[test]
    fn permissions_error_retained() {
        let entry = retained_entry(&ComputerUseError::Permissions, false);

        assert_eq!(entry, Some(PrepareEntryKind::Permissions));
    }

    #[test]
    fn protocol_error_not_retained() {
        let entry = retained_entry(&ComputerUseError::Protocol, false);

        assert_eq!(entry, None);
    }

    #[test]
    fn dismissed_not_retained() {
        let entry = retained_entry(&ComputerUseError::Permissions, true);

        assert_eq!(entry, None);
    }

    #[test]
    fn transport_like_not_retained() {
        let entry = retained_entry(&ComputerUseError::Unavailable, false);

        assert_eq!(entry, None);
    }
}
