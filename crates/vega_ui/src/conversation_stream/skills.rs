//! Composer Skills are presentation and user intent only. Catalog scans,
//! consent CAS and root reads happen in the app/controller worker.

use super::*;
use vega_conversation::types::{SkillComposerCandidate, SkillComposerMutation};

impl ConversationStream {
    pub(crate) fn clear_skill_route_state(&mut self) {
        self.skill_projection_generation = self.skill_projection_generation.wrapping_add(1);
        self.skill_projection = None;
        self.skill_mutation_pending = false;
        self.skill_intent = None;
        self.active_skills.clear();
    }

    /// Settings may change consent while a catalog/mutation worker is still
    /// running. Retire its presentation owner; keep a draft's user intent so
    /// first send must still revalidate instead of silently omitting it.
    pub fn suspend_skill_projection(&mut self, cx: &mut Context<Self>) {
        self.skill_projection_generation = self.skill_projection_generation.wrapping_add(1);
        self.skill_projection = None;
        self.skill_mutation_pending = false;
        cx.notify();
    }

    /// Route-fenced read request. A draft has no Store row and no write is
    /// initiated by opening this picker.
    pub fn request_skill_projection(&mut self, cx: &mut Context<Self>) {
        self.skill_projection_generation = self.skill_projection_generation.wrapping_add(1);
        cx.emit(SkillComposerProjectionRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
            draft: self.draft_route,
            generation: self.skill_projection_generation,
        });
        cx.notify();
    }

    pub fn skill_projection_generation(&self) -> u64 {
        self.skill_projection_generation
    }

    #[doc(hidden)]
    pub fn composer_skill_pin_count(&self) -> Option<usize> {
        self.skill_projection
            .as_ref()
            .map(|projection| projection.pins.len())
    }

    #[doc(hidden)]
    pub fn composer_skill_mutation_pending(&self) -> bool {
        self.skill_mutation_pending
    }

    /// The exact reviewed candidates the (now Settings-owned) picker would
    /// present, for the app integration harness. Issue #96 removed the
    /// Composer entry, so this read-only projection is the harness seam that
    /// still exercises the real `choose_skill_candidate` intent path.
    #[doc(hidden)]
    pub fn composer_skill_candidates(&self) -> Vec<SkillComposerCandidate> {
        self.skill_projection
            .as_ref()
            .map(|projection| projection.candidates.clone())
            .unwrap_or_default()
    }

    pub fn apply_skill_projection(
        &mut self,
        generation: u64,
        result: Result<SkillComposerProjection, &'static str>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.skill_projection_generation {
            return;
        }
        match result {
            Ok(projection) if projection.thread_id == self.thread.id => {
                self.skill_projection = Some(projection);
            }
            _ => {
                self.skill_projection = None;
                self.controller_error = Some("Skills 来源已变化，请刷新后重试".into());
            }
        }
        cx.notify();
    }

    /// A draft stores only an exact choice intent. A durable thread requests
    /// a worker-side CAS pin, blocking repeat clicks until acknowledgement.
    pub fn choose_skill_candidate(
        &mut self,
        candidate: &SkillComposerCandidate,
        cx: &mut Context<Self>,
    ) {
        if self.skill_mutation_pending || self.composer_submit_pending || self.actions.running {
            return;
        }
        let Some(projection) = self.skill_projection.as_ref() else {
            return;
        };
        let expected_generation = projection.consent_generation;
        if !projection.candidates.iter().any(|item| {
            item.source_id == candidate.source_id
                && item.name == candidate.name
                && item.content_sha256 == candidate.content_sha256
        }) {
            return;
        }
        if self.draft_route {
            let intent = SkillSelectionIntent {
                source_id: candidate.source_id.clone(),
                name: candidate.name.clone(),
                content_sha256: candidate.content_sha256.clone(),
                expected_consent_generation: expected_generation,
            };
            self.skill_intent = (self.skill_intent.as_ref() != Some(&intent)).then_some(intent);
            cx.notify();
            return;
        }
        self.skill_mutation_pending = true;
        cx.emit(SkillComposerMutationRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
            owner_generation: self.skill_projection_generation,
            expected_generation,
            mutation: if candidate.selected {
                SkillComposerMutation::Unpin {
                    name: candidate.name.clone(),
                }
            } else {
                SkillComposerMutation::Pin {
                    source_id: candidate.source_id.clone(),
                    name: candidate.name.clone(),
                    content_sha256: candidate.content_sha256.clone(),
                }
            },
        });
        cx.notify();
    }

    pub fn remove_skill_pin(&mut self, name: String, cx: &mut Context<Self>) {
        if self.skill_mutation_pending || self.draft_route {
            return;
        }
        let Some(projection) = self.skill_projection.as_ref() else {
            return;
        };
        if !projection.pins.iter().any(|pin| pin.name == name) {
            return;
        }
        self.skill_mutation_pending = true;
        cx.emit(SkillComposerMutationRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
            owner_generation: self.skill_projection_generation,
            expected_generation: projection.consent_generation,
            mutation: SkillComposerMutation::Unpin { name },
        });
        cx.notify();
    }

    pub fn disable_active_skill(&mut self, name: String, cx: &mut Context<Self>) {
        if self.skill_mutation_pending || self.draft_route {
            return;
        }
        let Some(projection) = self.skill_projection.as_ref() else {
            return;
        };
        let Some(active) = self.active_skills.iter().find(|item| item.name == name) else {
            return;
        };
        self.skill_mutation_pending = true;
        cx.emit(SkillComposerMutationRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
            owner_generation: self.skill_projection_generation,
            expected_generation: projection.consent_generation,
            mutation: SkillComposerMutation::DisableFuture {
                name: active.name.clone(),
                source_label: active.source_label.clone(),
                content_sha256: active.content_sha256.clone(),
            },
        });
        cx.notify();
    }

    pub fn finish_skill_mutation(&mut self, success: bool, cx: &mut Context<Self>) {
        self.skill_mutation_pending = false;
        if success {
            self.controller_error = None;
            self.request_skill_projection(cx);
        } else {
            self.controller_error = Some("Skill 授权或文件已变化，请刷新审阅后重试".into());
            cx.notify();
        }
    }

    pub fn finish_submitted_skill_pin(&mut self, success: bool, cx: &mut Context<Self>) {
        if success {
            self.skill_intent = None;
            self.controller_error = None;
            self.request_skill_projection(cx);
        } else {
            self.reject_composer_submission(cx);
            self.controller_error =
                Some("Skill 授权或文件已变化；请刷新审阅后重试，消息尚未发送".into());
            cx.notify();
        }
    }

    pub(crate) fn render_active_skills(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut row = div()
            .debug_selector(|| "active-skills".into())
            .flex()
            .flex_col()
            .gap_1();
        for skill in &self.active_skills {
            let name = skill.name.clone();
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(format!("正在使用 {} · {}", skill.name, skill.source_label))
                    .child(
                        div()
                            .id("active-skill-stop")
                            .debug_selector(|| "active-skill-stop".into())
                            .focusable()
                            .tab_stop(true)
                            .aria_label("停止当前任务")
                            .focus_visible(move |style| {
                                style.border_1().border_color(colors.brand_primary)
                            })
                            .cursor_pointer()
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.request_composer_stop(cx);
                                }),
                            )
                            .on_key_down(cx.listener(
                                |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.request_composer_stop(cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child("停止"),
                    )
                    .child(
                        div()
                            .id("active-skill-disable")
                            .debug_selector(|| "active-skill-disable".into())
                            .focusable()
                            .tab_stop(true)
                            .aria_label("停用此 Skill 的后续激活")
                            .focus_visible(move |style| {
                                style.border_1().border_color(colors.brand_primary)
                            })
                            .cursor_pointer()
                            .on_key_down(cx.listener({
                                let name = name.clone();
                                move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.disable_active_skill(name.clone(), cx);
                                        cx.stop_propagation();
                                    }
                                }
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.disable_active_skill(name.clone(), cx);
                                }),
                            )
                            .child("停用后续激活"),
                    ),
            );
        }
        row.when(!self.active_skills.is_empty(), |row| {
            row.child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child("停止或停用不能撤销已完成的操作"),
            )
        })
        .into_any_element()
    }
}
