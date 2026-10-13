use super::{ComputerUseError, Observation, Target};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};

#[derive(Default)]
pub(super) struct ProgressGuard {
    failures: HashMap<String, u8>,
    actions: HashMap<String, (String, u8)>,
    reads: VecDeque<(String, ReadState, u8)>,
    stopped: bool,
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
    fn record_read(&mut self, key: String, state: ReadState, baseline: u8) -> bool {
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
        count >= 3
    }

    fn reset_reads_for_progress(&mut self, target: &str) {
        let current = format!("screenshot:{target}");
        self.reads
            .retain(|(key, _, _)| key.starts_with("screenshot:") && key != &current);
    }

    pub(super) fn delivered(&mut self, target: &super::DesktopTarget) {
        self.reset_reads_for_progress(&format!("{}:{}", target.pid, target.window_id));
    }

    pub(super) fn stopped(&self) -> bool {
        self.stopped
    }

    pub(super) fn finish(
        &mut self,
        attempt: Attempt,
        result: &Result<String, ComputerUseError>,
        after: Option<&Observation>,
        screenshot_reused: bool,
    ) -> bool {
        if self.stopped {
            return true;
        }
        if let Err(error) = result {
            if matches!(
                error,
                ComputerUseError::Cancelled | ComputerUseError::Busy | ComputerUseError::NotReady
            ) {
                return false;
            }
            let key = format!(
                "{}:{}:{}",
                attempt.target,
                attempt.operation,
                error.feedback()
            );
            if self.failures.len() >= 64 && !self.failures.contains_key(&key) {
                self.stopped = true;
                return true;
            }
            let count = self.failures.entry(key).or_default();
            *count = count.saturating_add(1);
            self.stopped = *count >= 3;
            return self.stopped;
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
        if let Some((key, state)) = read
            && self.record_read(key, state, 1)
        {
            self.stopped = true;
            return true;
        }
        let Some(after) =
            after.filter(|observation| matches!(observation.target, Target::Desktop(_)))
        else {
            return false;
        };
        self.failures.retain(|key, _| {
            !key.starts_with(&format!("{}:{}:", attempt.target, attempt.operation))
        });
        let Some(action) = attempt.action else {
            return false;
        };
        if attempt.state.is_none() || attempt.state != state(after) {
            self.actions.remove(&attempt.target);
            if attempt.state.is_some() {
                self.reset_reads_for_progress(&attempt.target);
                if let Some((key, state)) = ReadState::new(after) {
                    self.record_read(key, state, 0);
                }
            }
            return false;
        }
        if self.actions.len() >= 64 && !self.actions.contains_key(&attempt.target) {
            self.stopped = true;
            return true;
        }
        let entry = self
            .actions
            .entry(attempt.target)
            .or_insert_with(|| (action.clone(), 0));
        if entry.0 != action {
            *entry = (action, 0);
        }
        entry.1 = entry.1.saturating_add(1);
        self.stopped = entry.1 >= 3;
        self.stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer_use::{ComputerUseControl, tests::fixture_observation};
    use serde_json::json;

    #[test]
    fn issue75_usability_refresh_refs_and_successful_observe_do_not_reset_action_failures() {
        let control = ComputerUseControl::new();
        let mut guard = ProgressGuard::default();
        for index in 0..3 {
            let mut observation = fixture_observation(&control, 1, index);
            if let Target::Desktop(target) = &mut observation.target {
                target.selection_id = format!("choice-{index}");
            }
            let args =
                json!({"action":"click","observation_id":observation.id,"x":"invalid","y":1});
            assert_eq!(
                guard.finish(
                    Attempt::new(
                        "ComputerAct",
                        &args,
                        Some(&observation.target),
                        Some(&observation)
                    ),
                    &Err(ComputerUseError::InvalidArguments("x")),
                    None,
                    false
                ),
                index == 2
            );
            if index < 2 {
                assert!(!guard.finish(
                    Attempt::new(
                        "ComputerObserve",
                        &json!({"target_id":format!("choice-{index}")}),
                        Some(&observation.target),
                        None
                    ),
                    &Ok("observed".into()),
                    Some(&observation),
                    false
                ));
            }
        }
    }

    #[test]
    fn issue75_usability_reference_changes_do_not_hide_repeated_action_without_state_change() {
        let control = ComputerUseControl::new();
        let mut guard = ProgressGuard::default();
        for index in 0..3 {
            let mut before = fixture_observation(&control, 1, index);
            before.elements =
                json!([{"element_token":format!("token-{index}"),"role":"button","label":"Next"}]);
            let mut after = fixture_observation(&control, 1, index + 1);
            after.elements = json!([{"element_token":format!("token-{}", index+1),"role":"button","label":"Next"}]);
            let args = json!({"observation_id":before.id,"action":"click","target":format!("token-{index}")});
            assert_eq!(
                guard.finish(
                    Attempt::new("ComputerAct", &args, Some(&before.target), Some(&before)),
                    &Ok("done".into()),
                    Some(&after),
                    false
                ),
                index == 2
            );
        }
    }

    #[test]
    fn issue75_usability_viewport_progress_is_valid_even_with_identical_ax_tree() {
        let control = ComputerUseControl::new();
        let mut guard = ProgressGuard::default();
        let mut before = fixture_observation(&control, 1, 0);
        before.elements = json!([{"element_token":"pane","role":"scroll_area","label":"Document"}]);
        for index in 1..8 {
            let mut after = before.clone();
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                4,
                3,
                image::Rgb([index, 0, 0]),
            ));
            let mut bytes = std::io::Cursor::new(Vec::new());
            image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
            after.image = Some(crate::ImageAttachment::from_bytes(bytes.into_inner()).unwrap());
            let args = json!({"action":"scroll","target":"pane","direction":"down","amount":1});
            assert!(!guard.finish(
                Attempt::new("ComputerAct", &args, Some(&before.target), Some(&before)),
                &Ok("done".into()),
                Some(&after),
                false
            ));
            before = after;
        }
    }
}

#[cfg(test)]
mod unresolved_target_tests {
    use super::*;
    #[test]
    fn issue75_usability_same_error_with_changed_unresolved_references_and_values_stops_at_three() {
        for operation in ["ComputerObserve", "ComputerAct", "ComputerScreenshot"] {
            let mut guard = ProgressGuard::default();
            for index in 0..3 {
                let args = serde_json::json!({"observation_id":format!("unknown-{index}"),"target_id":format!("expired-{index}"),"text":format!("unused-{index}"),"x":index});
                assert_eq!(
                    guard.finish(
                        Attempt::new(operation, &args, None, None),
                        &Err(ComputerUseError::InvalidArguments("target_id")),
                        None,
                        false
                    ),
                    index == 2
                );
            }
        }
    }
}
