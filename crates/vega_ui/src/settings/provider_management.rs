//! Provider selection and explicit, cancellable background operations.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;
use vega_conversation::ProviderSettingsService;
use vega_conversation::types::{
    ProviderNetworkAction, ProviderNetworkOutcome, ProviderNetworkRequest, ProviderPatchAction,
    ProviderPatchRequest,
};

#[derive(Default)]
pub(crate) struct ProviderManagement {
    pub(super) selected: Option<String>,
    generation: u64,
    operation: u64,
    cancel: Option<CancellationToken>,
    pub(super) saving: bool,
    pub(super) form: bool,
    pub(super) form_base: Option<ProviderConfig>,
    candidates: Option<Vec<String>>,
    checked: BTreeSet<String>,
    statuses: BTreeMap<String, String>,
    message: Option<String>,
    model_editor: Option<(Option<String>, Entity<TextInput>)>,
}
impl Drop for ProviderManagement {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}
#[derive(Clone)]
struct ProviderDrag(String);
impl Render for ProviderDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_2()
            .bg(theme(cx).colors.bg_elevated)
            .child(self.0.clone())
    }
}
#[derive(Clone)]
enum Command {
    Select(String),
    Add,
    Edit,
    Enable,
    Up,
    Down,
    Discover,
    Test(String),
    Cancel,
    Import,
    Candidate(String),
    AddModel,
    EditModel(String),
    DeleteModel(String),
    SaveModel,
    Reload,
}

impl SettingsView {
    pub(crate) fn provider_escape(
        &mut self,
        _: &CloseSettings,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section == 0
            && (self.provider_management.form
                || self.provider_management.model_editor.is_some()
                || self.provider_management.candidates.is_some()
                || self.provider_management.cancel.is_some())
        {
            self.provider_command(Command::Cancel, cx);
            cx.stop_propagation();
        } else {
            self.cancel_provider_operation(cx);
            cx.propagate();
        }
    }
    pub(crate) fn cancel_provider_operation(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.provider_management;
        if let Some(cancel) = state.cancel.take() {
            cancel.cancel();
        }
        state.generation = state.generation.saturating_add(1);
        state.message = None;
        state.candidates = None;
        state.checked.clear();
        state.statuses.retain(|_, value| value != "正在测试…");
        cx.notify();
    }
    fn selected_provider(&self) -> Option<ProviderConfig> {
        self.config
            .providers
            .iter()
            .find(|p| Some(&p.name) == self.provider_management.selected.as_ref())
            .cloned()
    }
    fn provider_command(&mut self, command: Command, cx: &mut Context<Self>) {
        if self.provider_management.saving {
            return;
        }
        match command {
            Command::Select(name) => {
                self.cancel_provider_operation(cx);
                self.provider_management.selected = Some(name);
                self.provider_management.form = false;
                self.provider_management.model_editor = None;
                self.provider_management.statuses.clear();
                self.provider_management.message = None;
            }
            Command::Add => {
                self.cancel_provider_operation(cx);
                self.provider_management.form = true;
                self.provider_management.form_base = None;
                self.name_input.update(cx, TextInput::clear);
                self.base_url_input.update(cx, TextInput::clear);
                self.models_input.update(cx, TextInput::clear);
                self.key_input.update(cx, TextInput::clear);
            }
            Command::Edit => {
                self.cancel_provider_operation(cx);
                if let Some(p) = self.selected_provider() {
                    self.begin_edit_provider(&p.name, cx);
                    self.provider_management.form = true;
                }
            }
            Command::Enable => {
                if let Some(p) = self.selected_provider() {
                    self.provider_patch(ProviderPatchAction::SetEnabled(!p.enabled), cx);
                }
            }
            Command::Up | Command::Down => {
                if let Some(index) = self
                    .config
                    .providers
                    .iter()
                    .position(|p| Some(&p.name) == self.provider_management.selected.as_ref())
                {
                    let target = if matches!(command, Command::Up) {
                        index.checked_sub(1)
                    } else {
                        index
                            .checked_add(1)
                            .filter(|i| *i < self.config.providers.len())
                    };
                    if let Some(target) = target {
                        self.move_provider(index, target, cx);
                    }
                }
            }
            Command::Discover => self.provider_network(ProviderNetworkAction::DiscoverModels, cx),
            Command::Test(model) => {
                self.provider_network(ProviderNetworkAction::TestModel { model }, cx)
            }
            Command::Cancel => {
                self.cancel_provider_operation(cx);
                self.provider_management.model_editor = None;
                self.provider_management.form = false;
            }
            Command::Candidate(model) => {
                if !self.provider_management.checked.remove(&model) {
                    self.provider_management.checked.insert(model);
                }
            }
            Command::Import => {
                let models = self
                    .provider_management
                    .candidates
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|m| self.provider_management.checked.contains(m))
                    .collect();
                self.provider_patch(ProviderPatchAction::ImportModels(models), cx);
            }
            Command::AddModel | Command::EditModel(_) => {
                let original = if let Command::EditModel(model) = command {
                    Some(model)
                } else {
                    None
                };
                let input = cx.new(|cx| TextInput::new(cx, "模型 ID", false).with_tab_stop(true));
                if let Some(model) = &original {
                    input.update(cx, |input, cx| input.set_text(model, cx));
                }
                self.provider_management.model_editor = Some((original, input));
            }
            Command::DeleteModel(model) => {
                if let Some(p) = self.selected_provider() {
                    self.provider_patch(
                        ProviderPatchAction::EditModels(
                            p.models.into_iter().filter(|m| m != &model).collect(),
                        ),
                        cx,
                    );
                }
            }
            Command::SaveModel => {
                if let Some((original, input)) = &self.provider_management.model_editor {
                    let value = input.read(cx).text().to_string();
                    if parse_provider_models(&value).is_err()
                        || value.trim().is_empty()
                        || value.lines().count() != 1
                    {
                        self.error = Some("请输入一个有效的模型 ID".into());
                    } else if let Some(mut p) = self.selected_provider() {
                        if p.models
                            .iter()
                            .any(|m| m == value.trim() && Some(m) != original.as_ref())
                        {
                            self.error = Some("模型 ID 已存在".into());
                        } else {
                            if let Some(index) =
                                p.models.iter().position(|m| Some(m) == original.as_ref())
                            {
                                p.models[index] = value.trim().into();
                            } else {
                                p.models.push(value.trim().into());
                            }
                            self.provider_patch(ProviderPatchAction::EditModels(p.models), cx);
                        }
                    }
                }
            }
            Command::Reload => self.reload_providers(cx),
        }
        cx.notify();
    }
    fn move_provider(&mut self, index: usize, target: usize, cx: &mut Context<Self>) {
        let expected_order: Vec<_> = self
            .config
            .providers
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut ordered_names = expected_order.clone();
        let name = ordered_names.remove(index);
        ordered_names.insert(target, name);
        self.provider_patch(
            ProviderPatchAction::Move {
                expected_order,
                ordered_names,
            },
            cx,
        );
    }
    pub(crate) fn save_provider_background(
        &mut self,
        base: Option<ProviderConfig>,
        provider: ProviderConfig,
        key: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.provider_management.saving {
            return;
        }
        let Some(path) = self.config_path.clone() else {
            self.error = Some("配置路径不可用".into());
            cx.notify();
            return;
        };
        self.cancel_provider_operation(cx);
        self.provider_management.saving = true;
        let generation = self.provider_management.generation;
        let name = provider.name.clone();
        let draft = [
            self.name_input.clone(),
            self.base_url_input.clone(),
            self.models_input.clone(),
            self.key_input.clone(),
        ]
        .map(|input| input.read(cx).text().to_string());
        let worker = cx.background_executor().spawn(async move {
            let config =
                ProviderSettingsService::new(path.clone()).save_provider(base, provider, key)?;
            let refs = path
                .parent()
                .and_then(|root| keystore::available_refs(root).ok())
                .unwrap_or_default();
            Ok::<_, vega_conversation::types::ProviderSettingsError>((config, refs))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                match result {
                    Ok((config, refs)) => {
                        this.config = config;
                        this.available_key_refs = refs;
                        cx.emit(SettingsSaved);
                        if generation != this.provider_management.generation {
                            cx.notify();
                            return;
                        }
                        this.provider_management.selected = Some(name);
                        let inputs = [
                            this.name_input.clone(),
                            this.base_url_input.clone(),
                            this.models_input.clone(),
                            this.key_input.clone(),
                        ];
                        if inputs
                            .iter()
                            .zip(&draft)
                            .all(|(input, text)| input.read(cx).text() == text)
                        {
                            this.provider_management.form = false;
                            this.provider_management.form_base = None;
                            for input in inputs {
                                input.update(cx, TextInput::clear);
                            }
                        }
                        this.provider_management.statuses.clear();
                        this.provider_management.message = None;
                        this.error = None;
                    }
                    Err(error) => {
                        if generation == this.provider_management.generation {
                            this.error = Some(format!("{error}；当前输入仍未保存"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn reload_providers(&mut self, cx: &mut Context<Self>) {
        self.cancel_provider_operation(cx);
        let Some(path) = self.config_path.clone() else {
            return;
        };
        self.provider_management.saving = true;
        let generation = self.provider_management.generation;
        let worker = cx
            .background_executor()
            .spawn(async move { ProviderSettingsService::new(path).load() });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                if generation != this.provider_management.generation {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(config) => {
                        this.config = config;
                        this.provider_management.statuses.clear();
                        this.provider_management.message = None;
                        this.error = None;
                    }
                    Err(error) => {
                        if generation == this.provider_management.generation {
                            this.error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn provider_patch(&mut self, action: ProviderPatchAction, cx: &mut Context<Self>) {
        let (Some(path), Some(provider)) = (self.config_path.clone(), self.selected_provider())
        else {
            return;
        };
        if self.provider_management.saving {
            return;
        }
        let preview = if matches!(&action, ProviderPatchAction::ImportModels(_)) {
            Some((
                self.provider_management.candidates.take(),
                std::mem::take(&mut self.provider_management.checked),
            ))
        } else {
            None
        };
        self.cancel_provider_operation(cx);
        if let Some((candidates, checked)) = preview {
            self.provider_management.candidates = candidates;
            self.provider_management.checked = checked;
        }
        self.provider_management.saving = true;
        let generation = self.provider_management.generation;
        let worker = cx.background_executor().spawn(async move {
            ProviderSettingsService::new(path).patch(ProviderPatchRequest { provider, action })
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                match result {
                    Ok(config) => {
                        this.config = config;
                        cx.emit(SettingsSaved);
                        if generation != this.provider_management.generation {
                            cx.notify();
                            return;
                        }
                        this.provider_management.model_editor = None;
                        this.provider_management.candidates = None;
                        this.provider_management.checked.clear();
                        this.provider_management.statuses.clear();
                        this.provider_management.message = None;
                        this.error = None;
                    }
                    Err(error) => {
                        if generation == this.provider_management.generation {
                            this.error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn provider_network(&mut self, action: ProviderNetworkAction, cx: &mut Context<Self>) {
        let (Some(path), Some(provider)) = (self.config_path.clone(), self.selected_provider())
        else {
            return;
        };
        if self.provider_management.cancel.is_some()
            || self.provider_management.saving
            || !provider.enabled
        {
            return;
        }
        let state = &mut self.provider_management;
        state.operation = state.operation.saturating_add(1);
        let request = ProviderNetworkRequest {
            operation_id: state.operation,
            generation: state.generation,
            provider,
            action,
        };
        if let ProviderNetworkAction::TestModel { model } = &request.action {
            state.statuses.insert(model.clone(), "正在测试…".into());
        }
        state.message = Some("正在连接…".into());
        let cancel = CancellationToken::new();
        state.cancel = Some(cancel.clone());
        let worker = cx.background_executor().spawn(async move {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => {
                    runtime.block_on(ProviderSettingsService::new(path).network(request, cancel))
                }
                Err(_) => vega_conversation::types::ProviderNetworkResult {
                    request,
                    outcome: Err(vega_conversation::types::ProviderSettingsError::Config),
                },
            }
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if result.request.generation != this.provider_management.generation
                    || result.request.operation_id != this.provider_management.operation
                    || this.selected_provider().as_ref() != Some(&result.request.provider)
                    || this.provider_management.cancel.is_none()
                {
                    return;
                }
                this.provider_management.cancel = None;
                let label = match &result.outcome {
                    Ok(ProviderNetworkOutcome::Models(models)) => {
                        this.provider_management.candidates = Some(models.clone());
                        format!("读取到 {} 个模型；请选择后添加", models.len())
                    }
                    Ok(ProviderNetworkOutcome::ModelTestSucceeded) => "连接成功".into(),
                    Err(error) => error.to_string(),
                };
                if let ProviderNetworkAction::TestModel { model } = result.request.action {
                    this.provider_management
                        .statuses
                        .insert(model, label.clone());
                }
                this.provider_management.message = Some(label);
                cx.notify();
            });
        })
        .detach();
    }
    fn provider_button(
        &self,
        id: String,
        label: &'static str,
        command: Command,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let key_command = command.clone();
        let selector = id.clone();
        div()
            .id(id)
            .debug_selector(move || selector.clone())
            .key_context("ProviderControl")
            .focusable()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .text_color(if enabled {
                colors.text_secondary
            } else {
                colors.text_tertiary
            })
            .focus_visible(move |s| s.bg(colors.bg_active))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.provider_command(command.clone(), cx)
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.provider_command(key_command.clone(), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
            })
            .child(label)
            .into_any_element()
    }
    pub(crate) fn render_provider_management(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.sync_provider_edit_focuses(cx);
        if self.provider_management.selected.is_none() {
            self.provider_management.selected =
                self.config.providers.first().map(|p| p.name.clone());
            self.provider_management.form = self.config.providers.is_empty();
        }
        let colors = theme(cx).colors;
        let busy = self.provider_management.saving;
        let network_busy = self.provider_management.cancel.is_some();
        let mut list = div()
            .id("provider-list")
            .h_full()
            .w(px(Layout::PROVIDER_LIST_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll();
        for (index, p) in self.config.providers.clone().into_iter().enumerate() {
            let selected = self.provider_management.selected.as_ref() == Some(&p.name);
            let name = p.name.clone();
            let key_name = name.clone();
            list = list.child(
                div()
                    .id(("provider-select", index))
                    .track_focus(&self.provider_edit_focuses[index].1)
                    .tab_stop(true)
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .when(selected, |row| row.bg(colors.bg_active))
                    .cursor_pointer()
                    .focus_visible(move |s| s.bg(colors.bg_hover))
                    .on_drag(ProviderDrag(name.clone()), |drag, _, _, cx| {
                        cx.new(|_| drag.clone())
                    })
                    .on_drop(cx.listener(move |this, drag: &ProviderDrag, _, cx| {
                        if !this.provider_management.saving
                            && let Some(from) =
                                this.config.providers.iter().position(|p| p.name == drag.0)
                        {
                            this.provider_management.selected = Some(drag.0.clone());
                            this.move_provider(from, index, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.provider_command(Command::Select(name.clone()), cx)
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.provider_command(Command::Select(key_name.clone()), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(
                        div()
                            .text_color(if p.enabled {
                                colors.success
                            } else {
                                colors.text_tertiary
                            })
                            .child("●"),
                    )
                    .child(div().min_w_0().truncate().child(p.name)),
            );
        }
        list = list
            .child(self.provider_button(
                "provider-add".into(),
                "+ 添加供应商",
                Command::Add,
                !busy,
                cx,
            ))
            .child(self.provider_button(
                "provider-reload".into(),
                "重新读取",
                Command::Reload,
                !busy,
                cx,
            ));
        // Keep the flow intrinsically sized; the outer viewport owns scrolling.
        // A height-constrained flex column can shrink truncated text rows to zero.
        let mut detail = div()
            .debug_selector(|| "provider-detail-flow".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3();
        if self.provider_management.form {
            detail = detail
                .child(self.render_add_form(cx))
                .child(self.provider_button(
                    "provider-form-cancel".into(),
                    "取消",
                    Command::Cancel,
                    !busy,
                    cx,
                ));
        } else if let Some(p) = self.selected_provider() {
            let index = self
                .config
                .providers
                .iter()
                .position(|candidate| candidate.name == p.name)
                .unwrap_or(0);
            detail = detail
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(Typography::HEADING_CARD))
                                .child(p.name.clone()),
                        )
                        .child(self.provider_button(
                            "provider-enabled".into(),
                            if p.enabled { "已启用" } else { "已禁用" },
                            Command::Enable,
                            !busy,
                            cx,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.provider_button(
                            "provider-up".into(),
                            "上移",
                            Command::Up,
                            !busy && index > 0,
                            cx,
                        ))
                        .child(self.provider_button(
                            "provider-down".into(),
                            "下移",
                            Command::Down,
                            !busy && index + 1 < self.config.providers.len(),
                            cx,
                        ))
                        .child(self.provider_button(
                            "provider-edit".into(),
                            "编辑供应商",
                            Command::Edit,
                            !busy,
                            cx,
                        )),
                )
                .child(div().text_color(colors.text_secondary).child("Base URL"))
                .child(
                    div()
                        .debug_selector(|| "provider-base-url".into())
                        .min_w_0()
                        .truncate()
                        .child(p.base_url.clone()),
                )
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .child("API 格式 · Chat Completions"),
                )
                .child(div().text_color(colors.text_secondary).child(
                    if self.available_key_refs.contains(&p.key_ref) {
                        KEY_STORED_PLACEHOLDER
                    } else {
                        "需重新输入 API Key"
                    },
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.provider_button(
                            "provider-discover".into(),
                            "发现模型",
                            Command::Discover,
                            !busy && !network_busy && p.enabled,
                            cx,
                        ))
                        .child(self.provider_button(
                            "provider-add-model".into(),
                            "添加模型",
                            Command::AddModel,
                            !busy,
                            cx,
                        ))
                        .when(network_busy, |row| {
                            row.child(self.provider_button(
                                "provider-cancel".into(),
                                "停止",
                                Command::Cancel,
                                true,
                                cx,
                            ))
                        }),
                )
                .child(
                    div()
                        .text_color(colors.text_tertiary)
                        .child("测试会发送少量固定文本请求；不会使用聊天内容。"),
                );
            for (index, model) in p.models.iter().enumerate() {
                detail = detail.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .border_b_1()
                        .border_color(colors.border_subtle)
                        .py_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(div().flex_1().min_w_0().truncate().child(model.clone()))
                                .child(self.provider_button(
                                    format!("model-test-{index}"),
                                    "测试",
                                    Command::Test(model.clone()),
                                    !busy && !network_busy && p.enabled,
                                    cx,
                                ))
                                .child(self.provider_button(
                                    format!("model-edit-{index}"),
                                    "编辑",
                                    Command::EditModel(model.clone()),
                                    !busy,
                                    cx,
                                ))
                                .child(self.provider_button(
                                    format!("model-delete-{index}"),
                                    "删除",
                                    Command::DeleteModel(model.clone()),
                                    !busy,
                                    cx,
                                )),
                        )
                        .children(self.provider_management.statuses.get(model).map(|status| {
                            div()
                                .text_color(colors.text_secondary)
                                .child(status.clone())
                        })),
                );
            }
            if let Some((_, input)) = &self.provider_management.model_editor {
                detail = detail.child(input.clone()).child(
                    div()
                        .flex()
                        .gap_2()
                        .child(self.provider_button(
                            "model-save".into(),
                            "保存模型",
                            Command::SaveModel,
                            !busy,
                            cx,
                        ))
                        .child(self.provider_button(
                            "model-cancel".into(),
                            "取消",
                            Command::Cancel,
                            !busy,
                            cx,
                        )),
                );
            }
            if let Some(models) = self.provider_management.candidates.clone() {
                for (index, model) in models.into_iter().enumerate() {
                    let existing = p.models.contains(&model);
                    let checked = self.provider_management.checked.contains(&model);
                    detail = detail.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.provider_button(
                                format!("candidate-{index}"),
                                if existing {
                                    "已添加"
                                } else if checked {
                                    "☑"
                                } else {
                                    "☐"
                                },
                                Command::Candidate(model.clone()),
                                !existing && !busy,
                                cx,
                            ))
                            .child(div().min_w_0().truncate().child(model)),
                    );
                }
                detail = detail.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(self.provider_button(
                            "provider-import".into(),
                            "添加所选",
                            Command::Import,
                            !busy && !self.provider_management.checked.is_empty(),
                            cx,
                        ))
                        .child(self.provider_button(
                            "discovery-cancel".into(),
                            "取消",
                            Command::Cancel,
                            !busy,
                            cx,
                        )),
                );
            }
        }
        if let Some(message) = &self.provider_management.message {
            detail = detail.child(
                div()
                    .debug_selector(|| "provider-network-message".into())
                    .text_color(colors.text_secondary)
                    .child(message.clone()),
            );
        }
        div()
            .id("provider-management")
            .size_full()
            .min_h_0()
            .on_action(cx.listener(Self::provider_escape))
            .flex()
            .items_start()
            .min_w_0()
            .gap_4()
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter"
                        && this.provider_management.model_editor.as_ref().is_some_and(
                            |(_, input)| input.read(cx).focus_handle(cx).is_focused(window),
                        )
                    {
                        this.provider_command(Command::SaveModel, cx);
                        cx.stop_propagation();
                        return;
                    }
                    if event.keystroke.key == "escape"
                        && (this.provider_management.form
                            || this.provider_management.model_editor.is_some()
                            || this.provider_management.candidates.is_some()
                            || this.provider_management.cancel.is_some())
                    {
                        this.provider_command(Command::Cancel, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(list)
            .child(
                div()
                    .id("provider-detail")
                    .debug_selector(|| "provider-detail-viewport".into())
                    .h_full()
                    .min_h_0()
                    .min_w_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .child(detail),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
