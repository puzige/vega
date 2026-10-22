use super::CallbackIdentity;
use super::bundle::bundle_path;
use std::path::{Path, PathBuf};
use vega_conversation::types::{
    ComponentState, ComputerUseError, DirectCapture, PermissionState, Preparation,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardAction {
    Recheck,
    AccessibilitySettings,
    ScreenRecordingSettings,
    RevealInFinder,
    ReturnToTask,
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardEffect {
    Inspect {
        ticket: CheckTicket,
        prompt: bool,
        probe_direct_capture: bool,
    },
    OpenUrl(String),
    RevealInFinder(PathBuf),
    CancelPending,
    CloseWindow,
    ReturnToTask(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    Idle,
    Checking,
    Observed,
    Busy,
    Unknown,
    Failed(ComputerUseError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckTicket {
    task: String,
    lifetime: CallbackIdentity,
    check: CallbackIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardView {
    pub component: Option<ComponentState>,
    pub accessibility: PermissionState,
    pub screen_recording: PermissionState,
    pub direct_capture: DirectCapture,
    pub check_state: CheckState,
    pub open: bool,
    pub following: bool,
    pub can_drag: bool,
    pub can_recheck: bool,
    pub can_return_to_task: bool,
    pub desktop_ready: bool,
}

pub struct CardController {
    task: String,
    lifetime: CallbackIdentity,
    executable: PathBuf,
    bundle: Option<PathBuf>,
    pending: Option<CheckTicket>,
    preparation: Option<Preparation>,
    check_state: CheckState,
    open: bool,
    following: bool,
}

impl CardController {
    pub fn new(task: &str, executable: &Path) -> Self {
        Self {
            task: task.to_owned(),
            lifetime: CallbackIdentity::default(),
            executable: executable.to_owned(),
            bundle: bundle_path(executable),
            pending: None,
            preparation: None,
            check_state: CheckState::Idle,
            open: true,
            following: true,
        }
    }

    pub fn view(&self) -> CardView {
        CardView {
            component: self.preparation.as_ref().map(|value| value.component),
            accessibility: self
                .preparation
                .as_ref()
                .map_or(PermissionState::Unknown, |value| value.accessibility),
            screen_recording: self
                .preparation
                .as_ref()
                .map_or(PermissionState::Unknown, |value| value.screen_recording),
            direct_capture: DirectCapture::NotChecked,
            check_state: self.check_state,
            open: self.open,
            following: self.following,
            can_drag: self.open && self.bundle.is_some(),
            can_recheck: self.open,
            can_return_to_task: self.open,
            desktop_ready: false,
        }
    }

    pub fn act(&mut self, action: CardAction) -> Vec<CardEffect> {
        if !self.open {
            return Vec::new();
        }
        match action {
            CardAction::Recheck => {
                let ticket = CheckTicket {
                    task: self.task.clone(),
                    lifetime: self.lifetime.clone(),
                    check: CallbackIdentity::default(),
                };
                self.pending = Some(ticket.clone());
                self.check_state = CheckState::Checking;
                vec![CardEffect::Inspect {
                    ticket,
                    prompt: false,
                    probe_direct_capture: false,
                }]
            }
            CardAction::AccessibilitySettings => vec![CardEffect::OpenUrl(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                    .into(),
            )],
            CardAction::ScreenRecordingSettings => vec![CardEffect::OpenUrl(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
                    .into(),
            )],
            CardAction::RevealInFinder => vec![CardEffect::RevealInFinder(
                self.bundle.as_ref().unwrap_or(&self.executable).clone(),
            )],
            CardAction::ReturnToTask => {
                let mut effects = self.close();
                effects.push(CardEffect::ReturnToTask(self.task.clone()));
                effects
            }
            CardAction::Close => self.close(),
        }
    }

    pub fn complete(
        &mut self,
        ticket: CheckTicket,
        result: Result<Preparation, ComputerUseError>,
    ) -> Vec<CardEffect> {
        if !self.open || self.pending.as_ref() != Some(&ticket) {
            return Vec::new();
        }
        self.pending = None;
        match result {
            Ok(preparation) => {
                self.check_state = if preparation.accessibility == PermissionState::Unknown
                    || preparation.screen_recording == PermissionState::Unknown
                {
                    CheckState::Unknown
                } else {
                    CheckState::Observed
                };
                self.preparation = Some(preparation);
            }
            Err(error) => {
                self.preparation = None;
                self.check_state = if error == ComputerUseError::Busy {
                    CheckState::Busy
                } else {
                    CheckState::Failed(error)
                };
            }
        }
        Vec::new()
    }

    pub fn begin_drag(&mut self) -> Option<PathBuf> {
        if !self.open {
            return None;
        }
        let bundle = self.bundle.clone()?;
        self.following = false;
        Some(bundle)
    }

    pub fn drag_ended(&mut self) {}

    fn close(&mut self) -> Vec<CardEffect> {
        self.open = false;
        self.following = false;
        self.pending = None;
        vec![CardEffect::CancelPending, CardEffect::CloseWindow]
    }
}
