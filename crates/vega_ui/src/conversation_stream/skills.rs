//! Composer Skills are presentation and user intent only. Catalog scans,
//! consent CAS and root reads happen in the app/controller worker.

use super::*;
use vega_conversation::types::{SkillComposerCandidate, SkillComposerMutation};

impl ConversationStream {
    pub(crate) fn clear_skill_route_state(&mut self) {
        self.skill_projection_generation = self.skill_projection_generation.wrapping_add(1);
        self.skill_projection = None;
        self.skill_projection_loading = false;
        self.skill_picker_open = false;
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
        self.skill_projection_loading = false;
        self.skill_mutation_pending = false;
        self.skill_picker_open = false;
        cx.notify();
    }

    /// Route-fenced read request. A draft has no Store row and no write is
    /// initiated by opening this picker.
    pub fn request_skill_projection(&mut self, cx: &mut Context<Self>) {
        self.skill_projection_generation = self.skill_projection_generation.wrapping_add(1);
        self.skill_projection_loading = true;
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

    pub fn apply_skill_projection(
        &mut self,
        generation: u64,
        result: Result<SkillComposerProjection, &'static str>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.skill_projection_generation {
            return;
        }
        self.skill_projection_loading = false;
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
            self.skill_picker_open = false;
            cx.notify();
            return;
        }
        self.skill_mutation_pending = true;
        self.skill_picker_open = false;
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

    fn toggle_skill_picker_state(&mut self, cx: &mut Context<Self>) {
        if self.skill_mutation_pending {
            return;
        }
        if self.skill_picker_open {
            self.skill_picker_open = false;
        } else {
            self.close_composer_popovers(cx);
            self.skill_picker_open = true;
            self.request_skill_projection(cx);
        }
        cx.notify();
    }

    fn toggle_skill_picker(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_skill_picker_state(cx);
    }

    pub(crate) fn render_skill_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let selected = self
            .skill_intent
            .as_ref()
            .map(|intent| intent.name.as_str());
        let pins = self
            .skill_projection
            .as_ref()
            .map(|projection| projection.pins.len())
            .unwrap_or_default();
        let label = if self.skill_mutation_pending {
            "Skills 保存中…".into()
        } else if let Some(name) = selected {
            format!("Skill: {name}")
        } else if pins > 0 {
            format!("Skills {pins}")
        } else {
            "Skills".into()
        };
        div()
            .relative()
            .flex_shrink_0()
            .child(
                div()
                    .id("composer-skills")
                    .debug_selector(|| "composer-skills".into())
                    .focusable()
                    .tab_stop(true)
                    .aria_label("选择 Skills")
                    .focus_visible(move |style| style.border_1().border_color(colors.brand_primary))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .cursor_pointer()
                    .hover(move |row| row.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::toggle_skill_picker))
                    .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.toggle_skill_picker_state(cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(label),
            )
            .when(self.skill_picker_open, |row| {
                row.child(self.render_skill_picker_rows(cx))
            })
            .into_any_element()
    }

    fn render_skill_picker_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(projection) = &self.skill_projection {
            for candidate in projection.candidates.iter().take(32) {
                let candidate = candidate.clone();
                let selected = self.skill_intent.as_ref().is_some_and(|intent| {
                    intent.source_id == candidate.source_id
                        && intent.name == candidate.name
                        && intent.content_sha256 == candidate.content_sha256
                }) || candidate.selected;
                let selector = format!("composer-skill-row-{}", candidate.name);
                let key_candidate = candidate.clone();
                let label = format!(
                    "{}{} · {}",
                    if selected { "✓ " } else { "" },
                    candidate.name,
                    candidate.source_label
                );
                rows.push(
                    div()
                        .id(selector.clone())
                        .debug_selector(move || selector.clone())
                        .focusable()
                        .tab_stop(true)
                        .focus_visible(move |style| {
                            style.border_1().border_color(colors.brand_primary)
                        })
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_primary)
                        .cursor_pointer()
                        .hover(move |row| row.bg(colors.bg_hover))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.choose_skill_candidate(&candidate, cx);
                            }),
                        )
                        .on_key_down(cx.listener(
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.choose_skill_candidate(&key_candidate, cx);
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .child(label)
                        .into_any_element(),
                );
            }
            for pin in projection.pins.iter().filter(|pin| !pin.available) {
                let name = pin.name.clone();
                let selector = format!("composer-skill-stale-{name}");
                rows.push(
                    div()
                        .id(selector.clone())
                        .debug_selector(move || selector.clone())
                        .focusable()
                        .tab_stop(true)
                        .focus_visible(move |style| {
                            style.border_1().border_color(colors.brand_primary)
                        })
                        .px_2()
                        .py_1()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.warning)
                        .cursor_pointer()
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.remove_skill_pin(name.clone(), cx);
                            }),
                        )
                        .on_key_down(cx.listener({
                            let name = pin.name.clone();
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.remove_skill_pin(name.clone(), cx);
                                    cx.stop_propagation();
                                }
                            }
                        }))
                        .child(format!("{} · 已变化（点击移除）", pin.name))
                        .into_any_element(),
                );
            }
        }
        if rows.is_empty() {
            rows.push(
                div()
                    .px_2()
                    .py_1()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child(if self.skill_projection_loading {
                        "正在读取已审阅 Skills…"
                    } else {
                        "请先在设置中审阅并启用 Skills"
                    })
                    .into_any_element(),
            );
        }
        div()
            .id("composer-skills-menu")
            .debug_selector(|| "composer-skills-menu".into())
            .absolute()
            .bottom(gpui_kit::relative(1.0))
            .left_0()
            .mb_2()
            .w(px(280.))
            .max_h(px(300.))
            .overflow_y_scroll()
            .p_2()
            .rounded(px(Layout::MENU_RADIUS))
            .bg(colors.bg_elevated)
            .border_1()
            .border_color(colors.border_subtle)
            .shadow_sm()
            .children(rows)
            .into_any_element()
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
