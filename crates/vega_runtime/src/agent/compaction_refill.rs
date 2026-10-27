#[derive(Default)]
pub(super) struct CompactionRefill {
    rounds_since_install: Option<u8>,
    rapid_refills: u8,
}

impl CompactionRefill {
    pub(super) fn primary_started(&mut self) {
        if let Some(rounds) = &mut self.rounds_since_install {
            *rounds = rounds.saturating_add(1);
        }
    }

    pub(super) fn admit(&mut self) -> bool {
        match self.rounds_since_install {
            Some(rounds) if rounds < 3 => {
                self.rapid_refills = self.rapid_refills.saturating_add(1);
            }
            _ => self.rapid_refills = 0,
        }
        self.rapid_refills < 3
    }

    pub(super) fn installed(&mut self) {
        self.rounds_since_install = Some(0);
    }
}

#[cfg(test)]
mod tests {
    use super::CompactionRefill;

    #[test]
    fn third_rapid_refill_stops_before_another_summary() {
        let mut state = CompactionRefill::default();
        assert!(state.admit());
        state.installed();
        for _ in 0..2 {
            state.primary_started();
            assert!(state.admit());
            state.installed();
        }
        state.primary_started();
        assert!(!state.admit());
    }

    #[test]
    fn three_primary_rounds_break_the_rapid_refill_sequence() {
        let mut state = CompactionRefill::default();
        state.installed();
        assert!(state.admit());
        state.installed();
        assert!(state.admit());
        state.installed();
        for _ in 0..3 {
            state.primary_started();
        }
        assert!(state.admit());
        state.installed();
        assert!(state.admit());
    }

    #[test]
    fn failed_attempt_does_not_reset_primary_round_distance() {
        let mut state = CompactionRefill::default();
        state.installed();
        state.primary_started();
        assert!(state.admit());
        state.primary_started();
        state.primary_started();
        assert!(state.admit());
        assert_eq!(state.rapid_refills, 0);
    }
}
