use super::{
    ComputerUseError, Observation, Target,
    feedback::{ExecutionState, ToolFailure},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};

#[derive(Default)]
pub(super) struct ProgressGuard {
    failures: HashMap<String, u8>,
    actions: HashMap<String, (String, u8)>,
    reads: VecDeque<(String, ReadState, u8)>,
    stopped: Option<GuardDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum GuardTrigger {
    RepeatedDirectory,
    RepeatedWindow,
    RepeatedFailure,
    UnchangedAction,
    FailureCapacity,
    ActionCapacity,
    ScreenshotCount,
    ScreenshotBytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum UnderlyingResult {
    Succeeded,
    Failed,
    OutcomeUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuardDiagnostic {
    pub trigger: GuardTrigger,
    pub count: usize,
    pub threshold: usize,
    pub underlying_result: UnderlyingResult,
    pub dispatch_started: bool,
}

impl GuardDiagnostic {
    pub(super) fn valid(self) -> bool {
        let threshold = match self.trigger {
            GuardTrigger::RepeatedDirectory
            | GuardTrigger::RepeatedWindow
            | GuardTrigger::RepeatedFailure
            | GuardTrigger::UnchangedAction => 3,
            GuardTrigger::FailureCapacity | GuardTrigger::ActionCapacity => 64,
            GuardTrigger::ScreenshotCount => crate::images::MAX_IMAGES,
            GuardTrigger::ScreenshotBytes => crate::images::MAX_TURN_IMAGE_BYTES,
        };
        self.threshold == threshold
            && match self.trigger {
                GuardTrigger::RepeatedDirectory
                | GuardTrigger::RepeatedWindow
                | GuardTrigger::UnchangedAction => {
                    self.count == 3
                        && self.underlying_result == UnderlyingResult::Succeeded
                        && self.dispatch_started
                }
                GuardTrigger::RepeatedFailure => {
                    self.count == 3 && self.underlying_result == UnderlyingResult::Failed
                }
                GuardTrigger::FailureCapacity => {
                    self.count == 65 && self.underlying_result == UnderlyingResult::Failed
                }
                GuardTrigger::ActionCapacity => {
                    self.count == 65
                        && self.underlying_result == UnderlyingResult::Succeeded
                        && self.dispatch_started
                }
                GuardTrigger::ScreenshotCount => {
                    self.count == crate::images::MAX_IMAGES + 1
                        && self.underlying_result == UnderlyingResult::Succeeded
                        && self.dispatch_started
                }
                GuardTrigger::ScreenshotBytes => {
                    self.count > threshold
                        && self.count <= threshold + crate::images::MAX_IMAGE_BYTES
                        && self.underlying_result == UnderlyingResult::Succeeded
                        && self.dispatch_started
                }
            }
    }
}

pub(super) struct Attempt {
    target: String,
    operation: String,
    action: Option<String>,
    state: Option<String>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn stable(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "element_token"
                            | "element_index"
                            | "snapshot_id"
                            | "capture_id"
                            | "observation_id"
                            | "target_id"
                            | "cursor"
                            | "timestamp"
                            | "updated_at"
                    )
                })
                .map(|(key, value)| (key.clone(), stable(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(stable).collect()),
        _ => value.clone(),
    }
}

fn state(observation: &Observation) -> Option<String> {
    let pixels = observation
        .image
        .as_ref()
        .and_then(|image| image::load_from_memory(image.bytes()).ok())
        .map(|image| digest(image.to_rgba8().as_raw()));
    let elements = stable(&observation.elements);
    if pixels.is_none()
        && !elements
            .as_array()
            .is_some_and(|elements| !elements.is_empty())
    {
        return None;
    }
    Some(digest(
        serde_json::json!({"elements":elements,"pixels":pixels})
            .to_string()
            .as_bytes(),
    ))
}

#[derive(PartialEq)]
struct Pixels {
    width: u32,
    height: u32,
    columns: Vec<[u8; 32]>,
}

impl Pixels {
    fn new(observation: &Observation) -> Option<Self> {
        let image = image::load_from_memory(observation.image.as_ref()?.bytes())
            .ok()?
            .to_rgba8();
        let mut columns = vec![Sha256::new(); image.width() as usize];
        for (x, _, pixel) in image.enumerate_pixels() {
            columns[x as usize].update(pixel.0);
        }
        Some(Self {
            width: image.width(),
            height: image.height(),
            columns: columns
                .into_iter()
                .map(|column| column.finalize().into())
                .collect(),
        })
    }

    fn only_caret_changed(&self, other: &Self) -> bool {
        if self.width != other.width || self.height != other.height {
            return false;
        }
        let mut changed = self
            .columns
            .iter()
            .zip(&other.columns)
            .enumerate()
            .filter_map(|(index, (a, b))| (a != b).then_some(index));
        let Some(first) = changed.next() else {
            return true;
        };
        changed.next_back().unwrap_or(first) - first < 4
    }
}

enum ReadState {
    Catalog(String),
    Window {
        elements: String,
        pixels: Option<Pixels>,
        text_input: bool,
    },
}

impl ReadState {
    fn new(observation: &Observation) -> Option<(String, Self)> {
        match &observation.target {
            Target::DesktopCatalog => {
                let page: Value = serde_json::from_str(&observation.text).ok()?;
                let windows = page["windows"].as_array()?;
                let mut identities = windows
                    .iter()
                    .map(|window| format!("{}:{}", window["pid"], window["window_id"]))
                    .collect::<Vec<_>>();
                identities.sort();
                let key = format!("catalog:{}", digest(identities.join(",").as_bytes()));
                let mut entries = windows
                    .iter()
                    .map(|window| stable(window).to_string())
                    .collect::<Vec<_>>();
                entries.sort();
                Some((key, Self::Catalog(digest(entries.join("\n").as_bytes()))))
            }
            Target::Desktop(target) => {
                let elements = stable(&observation.elements);
                let pixels = Pixels::new(observation);
                if pixels.is_none()
                    && !elements
                        .as_array()
                        .is_some_and(|entries| !entries.is_empty())
                {
                    return None;
                }
                let text_input = elements.as_array().is_some_and(|entries| {
                    entries.iter().any(|entry| {
                        matches!(
                            entry["role"].as_str(),
                            Some("AXTextArea" | "AXTextField" | "AXSearchField")
                        )
                    })
                });
                Some((
                    format!("window:{}:{}", target.pid, target.window_id),
                    Self::Window {
                        elements: digest(elements.to_string().as_bytes()),
                        pixels,
                        text_input,
                    },
                ))
            }
            Target::Browser { .. } => None,
        }
    }

    fn equivalent(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Catalog(a), Self::Catalog(b)) => a == b,
            (
                Self::Window {
                    elements: a,
                    pixels: pa,
                    text_input: ta,
                },
                Self::Window {
                    elements: b,
                    pixels: pb,
                    text_input: tb,
                },
            ) => {
                a == b
                    && (pa == pb
                        || (*ta
                            && *tb
                            && pa
                                .as_ref()
                                .zip(pb.as_ref())
                                .is_some_and(|(a, b)| a.only_caret_changed(b))))
            }
            _ => false,
        }
    }
}

impl Attempt {
    pub(super) fn new(
        name: &str,
        arguments: &Value,
        target: Option<&Target>,
        previous: Option<&Observation>,
    ) -> Self {
        let target = match target {
            Some(Target::Desktop(target)) => format!("{}:{}", target.pid, target.window_id),
            _ => "arguments".into(),
        };
        let action = if name == "ComputerAct" {
            previous.and_then(|observation| {
                super::backend::map_action(super::Capability::Desktop, observation, arguments)
                    .ok()
                    .map(|(tool, mut mapped)| {
                        if let Some(token) = arguments["target"].as_str() {
                            mapped["semantic_element"] = observation
                                .elements
                                .as_array()
                                .and_then(|elements| {
                                    elements.iter().find(|element| {
                                        element["element_token"].as_str() == Some(token)
                                    })
                                })
                                .map(stable)
                                .unwrap_or(Value::Null);
                        }
                        digest(
                            serde_json::json!({"tool":tool,"arguments":stable(&mapped)})
                                .to_string()
                                .as_bytes(),
                        )
                    })
            })
        } else {
            None
        };
        Self {
            target,
            operation: name.into(),
            action,
            state: previous.and_then(state),
        }
    }
}

impl ProgressGuard {
    fn record_read(&mut self, key: String, state: ReadState, baseline: u8) -> u8 {
        let prior = self
            .reads
            .iter()
            .position(|(target, _, _)| target == &key)
            .and_then(|index| self.reads.remove(index));
        let count = match prior {
            Some((_, previous, count)) if previous.equivalent(&state) => count.saturating_add(1),
            _ => baseline,
        };
        if self.reads.len() >= 64 {
            self.reads.pop_front();
        }
        self.reads.push_back((key, state, count));
        count
    }

    fn reset_reads_for_progress(&mut self, target: &str) {
        let current = format!("screenshot:{target}");
        self.reads
            .retain(|(key, _, _)| key.starts_with("screenshot:") && key != &current);
    }

    pub(super) fn delivered(&mut self, target: &super::DesktopTarget) {
        self.reset_reads_for_progress(&format!("{}:{}", target.pid, target.window_id));
    }

    pub(super) fn screenshot_limit(&mut self, count: usize, bytes: usize) {
        self.stopped = Some(GuardDiagnostic {
            trigger: if count > crate::images::MAX_IMAGES {
                GuardTrigger::ScreenshotCount
            } else {
                GuardTrigger::ScreenshotBytes
            },
            count: if count > crate::images::MAX_IMAGES {
                count
            } else {
                bytes
            },
            threshold: if count > crate::images::MAX_IMAGES {
                crate::images::MAX_IMAGES
            } else {
                crate::images::MAX_TURN_IMAGE_BYTES
            },
            underlying_result: UnderlyingResult::Succeeded,
            dispatch_started: true,
        });
    }

    pub(super) fn stopped(&self) -> Option<GuardDiagnostic> {
        self.stopped
    }

    fn stop(
        &mut self,
        trigger: GuardTrigger,
        count: usize,
        threshold: usize,
        result: &Result<String, ToolFailure>,
        dispatch_started: bool,
    ) -> Option<GuardDiagnostic> {
        let diagnostic = GuardDiagnostic {
            trigger,
            count,
            threshold,
            underlying_result: match result {
                Ok(_) => UnderlyingResult::Succeeded,
                Err(failure) if failure.execution_state == ExecutionState::OutcomeUnknown => {
                    UnderlyingResult::OutcomeUnknown
                }
                Err(_) => UnderlyingResult::Failed,
            },
            dispatch_started,
        };
        self.stopped = Some(diagnostic);
        self.stopped
    }

    pub(super) fn finish(
        &mut self,
        attempt: Attempt,
        result: &Result<String, ToolFailure>,
        after: Option<&Observation>,
        screenshot_reused: bool,
        dispatch_started: bool,
    ) -> Option<GuardDiagnostic> {
        if self.stopped.is_some() {
            return self.stopped;
        }
        if let Err(failure) = result {
            let error = failure.error;
            if matches!(
                error,
                ComputerUseError::Cancelled | ComputerUseError::Busy | ComputerUseError::NotReady
            ) {
                return None;
            }
            let key = format!(
                "{}:{}:{}",
                attempt.target,
                attempt.operation,
                error.feedback_value()
            );
            if self.failures.len() >= 64 && !self.failures.contains_key(&key) {
                return self.stop(
                    GuardTrigger::FailureCapacity,
                    65,
                    64,
                    result,
                    dispatch_started,
                );
            }
            let count = self.failures.entry(key).or_default();
            *count = count.saturating_add(1);
            let count = *count;
            return if count >= 3 {
                self.stop(
                    GuardTrigger::RepeatedFailure,
                    usize::from(count),
                    3,
                    result,
                    dispatch_started,
                )
            } else {
                None
            };
        }
        let read = after
            .filter(|_| {
                attempt.operation == "ComputerObserve"
                    || (attempt.operation == "ComputerScreenshot" && screenshot_reused)
            })
            .and_then(ReadState::new)
            .map(|(key, state)| {
                let key = if screenshot_reused {
                    format!("screenshot:{}", attempt.target)
                } else {
                    key
                };
                (key, state)
            });
        if let Some((key, state)) = read {
            let trigger = if matches!(state, ReadState::Catalog(_)) {
                GuardTrigger::RepeatedDirectory
            } else {
                GuardTrigger::RepeatedWindow
            };
            let count = self.record_read(key, state, 1);
            if count >= 3 {
                return self.stop(trigger, usize::from(count), 3, result, dispatch_started);
            }
        }
        let after = after.filter(|observation| matches!(observation.target, Target::Desktop(_)))?;
        self.failures.retain(|key, _| {
            !key.starts_with(&format!("{}:{}:", attempt.target, attempt.operation))
        });
        let action = attempt.action?;
        if attempt.state.is_none() || attempt.state != state(after) {
            self.actions.remove(&attempt.target);
            if attempt.state.is_some() {
                self.reset_reads_for_progress(&attempt.target);
                if let Some((key, state)) = ReadState::new(after) {
                    self.record_read(key, state, 0);
                }
            }
            return None;
        }
        if self.actions.len() >= 64 && !self.actions.contains_key(&attempt.target) {
            return self.stop(
                GuardTrigger::ActionCapacity,
                65,
                64,
                result,
                dispatch_started,
            );
        }
        let entry = self
            .actions
            .entry(attempt.target)
            .or_insert_with(|| (action.clone(), 0));
        if entry.0 != action {
            *entry = (action, 0);
        }
        entry.1 = entry.1.saturating_add(1);
        let count = entry.1;
        if count >= 3 {
            self.stop(
                GuardTrigger::UnchangedAction,
                usize::from(count),
                3,
                result,
                dispatch_started,
            )
        } else {
            None
        }
    }
}
