//! Settings view (A1-10 UI skeleton): provider list, add-provider form, and
//! default model / permission mode pickers.
//!
//! The view is opened with Cmd+, ([`OpenSettings`]) and closed with Esc or
//! the back button ([`CloseSettings`]); whether it replaces the session
//! placeholder is tracked by the [`SettingsOpen`] global, following the
//! global pattern proven in T07. It loads the config from the config root
//! (`vega_store::paths`, tech-spec §6) when constructed and saves it back on
//! every mutation, so configuration survives a restart.
//!
//! Credentials never appear in the UI: the key form field is masked while
//! typing and every stored provider shows the constant "•••••••已存储"
//! placeholder; the key value itself only ever goes to the Keychain.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Div, Entity, EventEmitter, FocusHandle, Global, MouseButton, MouseUpEvent,
    Window, actions, div, px, relative,
};
use vega_conversation::types::{
    PricingDraftReason, PricingEntryKind, PricingEntryProjection, PricingMutation, PricingNotice,
    PricingRateInputs, PricingSettingsErrorCode, PricingSettingsProjection,
};
use vega_store::config::{self, AppConfig, ProviderConfig};
use vega_store::keystore;
use vega_theme::{Typography, theme};

use crate::text_input::TextInput;

actions!(
    vega_settings,
    [
        OpenSettings,
        CloseSettings,
        ActivatePricingAction,
        NextPricingAction,
        PreviousPricingAction
    ]
);

/// Typed pricing mutation emitted to the app-owned controller.
pub struct PricingMutationRequested {
    pub generation: u64,
    pub mutation: Result<PricingMutation, PricingSettingsErrorCode>,
}

/// Explicit recovery/reload request emitted to the app-owned controller.
pub struct PricingReloadRequested;

/// Retries the controller-owned exact dirty pricing plan.
pub struct PricingRetryRequested {
    pub generation: u64,
}

/// Discards the controller-owned dirty plan and keeps current authority.
pub struct PricingDiscardRequested {
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PricingEditorKind {
    AddCustom,
    UpdateCustom,
    UpdateBuiltinBase,
    UpdateDeepSeek,
}

#[derive(Clone)]
struct PricingEditor {
    kind: PricingEditorKind,
    model: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
enum PricingFocusTarget {
    Reload,
    Add,
    Edit(usize),
    Secondary(usize),
    Retry,
    Discard,
    Save,
    Cancel,
}

/// Whether the settings view currently replaces the session placeholder.
///
/// Toggled by the app-level [`OpenSettings`]/[`CloseSettings`] handlers.
pub struct SettingsOpen(pub bool);

impl Global for SettingsOpen {}

/// Fixed permission-mode vocabulary (matches `vega_store::config::Defaults`).
const PERMISSION_MODES: [&str; 3] = ["readonly", "confirm", "auto"];

/// Status placeholder shown for every provider with a non-empty `key_ref`;
/// the key value itself is never rendered (safety red line).
const KEY_STORED_PLACEHOLDER: &str = "•••••••已存储";

/// The settings view: a plain page with the provider list, the add-provider
/// form, and the default pickers. Holds its own form input buffers, so it
/// must be cached by the parent across re-renders (it is rebuilt — reloading
/// the config — each time settings is opened).
pub struct SettingsView {
    config: AppConfig,
    name_input: Entity<TextInput>,
    base_url_input: Entity<TextInput>,
    key_input: Entity<TextInput>,
    mode_open: bool,
    model_open: bool,
    /// Inline error message (ui-spec §4.6: no modals); empty until an IO or
    /// Keychain failure occurs.
    error: Option<String>,
    pricing: PricingSettingsProjection,
    pricing_editor: Option<PricingEditor>,
    pricing_model_input: Entity<TextInput>,
    pricing_rate_inputs: [Entity<TextInput>; 8],
    pricing_focuses: Vec<(PricingFocusTarget, FocusHandle)>,
}

impl EventEmitter<PricingMutationRequested> for SettingsView {}
impl EventEmitter<PricingReloadRequested> for SettingsView {}
impl EventEmitter<PricingRetryRequested> for SettingsView {}
impl EventEmitter<PricingDiscardRequested> for SettingsView {}

const PRICING_INPUT_BYTES_LIMIT: usize = 1024 * 1024;

impl SettingsView {
    /// Loads the current config and creates empty form inputs.
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 构造时载入现有配置：这是"重启后配置恢复"的落点。失败时以内联
        // 错误提示，并以默认配置继续渲染（ui-spec §4.6：不弹模态）。
        let (config, error) = match config::load() {
            Ok(config) => (config, None),
            Err(err) => (AppConfig::default(), Some(format!("配置加载失败：{err}"))),
        };
        Self::from_config(config, error, cx)
    }

    fn from_config(config: AppConfig, error: Option<String>, cx: &mut Context<Self>) -> Self {
        let name_input = cx.new(|cx| TextInput::new(cx, "名称", false));
        let base_url_input = cx.new(|cx| TextInput::new(cx, "Base URL", false));
        let key_input = cx.new(|cx| TextInput::new(cx, "API Key", true));
        let pricing_model_input = cx.new(|cx| TextInput::new(cx, "模型 ID", false));
        let pricing_rate_inputs = [
            cx.new(|cx| TextInput::new(cx, "Base Input", false)),
            cx.new(|cx| TextInput::new(cx, "Base Output", false)),
            cx.new(|cx| TextInput::new(cx, "Base Cache Read", false)),
            cx.new(|cx| TextInput::new(cx, "Base Cache Write", false)),
            cx.new(|cx| TextInput::new(cx, "Peak Input", false)),
            cx.new(|cx| TextInput::new(cx, "Peak Output", false)),
            cx.new(|cx| TextInput::new(cx, "Peak Cache Read", false)),
            cx.new(|cx| TextInput::new(cx, "Peak Cache Write", false)),
        ];
        Self {
            config,
            name_input,
            base_url_input,
            key_input,
            mode_open: false,
            model_open: false,
            error,
            pricing: PricingSettingsProjection::Loading,
            pricing_editor: None,
            pricing_model_input,
            pricing_rate_inputs,
            pricing_focuses: Vec::new(),
        }
    }

    #[cfg(test)]
    fn new_for_test(cx: &mut Context<Self>) -> Self {
        Self::from_config(AppConfig::default(), None, cx)
    }

    /// Applies the latest safe projection from the app-owned pricing controller.
    pub fn apply_pricing_projection(
        &mut self,
        projection: PricingSettingsProjection,
        cx: &mut Context<Self>,
    ) {
        self.pricing = projection;
        if matches!(self.pricing, PricingSettingsProjection::Saving { .. }) {
            self.pricing_editor = None;
        }
        self.rebuild_pricing_focuses(cx);
        cx.notify();
    }

    fn rebuild_pricing_focuses(&mut self, cx: &mut Context<Self>) {
        let mut targets = Vec::new();
        match &self.pricing {
            PricingSettingsProjection::Invalid(_) => targets.push(PricingFocusTarget::Reload),
            PricingSettingsProjection::Ready {
                entries,
                draft_reason,
                ..
            } => {
                targets.push(PricingFocusTarget::Reload);
                if draft_reason.is_some() {
                    targets.push(PricingFocusTarget::Retry);
                    targets.push(PricingFocusTarget::Discard);
                } else {
                    targets.push(PricingFocusTarget::Add);
                    for index in 0..entries.len() {
                        targets.push(PricingFocusTarget::Edit(index));
                        targets.push(PricingFocusTarget::Secondary(index));
                    }
                    if self.pricing_editor.is_some() {
                        targets.push(PricingFocusTarget::Save);
                        targets.push(PricingFocusTarget::Cancel);
                    }
                }
            }
            PricingSettingsProjection::Loading
            | PricingSettingsProjection::Saving { .. }
            | PricingSettingsProjection::Reloading => {}
        }
        self.pricing_focuses = targets
            .into_iter()
            .map(|target| (target, cx.focus_handle().tab_stop(true)))
            .collect();
    }

    fn pricing_focus(&self, target: &PricingFocusTarget) -> Option<FocusHandle> {
        self.pricing_focuses
            .iter()
            .find(|(candidate, _)| candidate == target)
            .map(|(_, focus)| focus.clone())
    }

    fn pricing_generation(&self) -> Option<u64> {
        match &self.pricing {
            PricingSettingsProjection::Ready { generation, .. } => Some(*generation),
            _ => None,
        }
    }

    fn pricing_allows_editing(&self) -> bool {
        matches!(
            self.pricing,
            PricingSettingsProjection::Ready {
                draft_reason: None,
                ..
            }
        )
    }

    fn begin_add_custom(&mut self, cx: &mut Context<Self>) {
        if !self.pricing_allows_editing() {
            return;
        }
        self.pricing_model_input.update(cx, TextInput::clear);
        for input in &self.pricing_rate_inputs {
            input.update(cx, TextInput::clear);
        }
        self.pricing_editor = Some(PricingEditor {
            kind: PricingEditorKind::AddCustom,
            model: None,
        });
        self.rebuild_pricing_focuses(cx);
        cx.notify();
    }

    fn begin_edit_pricing(&mut self, entry: PricingEntryProjection, cx: &mut Context<Self>) {
        if !self.pricing_allows_editing() {
            return;
        }
        let kind = match entry.kind {
            PricingEntryKind::CustomStatic => PricingEditorKind::UpdateCustom,
            PricingEntryKind::BuiltInScheduled => PricingEditorKind::UpdateDeepSeek,
            PricingEntryKind::BuiltInStatic | PricingEntryKind::BuiltInCapped => {
                PricingEditorKind::UpdateBuiltinBase
            }
        };
        self.pricing_model_input
            .update(cx, |input, cx| input.set_text(&entry.model, cx));
        let mut values = rate_values(&entry.base).into_iter();
        for input in &self.pricing_rate_inputs[..4] {
            if let Some(value) = values.next() {
                input.update(cx, |input, cx| input.set_text(value, cx));
            }
        }
        if let Some(peak) = &entry.peak {
            let mut values = rate_values(peak).into_iter();
            for input in &self.pricing_rate_inputs[4..] {
                if let Some(value) = values.next() {
                    input.update(cx, |input, cx| input.set_text(value, cx));
                }
            }
        } else {
            for input in &self.pricing_rate_inputs[4..] {
                input.update(cx, TextInput::clear);
            }
        }
        self.pricing_editor = Some(PricingEditor {
            kind,
            model: Some(entry.model),
        });
        self.rebuild_pricing_focuses(cx);
        cx.notify();
    }

    fn emit_pricing_mutation(&mut self, mutation: PricingMutation, cx: &mut Context<Self>) {
        let Some(generation) = self.pricing_generation() else {
            return;
        };
        let mutation = if pricing_mutation_input_bytes(&mutation)
            .is_some_and(|bytes| bytes <= PRICING_INPUT_BYTES_LIMIT)
        {
            Ok(mutation)
        } else {
            Err(PricingSettingsErrorCode::LimitExceeded)
        };
        cx.emit(PricingMutationRequested {
            generation,
            mutation,
        });
    }

    fn submit_pricing_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.pricing_editor.clone() else {
            return;
        };
        let model = match editor.model {
            Some(model) => model,
            None => self.pricing_model_input.read(cx).text().to_string(),
        };
        let base = read_rate_inputs(&self.pricing_rate_inputs[..4], cx);
        let mutation = match editor.kind {
            PricingEditorKind::AddCustom => PricingMutation::AddCustom { model, rates: base },
            PricingEditorKind::UpdateCustom => PricingMutation::UpdateCustom { model, rates: base },
            PricingEditorKind::UpdateBuiltinBase => {
                PricingMutation::UpdateBuiltinBase { model, rates: base }
            }
            PricingEditorKind::UpdateDeepSeek => PricingMutation::UpdateDeepSeek {
                model,
                base,
                peak: read_rate_inputs(&self.pricing_rate_inputs[4..], cx),
            },
        };
        self.emit_pricing_mutation(mutation, cx);
    }

    fn activate_pricing_action(
        &mut self,
        _: &ActivatePricingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .pricing_focuses
            .iter()
            .find(|(_, focus)| focus.is_focused(window))
            .map(|(target, _)| target.clone());
        let Some(target) = target else {
            cx.propagate();
            return;
        };
        match target {
            PricingFocusTarget::Reload => cx.emit(PricingReloadRequested),
            PricingFocusTarget::Add => self.begin_add_custom(cx),
            PricingFocusTarget::Edit(index) => {
                if let PricingSettingsProjection::Ready {
                    entries,
                    draft_reason: None,
                    ..
                } = &self.pricing
                    && let Some(entry) = entries.get(index).cloned()
                {
                    self.begin_edit_pricing(entry, cx);
                }
            }
            PricingFocusTarget::Secondary(index) => {
                if let PricingSettingsProjection::Ready {
                    entries,
                    draft_reason: None,
                    ..
                } = &self.pricing
                    && let Some(entry) = entries.get(index)
                {
                    let mutation = if entry.kind == PricingEntryKind::CustomStatic {
                        PricingMutation::DeleteCustom {
                            model: entry.model.clone(),
                        }
                    } else {
                        PricingMutation::ResetBuiltin {
                            model: entry.model.clone(),
                        }
                    };
                    self.emit_pricing_mutation(mutation, cx);
                }
            }
            PricingFocusTarget::Retry => {
                if let Some(generation) = self.pricing_generation() {
                    cx.emit(PricingRetryRequested { generation });
                }
            }
            PricingFocusTarget::Discard => {
                if let Some(generation) = self.pricing_generation() {
                    cx.emit(PricingDiscardRequested { generation });
                }
            }
            PricingFocusTarget::Save => self.submit_pricing_editor(cx),
            PricingFocusTarget::Cancel => {
                self.pricing_editor = None;
                self.rebuild_pricing_focuses(cx);
                cx.notify();
            }
        }
    }

    fn move_pricing_focus(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self
            .pricing_focuses
            .iter()
            .position(|(_, focus)| focus.is_focused(window))
        else {
            cx.propagate();
            return;
        };
        let next = if reverse {
            current.checked_sub(1)
        } else {
            current
                .checked_add(1)
                .filter(|next| *next < self.pricing_focuses.len())
        };
        let Some(next) = next else {
            cx.propagate();
            return;
        };
        self.pricing_focuses[next].1.focus(window, cx);
    }

    fn next_pricing_action(
        &mut self,
        _: &NextPricingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_pricing_focus(false, window, cx);
    }

    fn previous_pricing_action(
        &mut self,
        _: &PreviousPricingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_pricing_focus(true, window, cx);
    }

    fn render_pricing(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(section_title(
                "模型定价（USD / 1M tokens）",
                colors.text_primary,
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(action_button(
                        "重新加载",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Reload),
                        cx.listener(|this, _, _, cx| {
                            if !matches!(this.pricing, PricingSettingsProjection::Saving { .. }) {
                                cx.emit(PricingReloadRequested);
                            }
                        }),
                    ))
                    .child(action_button(
                        "添加自定义",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Add),
                        cx.listener(|this, _, _, cx| {
                            this.begin_add_custom(cx);
                        }),
                    )),
            );

        let mut column = div().flex().flex_col().gap_2().child(header);
        match &self.pricing {
            PricingSettingsProjection::Loading | PricingSettingsProjection::Reloading => {
                column = column.child(pricing_status("正在加载定价…", colors.text_secondary));
            }
            PricingSettingsProjection::Invalid(code) => {
                column = column.child(pricing_status(pricing_error_label(*code), colors.danger));
            }
            PricingSettingsProjection::Ready {
                generation,
                entries,
                notice,
                draft_reason,
                error,
                ..
            } => {
                let entries = entries.clone();
                let generation = *generation;
                let notice = *notice;
                let draft_reason = *draft_reason;
                let error = *error;
                if let Some(notice) = notice {
                    column =
                        column.child(pricing_status(pricing_notice_label(notice), colors.warning));
                }
                if let Some(draft_reason) = draft_reason {
                    column = column.child(pricing_status(
                        match draft_reason {
                            PricingDraftReason::RetryPending => "保存未提交；原草稿可重试或放弃",
                            PricingDraftReason::ExternalConflict => {
                                "已采用外部有效版本；当前编辑草稿仍有冲突"
                            }
                        },
                        colors.warning,
                    ));
                    column = column.child(
                        div()
                            .flex()
                            .gap_2()
                            .child(action_button(
                                "重试原草稿",
                                colors,
                                self.pricing_focus(&PricingFocusTarget::Retry),
                                cx.listener(move |_, _, _, cx| {
                                    cx.emit(PricingRetryRequested { generation });
                                }),
                            ))
                            .child(action_button(
                                "放弃草稿",
                                colors,
                                self.pricing_focus(&PricingFocusTarget::Discard),
                                cx.listener(move |_, _, _, cx| {
                                    cx.emit(PricingDiscardRequested { generation });
                                }),
                            )),
                    );
                }
                if let Some(error) = error {
                    column =
                        column.child(pricing_status(pricing_error_label(error), colors.danger));
                }
                for (index, entry) in entries.into_iter().enumerate() {
                    column = column.child(self.render_pricing_entry(index, entry, cx));
                }
            }
            PricingSettingsProjection::Saving { entries, .. } => {
                let entries = entries.clone();
                column = column.child(pricing_status("正在保存并复验…", colors.text_secondary));
                for (index, entry) in entries.into_iter().enumerate() {
                    column = column.child(self.render_pricing_entry(index, entry, cx));
                }
            }
        }
        if let Some(editor) = self.pricing_editor.clone() {
            column = column.child(self.render_pricing_editor(editor, cx));
        }
        column.into_any_element()
    }

    fn render_pricing_entry(
        &mut self,
        index: usize,
        entry: PricingEntryProjection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let summary = if entry.kind == PricingEntryKind::BuiltInScheduled {
            "内置 · Base 4 项 + Peak 4 项 · UTC 时段锁定"
        } else if entry.kind == PricingEntryKind::CustomStatic {
            "自定义 · Static 4 项"
        } else {
            "内置 · Base 4 项 · Profile metadata 锁定"
        };
        let edit_entry = entry.clone();
        let model_for_reset = entry.model.clone();
        let model_for_delete = entry.model.clone();
        div()
            .flex()
            .items_center()
            .justify_between()
            .min_w_0()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().truncate().child(entry.model))
                    .child(
                        div()
                            .text_size(px(Typography::BODY))
                            .text_color(colors.text_secondary)
                            .child(summary),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(action_button(
                        "编辑",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Edit(index)),
                        cx.listener(move |this, _, _, cx| {
                            this.begin_edit_pricing(edit_entry.clone(), cx);
                        }),
                    ))
                    .when(entry.kind != PricingEntryKind::CustomStatic, |row| {
                        row.child(action_button(
                            "重置",
                            colors,
                            self.pricing_focus(&PricingFocusTarget::Secondary(index)),
                            cx.listener(move |this, _, _, cx| {
                                this.emit_pricing_mutation(
                                    PricingMutation::ResetBuiltin {
                                        model: model_for_reset.clone(),
                                    },
                                    cx,
                                );
                            }),
                        ))
                    })
                    .when(entry.kind == PricingEntryKind::CustomStatic, |row| {
                        row.child(action_button(
                            "删除",
                            colors,
                            self.pricing_focus(&PricingFocusTarget::Secondary(index)),
                            cx.listener(move |this, _, _, cx| {
                                this.emit_pricing_mutation(
                                    PricingMutation::DeleteCustom {
                                        model: model_for_delete.clone(),
                                    },
                                    cx,
                                );
                            }),
                        ))
                    }),
            )
            .into_any_element()
    }

    fn render_pricing_editor(
        &mut self,
        editor: PricingEditor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let show_peak = editor.kind == PricingEditorKind::UpdateDeepSeek;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .rounded_lg()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .when(editor.kind == PricingEditorKind::AddCustom, |form| {
                form.child(self.pricing_model_input.clone())
            })
            .children(self.pricing_rate_inputs[..4].iter().cloned())
            .when(show_peak, |form| {
                form.children(self.pricing_rate_inputs[4..].iter().cloned())
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(action_button(
                        "保存定价",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Save),
                        cx.listener(|this, _, _, cx| {
                            this.submit_pricing_editor(cx);
                        }),
                    ))
                    .child(action_button(
                        "取消",
                        colors,
                        self.pricing_focus(&PricingFocusTarget::Cancel),
                        cx.listener(|this, _, _, cx| {
                            this.pricing_editor = None;
                            this.rebuild_pricing_focuses(cx);
                            cx.notify();
                        }),
                    )),
            )
            .into_any_element()
    }

    fn on_back(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        // 与 Esc 同效：派发同一动作，由 app 级处理器统一收口。
        window.dispatch_action(Box::new(CloseSettings), cx);
    }

    fn on_submit(&mut self, _: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text().trim().to_string();
        let base_url = self.base_url_input.read(cx).text().trim().to_string();
        let key = self.key_input.read(cx).text().to_string();
        if !form_is_submittable(&name, &base_url, &key) {
            // 空字段时按钮本应无效；这里的守卫保证即便触发也无副作用。
            return;
        }

        // 凭据只进 Keychain（安全红线）；key_ref 约定为 provider 名称。
        if let Err(error) = keystore::set_key(&name, &key) {
            self.error = Some(format!("Keychain 写入失败：{error}"));
            cx.notify();
            return;
        }

        upsert_provider(
            &mut self.config.providers,
            ProviderConfig {
                name: name.clone(),
                base_url,
                // 表单不编辑 models：新增时为空，同名更新时保留原值。
                models: Vec::new(),
                key_ref: name,
            },
        );

        // 立即落盘，满足"保存 → config.toml 更新；重启后配置恢复"。
        if let Err(error) = self.config.save() {
            self.error = Some(format!("配置保存失败：{error}"));
            cx.notify();
            return;
        }

        self.name_input.update(cx, TextInput::clear);
        self.base_url_input.update(cx, TextInput::clear);
        self.key_input.update(cx, TextInput::clear);
        self.error = None;
        cx.notify();
    }

    fn select_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        self.mode_open = false;
        if select_permission_mode(&mut self.config, mode).is_ok() {
            // 改动即保存。
            if let Err(error) = self.config.save() {
                self.error = Some(format!("配置保存失败：{error}"));
            }
        }
        cx.notify();
    }

    fn select_model(&mut self, model: &str, cx: &mut Context<Self>) {
        self.model_open = false;
        set_default_model(&mut self.config, model);
        // 改动即保存。
        if let Err(error) = self.config.save() {
            self.error = Some(format!("配置保存失败：{error}"));
        }
        cx.notify();
    }

    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_back))
                    .child("返回"),
            )
            .child(
                div()
                    .text_size(px(Typography::HEADING_PAGE))
                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                    .child("设置"),
            )
            .into_any_element()
    }

    fn render_provider_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("Provider", colors.text_primary))
            .children(self.config.providers.is_empty().then(|| {
                div()
                    .text_color(colors.text_tertiary)
                    .text_size(px(Typography::BODY))
                    .child("暂无 Provider，使用下方表单添加")
                    .into_any_element()
            }))
            .children(self.config.providers.iter().map(|provider| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(Typography::HEADING_CARD))
                                    .font_weight(Typography::HEADING_CARD_WEIGHT)
                                    .child(provider.name.clone()),
                            )
                            // 凡 key_ref 非空即显示已存储占位，永不回显 key 值。
                            .children((!provider.key_ref.is_empty()).then(|| {
                                div()
                                    .text_color(colors.text_secondary)
                                    .text_size(px(Typography::BODY))
                                    .child(KEY_STORED_PLACEHOLDER)
                            })),
                    )
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .text_size(px(Typography::BODY))
                            .child(provider.base_url.clone()),
                    )
                    .children((!provider.models.is_empty()).then(|| {
                        div()
                            .text_color(colors.text_tertiary)
                            .text_size(px(Typography::BODY))
                            .child(provider.models.join(", "))
                    }))
            }))
            .into_any_element()
    }

    fn render_add_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        // 渲染期读取输入内容：既驱动提交按钮的有效态，也让每次键入
        // 重新渲染本视图（GPUI 的渲染期读取即依赖注册）。
        let name = self.name_input.read(cx).text().to_string();
        let base_url = self.base_url_input.read(cx).text().to_string();
        let key = self.key_input.read(cx).text().to_string();
        let submittable = form_is_submittable(&name, &base_url, &key);

        let (button_bg, button_text) = if submittable {
            (colors.accent, colors.bg_base)
        } else {
            (colors.bg_hover, colors.text_tertiary)
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("添加 Provider", colors.text_primary))
            .child(self.name_input.clone())
            .child(self.base_url_input.clone())
            .child(self.key_input.clone())
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .self_start()
                    .bg(button_bg)
                    .text_color(button_text)
                    .text_size(px(Typography::SIDEBAR))
                    .when(submittable, |button| {
                        button
                            .cursor_pointer()
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_submit))
                    }),
            )
            .into_any_element()
    }

    fn render_defaults(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_title("默认项", colors.text_primary))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(field_label("权限模式", colors.text_secondary))
                    .child(self.render_mode_selector(cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(field_label("模型", colors.text_secondary))
                    .child(self.render_model_selector(cx)),
            )
            .into_any_element()
    }

    /// In-place expandable picker for the permission mode (minimal equivalent
    /// of a dropdown: click to expand, click an option to choose and collapse).
    fn render_mode_selector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let current = self.config.defaults.permission_mode.clone();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::BODY))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.mode_open = !this.mode_open;
                            cx.notify();
                        }),
                    )
                    .child(current.clone())
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .child(if self.mode_open { "▾" } else { "▸" }),
                    ),
            )
            .when(self.mode_open, |column| {
                column.children(PERMISSION_MODES.iter().map(|mode| {
                    let selected = *mode == current;
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_size(px(Typography::BODY))
                        .cursor_pointer()
                        .when(selected, move |row| row.bg(colors.bg_active))
                        .when(!selected, move |row| {
                            row.hover(move |s| s.bg(colors.bg_hover))
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                this.select_mode(mode, cx);
                            }),
                        )
                        .child(*mode)
                }))
            })
            .into_any_element()
    }

    /// In-place expandable picker for the default model; options are the
    /// union of all providers' models, with an empty-state hint when none.
    fn render_model_selector(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let models = all_models(&self.config.providers);
        let current = self.config.defaults.model.clone();
        let trigger_label = if current.is_empty() {
            "未选择".to_string()
        } else {
            current.clone()
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::BODY))
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            this.model_open = !this.model_open;
                            cx.notify();
                        }),
                    )
                    .child(trigger_label)
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .child(if self.model_open { "▾" } else { "▸" }),
                    ),
            )
            .when(self.model_open, |column| {
                column
                    .children(models.is_empty().then(|| {
                        div()
                            .text_color(colors.text_tertiary)
                            .text_size(px(Typography::BODY))
                            .child("暂无可选模型，先在下方添加 Provider")
                    }))
                    .children(models.iter().map(|model| {
                        let model = model.clone();
                        let selected = model == current;
                        let row_model = model.clone();
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(Typography::BODY))
                            .cursor_pointer()
                            .when(selected, move |row| row.bg(colors.bg_active))
                            .when(!selected, move |row| {
                                row.hover(move |s| s.bg(colors.bg_hover))
                            })
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                    this.select_model(&row_model, cx);
                                }),
                            )
                            .child(model)
                    }))
            })
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("settings-page")
            .key_context("PricingSettings")
            .on_action(cx.listener(Self::activate_pricing_action))
            .on_action(cx.listener(Self::next_pricing_action))
            .on_action(cx.listener(Self::previous_pricing_action))
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .text_size(px(Typography::BODY))
            .line_height(relative(Typography::BODY_LINE_HEIGHT))
            .overflow_y_scroll()
            .child(
                // Content column per UI spec §1: max 820px, centered, ≥24px
                // side padding.
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w_full()
                    .max_w(px(820.))
                    .mx_auto()
                    .px(px(24.))
                    .py(px(24.))
                    .child(self.render_header(cx))
                    .children(self.error.clone().map(|message| {
                        div()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(colors.bg_elevated)
                            .border_1()
                            .border_color(colors.danger)
                            .text_color(colors.danger)
                            .text_size(px(Typography::BODY))
                            .child(message)
                    }))
                    .child(self.render_provider_list(cx))
                    .child(self.render_add_form(cx))
                    .child(self.render_defaults(cx))
                    .child(self.render_pricing(cx)),
            )
    }
}

fn read_rate_inputs(inputs: &[Entity<TextInput>], cx: &App) -> PricingRateInputs {
    PricingRateInputs {
        input_usd_per_million: inputs[0].read(cx).text().to_string(),
        output_usd_per_million: inputs[1].read(cx).text().to_string(),
        cache_read_usd_per_million: inputs[2].read(cx).text().to_string(),
        cache_write_usd_per_million: inputs[3].read(cx).text().to_string(),
    }
}

fn rate_values(rates: &PricingRateInputs) -> [&str; 4] {
    [
        &rates.input_usd_per_million,
        &rates.output_usd_per_million,
        &rates.cache_read_usd_per_million,
        &rates.cache_write_usd_per_million,
    ]
}

fn pricing_mutation_input_bytes(mutation: &PricingMutation) -> Option<usize> {
    fn add_rates(total: usize, rates: &PricingRateInputs) -> Option<usize> {
        rate_values(rates)
            .into_iter()
            .try_fold(total, |total, value| total.checked_add(value.len()))
    }
    match mutation {
        PricingMutation::AddCustom { model, rates }
        | PricingMutation::UpdateCustom { model, rates }
        | PricingMutation::UpdateBuiltinBase { model, rates } => add_rates(model.len(), rates),
        PricingMutation::UpdateDeepSeek { model, base, peak } => {
            add_rates(add_rates(model.len(), base)?, peak)
        }
        PricingMutation::ResetBuiltin { model } | PricingMutation::DeleteCustom { model } => {
            Some(model.len())
        }
    }
}

fn pricing_status(label: &'static str, color: gpui::Rgba) -> Div {
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(color)
        .text_color(color)
        .child(label)
}

fn action_button(
    label: &'static str,
    colors: vega_theme::ThemeColors,
    focus: Option<FocusHandle>,
    listener: impl Fn(&MouseUpEvent, &mut Window, &mut gpui::App) + 'static,
) -> Div {
    let enabled = focus.is_some();
    div()
        .when_some(focus, |button, focus| button.track_focus(&focus))
        .px_2()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.bg_elevated)
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(colors.bg_hover))
                .on_mouse_up(MouseButton::Left, listener)
        })
        .when(!enabled, |button| button.text_color(colors.text_tertiary))
        .child(label)
}

fn pricing_error_label(code: PricingSettingsErrorCode) -> &'static str {
    match code {
        PricingSettingsErrorCode::Io => "定价文件读写失败，请检查数据目录后重新加载",
        PricingSettingsErrorCode::MalformedCatalog => {
            "定价文件已损坏；原文件已保留，请外部修复后重新加载"
        }
        PricingSettingsErrorCode::LockedProfile => "内置模型结构不完整或 metadata 被修改",
        PricingSettingsErrorCode::InvalidInput => "模型 ID 或价格格式无效",
        PricingSettingsErrorCode::ModelNotPriced => "当前模型未配置价格，请先添加后重试",
        PricingSettingsErrorCode::TargetChanged => "定价文件在保存期间变化，请重新加载",
        PricingSettingsErrorCode::RecoveryRequired => "保存结果无法确认，请重新加载恢复",
        PricingSettingsErrorCode::Busy => "已有定价操作正在进行",
        PricingSettingsErrorCode::LimitExceeded => "定价文件或条目超过安全上限",
    }
}

fn pricing_notice_label(notice: PricingNotice) -> &'static str {
    match notice {
        PricingNotice::DurabilityUnknownReconciled => "保存已复验，但目录 durability 曾无法确认",
        PricingNotice::ExternalWinnerAdopted => "保存期间文件发生变化，已采用外部有效版本",
    }
}

fn section_title(label: &'static str, color: gpui::Rgba) -> Div {
    div()
        .text_size(px(Typography::HEADING_BLOCK))
        .font_weight(Typography::HEADING_BLOCK_WEIGHT)
        .text_color(color)
        .child(label)
}

fn field_label(label: &'static str, color: gpui::Rgba) -> Div {
    div()
        .w(px(72.))
        .text_color(color)
        .text_size(px(Typography::BODY))
        .child(label)
}

/// Whether the add-provider form may be submitted: name, base_url, and key
/// must all be non-empty (name/base_url trimmed). Empty fields keep the
/// submit button inert (ui-spec §4.6: no error modal).
fn form_is_submittable(name: &str, base_url: &str, key: &str) -> bool {
    !name.trim().is_empty() && !base_url.trim().is_empty() && !key.is_empty()
}

/// Inserts `entry` into `providers`, appending when no provider with the same
/// name exists and replacing the existing one otherwise (the form does not
/// edit models, so a replacement keeps the stored models). Returns whether an
/// existing entry was replaced.
fn upsert_provider(providers: &mut Vec<ProviderConfig>, entry: ProviderConfig) -> bool {
    if let Some(existing) = providers
        .iter_mut()
        .find(|provider| provider.name == entry.name)
    {
        let models = std::mem::take(&mut existing.models);
        *existing = entry;
        existing.models = models;
        true
    } else {
        providers.push(entry);
        false
    }
}

/// Applies a permission-mode choice, rejecting values outside the fixed set.
fn select_permission_mode(config: &mut AppConfig, mode: &str) -> Result<(), &'static str> {
    if !PERMISSION_MODES.contains(&mode) {
        return Err("unknown permission mode");
    }
    config.defaults.permission_mode = mode.to_string();
    Ok(())
}

/// Sets the default model for new conversations.
fn set_default_model(config: &mut AppConfig, model: &str) {
    config.defaults.model = model.to_string();
}

/// Union of every provider's models in first-seen order, deduplicated.
fn all_models(providers: &[ProviderConfig]) -> Vec<String> {
    let mut models = Vec::new();
    for provider in providers {
        for model in &provider.models {
            if !models.contains(model) {
                models.push(model.clone());
            }
        }
    }
    models
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use gpui::{
        Bounds, KeyBinding, Render, TestAppContext, WindowBounds, WindowHandle, WindowOptions, size,
    };

    use super::*;

    struct SettingsHarness {
        view: Entity<SettingsView>,
        closes: Arc<AtomicUsize>,
    }

    impl Render for SettingsHarness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .on_action(cx.listener(|this, _: &CloseSettings, _, _| {
                    this.closes.fetch_add(1, Ordering::SeqCst);
                }))
                .child(self.view.clone())
        }
    }

    fn provider(name: &str, models: &[&str]) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            base_url: format!("https://{name}.example.com"),
            models: models.iter().map(|m| m.to_string()).collect(),
            key_ref: name.to_string(),
        }
    }

    #[test]
    fn form_rejects_empty_fields() {
        assert!(!form_is_submittable("", "https://x", "k"));
        assert!(!form_is_submittable("n", "", "k"));
        assert!(!form_is_submittable("n", "https://x", ""));
        // 空白 name 视为空。
        assert!(!form_is_submittable("   ", "https://x", "k"));
        assert!(form_is_submittable("n", "https://x", "k"));
    }

    #[test]
    fn upsert_appends_new_and_updates_same_name() {
        let mut providers = vec![provider("deepseek", &["deepseek-chat"])];
        // 异名追加。
        assert!(!upsert_provider(&mut providers, provider("openai", &[])));
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[1].name, "openai");
        // 同名更新：表单字段刷新，models 保留。
        let mut replacement = provider("deepseek", &[]);
        replacement.base_url = "https://api.deepseek.com/v1".to_string();
        assert!(upsert_provider(&mut providers, replacement));
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0].base_url, "https://api.deepseek.com/v1");
        assert_eq!(providers[0].models, vec!["deepseek-chat"]);
    }

    #[test]
    fn permission_mode_accepts_only_the_fixed_set() {
        let mut config = AppConfig::default();
        for mode in PERMISSION_MODES {
            select_permission_mode(&mut config, mode).unwrap();
            assert_eq!(config.defaults.permission_mode, mode);
        }
        assert!(select_permission_mode(&mut config, "yolo").is_err());
        assert_eq!(config.defaults.permission_mode, "auto");
    }

    #[test]
    fn default_model_changes_are_applied() {
        let mut config = AppConfig::default();
        assert!(config.defaults.model.is_empty());
        set_default_model(&mut config, "deepseek-chat");
        assert_eq!(config.defaults.model, "deepseek-chat");
    }

    #[test]
    fn all_models_unions_and_dedups_providers() {
        let providers = vec![
            provider("deepseek", &["deepseek-chat", "deepseek-reasoner"]),
            provider("openai", &["gpt", "deepseek-chat"]),
        ];
        assert_eq!(
            all_models(&providers),
            vec!["deepseek-chat", "deepseek-reasoner", "gpt"]
        );
        assert!(all_models(&[]).is_empty());
    }

    #[test]
    fn pricing_request_input_cap_is_checked_before_event_retention() {
        let rates = PricingRateInputs {
            input_usd_per_million: "0".repeat(PRICING_INPUT_BYTES_LIMIT - 4),
            output_usd_per_million: "0".to_string(),
            cache_read_usd_per_million: "0".to_string(),
            cache_write_usd_per_million: "0".to_string(),
        };
        let exact = PricingMutation::AddCustom {
            model: "m".to_string(),
            rates: rates.clone(),
        };
        assert_eq!(
            pricing_mutation_input_bytes(&exact),
            Some(PRICING_INPUT_BYTES_LIMIT)
        );
        let over = PricingMutation::AddCustom {
            model: "mm".to_string(),
            rates,
        };
        assert_eq!(
            pricing_mutation_input_bytes(&over),
            Some(PRICING_INPUT_BYTES_LIMIT + 1)
        );
    }

    #[gpui::test]
    async fn pricing_actions_are_tab_reachable_and_enter_space_activate_once(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            crate::init(cx);
            cx.bind_keys([KeyBinding::new(
                "escape",
                CloseSettings,
                Some("PricingSettings"),
            )]);
        });
        let long_cjk_model = format!("custom/{}", "模型-".repeat(20));
        let view = cx.new(SettingsView::new_for_test);
        view.update(cx, |view, cx| {
            view.apply_pricing_projection(
                PricingSettingsProjection::Ready {
                    generation: 7,
                    entries: vec![PricingEntryProjection {
                        model: long_cjk_model.clone(),
                        kind: PricingEntryKind::CustomStatic,
                        base: PricingRateInputs {
                            input_usd_per_million: "1".into(),
                            output_usd_per_million: "2".into(),
                            cache_read_usd_per_million: "3".into(),
                            cache_write_usd_per_million: "4".into(),
                        },
                        peak: None,
                    }],
                    notice: None,
                    draft_reason: None,
                    error: None,
                },
                cx,
            );
        });
        let events = Arc::new(Mutex::new(Vec::new()));
        let closes = Arc::new(AtomicUsize::new(0));
        let captured = events.clone();
        let root = view.clone();
        let projection_root = view.clone();
        let harness_closes = closes.clone();
        let window: WindowHandle<SettingsHarness> = cx
            .update(|cx| {
                let bounds = Bounds::centered(None, size(px(960.), px(600.)), cx);
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        ..Default::default()
                    },
                    move |_, cx| {
                        cx.new(|cx| {
                            cx.subscribe(
                                &root,
                                move |_, _, event: &PricingMutationRequested, cx| {
                                    let mut events =
                                        captured.lock().expect("pricing event capture");
                                    events.push((event.generation, event.mutation.is_ok()));
                                    let first = events.len() == 1;
                                    drop(events);
                                    if first {
                                        projection_root.update(cx, |view, cx| {
                                            view.apply_pricing_projection(
                                                PricingSettingsProjection::Saving {
                                                    generation: event.generation,
                                                    entries: Vec::new(),
                                                },
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )
                            .detach();
                            SettingsHarness {
                                view: root,
                                closes: harness_closes,
                            }
                        })
                    },
                )
            })
            .expect("settings window");
        cx.run_until_parked();
        assert_eq!(
            window
                .update(cx, |_, window, _| window.viewport_size())
                .expect("settings viewport"),
            size(px(960.), px(600.))
        );
        assert!(view.read_with(cx, |view, _| matches!(
            &view.pricing,
            PricingSettingsProjection::Ready { entries, .. }
                if entries.first().is_some_and(|entry| entry.model == long_cjk_model)
        )));
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::dark());
            cx.refresh_windows();
        });
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| cx.global::<vega_theme::Theme>().appearance),
            vega_theme::Appearance::Dark
        );
        window
            .update(cx, |_, window, cx| {
                let reload = view
                    .read(cx)
                    .pricing_focus(&PricingFocusTarget::Reload)
                    .expect("reload focus");
                reload.focus(window, cx);
            })
            .expect("focus reload");
        cx.simulate_keystrokes(window.into(), "tab");
        assert!(
            window
                .update(cx, |_, window, cx| {
                    view.read(cx)
                        .pricing_focus(&PricingFocusTarget::Add)
                        .is_some_and(|focus| focus.is_focused(window))
                })
                .expect("add focus")
        );
        cx.simulate_keystrokes(window.into(), "shift-tab");
        assert!(
            window
                .update(cx, |_, window, cx| {
                    view.read(cx)
                        .pricing_focus(&PricingFocusTarget::Reload)
                        .is_some_and(|focus| focus.is_focused(window))
                })
                .expect("reload focused after shift-tab")
        );
        cx.simulate_keystrokes(window.into(), "escape");
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        cx.simulate_keystrokes(window.into(), "tab");
        cx.simulate_keystrokes(window.into(), "enter");
        assert!(view.read_with(cx, |view, _| view.pricing_editor.is_some()));

        window
            .update(cx, |_, window, cx| {
                let save = view
                    .read(cx)
                    .pricing_focus(&PricingFocusTarget::Save)
                    .expect("save focus");
                save.focus(window, cx);
            })
            .expect("focus save");
        cx.simulate_keystrokes(window.into(), "space space");
        assert_eq!(*events.lock().expect("pricing events"), vec![(7, true)]);
    }
}
