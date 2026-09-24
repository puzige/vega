#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdatePhase {
    #[default]
    Loading,
    Idle,
    Checking,
    Current,
    Available,
    Downloading {
        received: u64,
        total: u64,
    },
    Ready,
    Installing,
    Error,
}

#[derive(Clone, Debug)]
pub struct UpdateProjection {
    pub automatic: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub last_checked: Option<u64>,
    pub notes: String,
    pub message: String,
    pub phase: UpdatePhase,
}

impl Default for UpdateProjection {
    fn default() -> Self {
        Self {
            automatic: true,
            current_version: "正在读取版本…".into(),
            latest_version: None,
            last_checked: None,
            notes: String::new(),
            message: String::new(),
            phase: UpdatePhase::Loading,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum UpdateRequest {
    Check,
    Automatic(bool),
    Install,
    Later,
    ReleasePage,
}
