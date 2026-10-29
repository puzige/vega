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
