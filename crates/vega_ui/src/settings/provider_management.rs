//! Provider selection and explicit, cancellable background operations.
use super::*;
use crate::interaction::InteractionExt;
use crate::scroll_edge_fade::{ScrollEdgeFadeExt, ScrollFade, ScrollableFadeExt};
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;
use vega_conversation::ProviderSettingsService;
use vega_conversation::types::{
    ProviderNetworkAction, ProviderNetworkOutcome, ProviderNetworkRequest, ProviderPatchAction,
    ProviderPatchRequest,
};
use vega_store::context_compaction::{DEFAULT_MODEL_INPUT_LIMIT, DEFAULT_MODEL_OUTPUT_RESERVE};

const PROVIDER_TEST_HELP_COPY: &str = "测试会发送少量固定文本请求；不会使用聊天内容。";

#[derive(Default)]
pub(crate) struct ProviderManagement {
    pub(super) selected: Option<String>,
    generation: u64,
    operation: u64,
    cancel: Option<CancellationToken>,
    pub(super) saving: bool,
    pub(super) form: bool,
    pub(super) form_api: vega_conversation::types::ProviderApi,
    pub(super) form_base: Option<ProviderConfig>,
    candidates: Option<Vec<String>>,
    checked: BTreeSet<String>,
    statuses: BTreeMap<String, String>,
    message: Option<String>,
    test_help_hovered: Option<usize>,
    test_help_pinned: Option<usize>,
    test_help_dismissed: Option<usize>,
    menu: Option<String>,
    menu_focuses: BTreeMap<String, FocusHandle>,
    menu_return_focus: Option<FocusHandle>,
    delete_model: Option<(String, String)>,
    detail_open: bool,
    list_scroll: gpui_kit::ScrollHandle,
    detail_scroll: gpui_kit::ScrollHandle,
    model_editor: Option<ModelEditor>,
    next_model_context_request: u64,
    pending_model_reopen: Option<(String, Option<String>)>,
    pending_command: Option<Command>,
    return_focus: Option<super::motion::ReturnFocus>,
    model_focuses: BTreeMap<(String, String), FocusHandle>,
    action_focuses: BTreeMap<String, FocusHandle>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolicySource {
    Loading,
    AssumedDefault,
    Saved,
    Unknown,
}

struct ModelEditor {
    original: Option<String>,
    model_input: Entity<TextInput>,
    input_limit: Entity<TextInput>,
    output_limit: Entity<TextInput>,
    automatic: bool,
    unknown: bool,
    source: PolicySource,
    legacy_present: bool,
    request_id: u64,
    saving: bool,
    loaded_values: Option<(Option<u64>, Option<u64>, bool)>,
    message: Option<String>,
    renamed_from: Option<String>,
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
    DeleteModel(String, String),
    SaveModel,
    ToggleModelAutomatic,
    ToggleModelUnknown,
    Reload,
    Menu(String),
    ConfirmDelete(String),
    BackToList,
    PriceLibrary,
}

impl SettingsView {
    pub(super) fn prepare_provider_presentation(&mut self, window: &Window) {
        if self.section != 0 || self.provider_management.model_editor.is_some() {
            return;
        }
        if self.provider_management.selected.is_none()
            || (!self.provider_management.form && self.selected_provider().is_none())
        {
            self.provider_management.selected = self
                .config
                .providers
                .first()
                .map(|provider| provider.name.clone());
            self.provider_management.form = self.config.providers.is_empty();
        }
        if !super::motion::narrow(window) {
            self.provider_management.detail_open = true;
        }
    }

    pub(super) fn settings_motion_destination(&self) -> super::motion::Destination {
        use super::motion::Destination;
        if !matches!(self.section, 0 | 2 | 3) {
            return Destination::Other(self.section);
        }
        if let Some((provider, model)) = &self.model_settings {
            return Destination::Model(provider.clone(), model.clone(), self.section);
        }
        if self.section != 0 {
            return Destination::Other(self.section);
        }
        if self.provider_management.form || self.provider_management.model_editor.is_some() {
            return Destination::ProviderForm(self.provider_management.selected.clone());
        }
        if self.provider_management.detail_open
            && let Some(provider) = self.selected_provider()
        {
            return Destination::Provider(provider.name);
        }
        Destination::ProviderList
    }

    pub(super) fn commit_settings_motion(&mut self, before: super::motion::Destination) {
        let after = self.settings_motion_destination();
        self.settings_motion.committed(before, after);
    }

    pub(super) fn remember_model_return_focus(&mut self) {
        if let Some((provider, model)) = self.model_settings.clone() {
            self.provider_management.detail_open = true;
            self.provider_management.return_focus =
                Some(super::motion::ReturnFocus::Model { provider, model });
        }
    }

    fn restore_provider_return_focus(
        &mut self,
        show_detail: bool,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.provider_management.return_focus.take() else {
            return;
        };
        let list_visible = !narrow || !show_detail;
        let provider_visible =
            show_detail && !self.provider_management.form && self.selected_provider().is_some();
        let fallback = if provider_visible {
            self.provider_management
                .action_focuses
                .get("provider-edit")
                .cloned()
        } else if list_visible {
            self.provider_management
                .action_focuses
                .get("provider-add")
                .cloned()
        } else if self.provider_management.form {
            Some(self.name_input.read(cx).focus_handle(cx))
        } else {
            Some(self.section_focuses[0].clone())
        };
        let focus = match target {
            super::motion::ReturnFocus::ProviderList { provider, index } if list_visible => self
                .provider_edit_focuses
                .iter()
                .find(|(name, _)| Some(name) == provider.as_ref())
                .or_else(|| {
                    self.provider_edit_focuses
                        .get(index.min(self.provider_edit_focuses.len().saturating_sub(1)))
                })
                .map(|(_, focus)| focus.clone())
                .or_else(|| {
                    self.provider_management
                        .action_focuses
                        .get("provider-add")
                        .cloned()
                }),
            super::motion::ReturnFocus::Model { provider, model }
                if provider_visible
                    && self.provider_management.selected.as_ref() == Some(&provider) =>
            {
                self.provider_management
                    .model_focuses
                    .get(&(provider, model))
                    .cloned()
            }
            _ => None,
        }
        .or(fallback);
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
    }

    fn provider_test_help_control(
        &self,
        index: usize,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let selector = if index == 0 {
            "provider-test-help-discover".to_string()
        } else {
            format!("provider-test-help-model-{}", index - 1)
        };
        let pointer_focus = focus.clone();
        let keyboard_focus = focus.clone();
        div()
            .id(selector.clone())
            .interactive(colors, true, false)
            .role(gpui_kit::Role::Button)
            .aria_label("测试请求说明")
            .aria_description(PROVIDER_TEST_HELP_COPY)
            .focusable()
            .track_focus(&focus)
            .tab_stop(true)
            .size(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .flex_shrink_0()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .focus_visible(move |style| style.border_color(colors.accent))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.provider_management.test_help_hovered = Some(index);
                    this.provider_management.test_help_dismissed = None;
                } else if this.provider_management.test_help_hovered == Some(index) {
                    this.provider_management.test_help_hovered = None;
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    let closing = this.provider_management.test_help_pinned == Some(index);
                    this.provider_management.test_help_pinned = (!closing).then_some(index);
                    this.provider_management.test_help_dismissed = closing.then_some(index);
                    window.focus(&pointer_focus, cx);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space" | "escape") {
                        let closing = event.keystroke.key == "escape"
                            || this.provider_management.test_help_pinned == Some(index);
                        this.provider_management.test_help_pinned = (!closing).then_some(index);
                        this.provider_management.test_help_dismissed = closing.then_some(index);
                        window.focus(&keyboard_focus, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .child(crate::icons::icon(
                crate::icons::Icon::Help,
                colors.text_secondary,
            ))
            .into_any_element()
    }

    fn provider_test_help_tooltip(
        &self,
        index: usize,
        colors: vega_theme::ThemeColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selector = if index == 0 {
            "provider-test-help-discover-tooltip".to_string()
        } else {
            format!("provider-test-help-model-{}-tooltip", index - 1)
        };
        let body_selector = format!("{selector}-body");
        let body = div()
            .id(body_selector.clone())
            .w_full()
            .max_w_full()
            .min_w_0()
            .whitespace_normal()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .text_size(px(Typography::METADATA))
            .text_color(colors.text_primary)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_normal()
                    .child(PROVIDER_TEST_HELP_COPY),
            );
        let popup = div()
            .absolute()
            .bottom(relative(1.0))
            .w(px(224.))
            .max_w_full()
            .when(index == 0, |popup| popup.left_0())
            .when(index > 0, |popup| popup.right_0())
            .on_mouse_down_out(
                cx.listener(move |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                    if this.provider_management.test_help_pinned == Some(index) {
                        this.provider_management.test_help_pinned = None;
                        this.provider_management.test_help_dismissed = Some(index);
                        cx.notify();
                    }
                }),
            )
            .child(body);
        gpui_kit::deferred(popup)
            .with_priority(2)
            .into_any_element()
    }

    pub(crate) fn provider_escape(
        &mut self,
        _: &CloseSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.provider_management.test_help_pinned.take() {
            self.provider_management.test_help_dismissed = Some(index);
            if let Some(focus) = self.provider_test_help_focuses.get(index) {
                window.focus(focus, cx);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.provider_management.menu.take().is_some() {
            if let Some(focus) = &self.provider_management.menu_return_focus {
                window.focus(focus, cx);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.section == 0
            && (self.provider_management.form
                || self.provider_management.model_editor.is_some()
                || self.provider_management.candidates.is_some()
                || self.provider_management.cancel.is_some())
        {
            if self.provider_has_draft(cx) {
                self.request_settings_section(0, cx);
            } else {
                self.provider_command(Command::Cancel, cx);
            }
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
    pub fn open_model_settings(
        &mut self,
        provider: &str,
        model: &str,
        section: usize,
        cx: &mut Context<Self>,
    ) {
        if self.config.providers.iter().any(|candidate| {
            candidate.name == provider
                && candidate.models.iter().any(|candidate| candidate == model)
        }) {
            self.provider_command(Command::Select(provider.into()), cx);
            self.provider_command(Command::EditModel(model.into()), cx);
        } else {
            self.model_settings = Some((provider.into(), model.into()));
        }
        self.section = section;
        self.rebuild_pricing_focuses(cx);
        self.rebuild_reasoning_focuses(cx);
        self.rebuild_model_capability_focuses(cx);
        cx.notify();
    }

    pub(crate) fn provider_is_saving(&self) -> bool {
        self.provider_management.saving
            || self
                .provider_management
                .model_editor
                .as_ref()
                .is_some_and(|editor| editor.saving)
    }

    pub(crate) fn provider_has_draft(&self, cx: &App) -> bool {
        self.provider_is_saving()
            || (self.provider_management.form && self.provider_form_changed(cx))
            || self
                .provider_management
                .model_editor
                .as_ref()
                .is_some_and(|editor| {
                    editor.model_input.read(cx).text().trim()
                        != editor.original.as_deref().unwrap_or_default()
                        || self.model_context_values_changed(cx)
                })
    }

    fn provider_form_changed(&self, cx: &App) -> bool {
        let name = self.name_input.read(cx).text();
        let url = self.base_url_input.read(cx).text();
        let models = self.models_input.read(cx).text();
        let key = self.key_input.read(cx).text();
        if let Some(base) = &self.provider_management.form_base {
            name.trim() != base.name
                || url.trim() != base.base_url
                || models.trim() != base.models.join("\n")
                || !key.is_empty()
                || self.provider_management.form_api != base.api
        } else {
            !name.is_empty() || !url.is_empty() || !models.is_empty() || !key.is_empty()
        }
    }

    pub(crate) fn finish_provider_navigation(&mut self, cx: &mut Context<Self>) {
        if let Some(command) = self.provider_management.pending_command.take() {
            self.provider_management.form = false;
            self.provider_management.model_editor = None;
            self.provider_command(command, cx);
        }
    }

    pub(crate) fn save_provider_draft(&mut self, cx: &mut Context<Self>) {
        if self.provider_management.form {
            self.submit_provider(cx);
        } else {
            self.provider_command(Command::SaveModel, cx);
        }
    }

    pub(crate) fn discard_provider_draft(&mut self, cx: &mut Context<Self>) {
        if let Some((_, model)) = self.model_settings.clone() {
            self.provider_command(Command::EditModel(model), cx);
        } else {
            self.provider_management.form = false;
        }
    }

    pub(crate) fn close_model_configuration(&mut self, cx: &mut Context<Self>) {
        self.provider_management.delete_model = None;
        self.model_settings = None;
        self.provider_management.model_editor = None;
        self.provider_management.form = false;
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
        if self.provider_is_saving() {
            return;
        }
        if matches!(
            command,
            Command::Select(_)
                | Command::Add
                | Command::Edit
                | Command::Reload
                | Command::PriceLibrary
                | Command::BackToList
        ) && self.provider_has_draft(cx)
        {
            self.provider_management.pending_command = Some(command);
            self.pending_section = Some(SettingsDestination::ProviderCommand);
            cx.notify();
            return;
        }
        if matches!(
            command,
            Command::Select(_)
                | Command::Add
                | Command::Edit
                | Command::Reload
                | Command::PriceLibrary
                | Command::BackToList
                | Command::AddModel
                | Command::EditModel(_)
        ) {
            self.provider_management.delete_model = None;
        }
        let before = self.settings_motion_destination();
        self.provider_management.return_focus = None;
        match command {
            Command::Menu(id) => {
                self.provider_management.menu =
                    if self.provider_management.menu.as_ref() == Some(&id) {
                        None
                    } else {
                        Some(id)
                    };
            }
            Command::ConfirmDelete(model) => {
                self.provider_management.menu = None;
                self.provider_management.delete_model = self
                    .selected_provider()
                    .filter(|provider| provider.models.contains(&model))
                    .map(|provider| (provider.name, model));
            }
            Command::BackToList => {
                let provider = self.provider_management.selected.clone();
                let index = self
                    .config
                    .providers
                    .iter()
                    .position(|candidate| Some(&candidate.name) == provider.as_ref())
                    .unwrap_or(0);
                self.provider_management.return_focus =
                    Some(super::motion::ReturnFocus::ProviderList { provider, index });
                self.provider_management.detail_open = false;
            }
            Command::PriceLibrary => {
                self.model_settings = None;
                self.request_settings_section(3, cx);
            }
            Command::Select(name) => {
                self.provider_management.delete_model = None;
                self.cancel_provider_operation(cx);
                self.provider_management.selected = Some(name);
                self.provider_management.detail_open = true;
                self.model_settings = None;
                self.provider_management.form = false;
                self.provider_management.model_editor = None;
                self.provider_management.statuses.clear();
                self.provider_management.message = None;
            }
            Command::Add => {
                self.cancel_provider_operation(cx);
                self.provider_management.form = true;
                self.provider_management.detail_open = true;
                self.provider_management.form_base = None;
                self.provider_management.form_api =
                    vega_conversation::types::ProviderApi::default();
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
                self.remember_model_return_focus();
                self.cancel_provider_operation(cx);
                self.provider_management.model_editor = None;
                self.provider_management.form = false;
                self.provider_management.delete_model = None;
                self.model_settings = None;
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
                self.model_settings = self
                    .selected_provider()
                    .and_then(|provider| original.clone().map(|model| (provider.name, model)));
                let input = cx.new(|cx| TextInput::new(cx, "模型 ID", false).with_tab_stop(true));
                if let Some(model) = &original {
                    input.update(cx, |input, cx| input.set_text(model, cx));
                }
                let input_limit =
                    cx.new(|cx| TextInput::new(cx, "输入最大 token 数", false).with_tab_stop(true));
                let output_limit =
                    cx.new(|cx| TextInput::new(cx, "输出最大 token 数", false).with_tab_stop(true));
                self.provider_management.next_model_context_request = self
                    .provider_management
                    .next_model_context_request
                    .saturating_add(1);
                let request_id = self.provider_management.next_model_context_request;
                self.provider_management.model_editor = Some(ModelEditor {
                    original: original.clone(),
                    model_input: input,
                    input_limit,
                    output_limit,
                    automatic: true,
                    unknown: false,
                    source: PolicySource::Loading,
                    legacy_present: false,
                    request_id,
                    saving: false,
                    loaded_values: None,
                    message: None,
                    renamed_from: None,
                });
                if let (Some(provider), Some(model)) = (self.selected_provider(), original) {
                    cx.emit(ModelContextLoadRequested {
                        request_id,
                        provider: provider.name,
                        model,
                    });
                }
            }
            Command::DeleteModel(provider, model) => {
                if self.provider_management.delete_model.as_ref()
                    != Some(&(provider.clone(), model.clone()))
                    || self.provider_management.selected.as_ref() != Some(&provider)
                {
                    return;
                }
                self.provider_management.delete_model = None;
                self.provider_management.menu = None;
                if let Some(p) = self
                    .selected_provider()
                    .filter(|p| p.models.contains(&model))
                {
                    self.provider_patch(
                        ProviderPatchAction::EditModels(
                            p.models.into_iter().filter(|m| m != &model).collect(),
                        ),
                        cx,
                    );
                }
            }
            Command::SaveModel => {
                if let Some(editor) = &self.provider_management.model_editor {
                    if editor.saving {
                        return;
                    }
                    let original = editor.original.clone();
                    let value = editor.model_input.read(cx).text().to_string();
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
                        } else if original.as_deref() == Some(value.trim()) {
                            self.save_model_context(cx);
                        } else {
                            // A config rename and a SQLite policy mutation
                            // have different authorities. Never silently
                            // apply edited limits to the old model identity.
                            if original.is_some() && self.model_context_values_changed(cx) {
                                self.error = Some(
                                    "请先保存模型 ID，再重新打开模型设置修改上下文容量".into(),
                                );
                                return;
                            }
                            if let Some(index) =
                                p.models.iter().position(|m| Some(m) == original.as_ref())
                            {
                                p.models[index] = value.trim().into();
                            } else {
                                p.models.push(value.trim().into());
                            }
                            self.provider_management.pending_model_reopen =
                                Some((value.trim().into(), original.clone()));
                            self.provider_patch(ProviderPatchAction::EditModels(p.models), cx);
                        }
                    }
                }
            }
            Command::ToggleModelAutomatic => {
                if let Some(editor) = self.provider_management.model_editor.as_mut()
                    && !editor.saving
                {
                    editor.automatic = !editor.automatic;
                }
            }
            Command::ToggleModelUnknown => {
                if let Some(editor) = self.provider_management.model_editor.as_mut()
                    && !editor.saving
                {
                    editor.unknown = !editor.unknown;
                }
            }
            Command::Reload => self.reload_providers(cx),
        }
        self.commit_settings_motion(before);
        cx.notify();
    }

    fn model_context_values_changed(&self, cx: &App) -> bool {
        let Some(editor) = self.provider_management.model_editor.as_ref() else {
            return false;
        };
        let current = if editor.unknown {
            Some((None, None, editor.automatic))
        } else {
            editor
                .input_limit
                .read(cx)
                .text()
                .trim()
                .parse::<u64>()
                .ok()
                .zip(
                    editor
                        .output_limit
                        .read(cx)
                        .text()
                        .trim()
                        .parse::<u64>()
                        .ok(),
                )
                .map(|(input, output)| (Some(input), Some(output), editor.automatic))
        };
        editor.loaded_values.as_ref() != current.as_ref()
    }

    fn save_model_context(&mut self, cx: &mut Context<Self>) {
        let Some(provider) = self.selected_provider() else {
            return;
        };
        let Some(editor) = self.provider_management.model_editor.as_mut() else {
            return;
        };
        let Some(model) = editor.original.clone() else {
            return;
        };
        if editor.source == PolicySource::Loading {
            self.error = Some("正在读取模型上下文配置，请稍后重试".into());
            return;
        }
        let (input_limit, output_limit) = if editor.unknown {
            (None, None)
        } else {
            let input = editor.input_limit.read(cx).text().trim().parse::<u64>();
            let output = editor.output_limit.read(cx).text().trim().parse::<u64>();
            match (input, output) {
                (Ok(input), Ok(output))
                    if input > 0
                        && output > 0
                        && input
                            .checked_add(output)
                            .is_some_and(|total| total <= u32::MAX as u64) =>
                {
                    (Some(input), Some(output))
                }
                _ => {
                    self.error =
                        Some("请输入正整数 token 数，且输入与输出之和不超过 4294967295".into());
                    return;
                }
            }
        };
        self.provider_management.next_model_context_request = self
            .provider_management
            .next_model_context_request
            .saturating_add(1);
        let request_id = self.provider_management.next_model_context_request;
        editor.request_id = request_id;
        editor.saving = true;
        editor.message = Some("正在保存模型上下文配置…".into());
        self.error = None;
        cx.emit(ModelContextSaveRequested {
            request_id,
            provider: provider.name,
            model,
            input_limit,
            output_limit,
            automatic_compaction: editor.automatic,
        });
    }

    /// Only an exact currently open editor accepts this background load.
    pub fn apply_model_context_loaded(
        &mut self,
        request: &ModelContextLoadRequested,
        result: Result<ModelContextLoaded, String>,
        cx: &mut Context<Self>,
    ) {
        if self.provider_management.selected.as_deref() != Some(&request.provider) {
            return;
        }
        let Some(editor) = self.provider_management.model_editor.as_mut() else {
            return;
        };
        if editor.request_id != request.request_id
            || editor.original.as_deref() != Some(&request.model)
            || editor.saving
        {
            return;
        }
        match result {
            Ok(loaded) => {
                let persisted = loaded.policy.is_some();
                let policy = loaded.policy.unwrap_or_else(|| {
                    ModelContextPolicy::assumed_default(&request.provider, &request.model)
                });
                editor.source = if policy.input_limit.is_none() {
                    PolicySource::Unknown
                } else if persisted {
                    PolicySource::Saved
                } else {
                    PolicySource::AssumedDefault
                };
                editor.unknown = policy.input_limit.is_none();
                editor.automatic = policy.automatic_compaction;
                editor.legacy_present = loaded.legacy_present;
                editor.loaded_values = Some((
                    policy.input_limit,
                    policy.output_reserve,
                    policy.automatic_compaction,
                ));
                editor.input_limit.update(cx, |input, cx| {
                    input.set_text(
                        &policy
                            .input_limit
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                        cx,
                    )
                });
                editor.output_limit.update(cx, |input, cx| {
                    input.set_text(
                        &policy
                            .output_reserve
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                        cx,
                    )
                });
                editor.message = None;
            }
            Err(error) => {
                editor.message = Some(error);
            }
        }
        cx.notify();
    }

    /// Save ACK is fenced by request id and exact provider/model identity.
    pub fn apply_model_context_saved(
        &mut self,
        request: &ModelContextSaveRequested,
        result: Result<ModelContextPolicy, String>,
        cx: &mut Context<Self>,
    ) {
        if self.provider_management.selected.as_deref() != Some(&request.provider) {
            return;
        }
        let Some(editor) = self.provider_management.model_editor.as_mut() else {
            return;
        };
        if editor.request_id != request.request_id
            || editor.original.as_deref() != Some(&request.model)
            || !editor.saving
        {
            return;
        }
        editor.saving = false;
        match result {
            Ok(policy) => {
                editor.source = if policy.input_limit.is_none() {
                    PolicySource::Unknown
                } else {
                    PolicySource::Saved
                };
                editor.loaded_values = Some((
                    policy.input_limit,
                    policy.output_reserve,
                    policy.automatic_compaction,
                ));
                editor.message = Some("已保存；后续请求生效，进行中的请求不受影响".into());
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.clone());
                editor.message = Some(error);
            }
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
        let config_before_save = self.config.clone();
        let worker = cx.background_executor().spawn(async move {
            let credential_changed = key.as_ref().is_some_and(|new_key| {
                let old_key = base.as_ref().and_then(|provider| {
                    path.parent()
                        .and_then(|root| keystore::get_key(root, &provider.key_ref).ok())
                });
                old_key.as_deref() != Some(new_key.as_str())
            });
            let config =
                ProviderSettingsService::new(path.clone()).save_provider(base, provider, key)?;
            let replay_identity_changed =
                credential_changed || config.providers != config_before_save.providers;
            let refs = path
                .parent()
                .and_then(|root| keystore::available_refs(root).ok())
                .unwrap_or_default();
            Ok::<_, vega_conversation::types::ProviderSettingsError>((
                config,
                refs,
                replay_identity_changed,
            ))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                match result {
                    Ok((config, refs, replay_identity_changed)) => {
                        this.config = config;
                        this.available_key_refs = refs;
                        cx.emit(SettingsSaved {
                            replay_identity_changed,
                        });
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
        let config_before_save = self.config.clone();
        let worker = cx.background_executor().spawn(async move {
            ProviderSettingsService::new(path)
                .patch(ProviderPatchRequest { provider, action })
                .map(|config| {
                    let replay_identity_changed = config.providers != config_before_save.providers;
                    (config, replay_identity_changed)
                })
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                match result {
                    Ok((config, replay_identity_changed)) => {
                        this.config = config;
                        cx.emit(SettingsSaved {
                            replay_identity_changed,
                        });
                        if generation != this.provider_management.generation {
                            cx.notify();
                            return;
                        }
                        this.provider_management.model_editor = None;
                        let reopen = this.provider_management.pending_model_reopen.take();
                        this.provider_management.candidates = None;
                        this.provider_management.checked.clear();
                        this.provider_management.statuses.clear();
                        this.provider_management.message = None;
                        this.error = None;
                        if let Some((model, renamed_from)) = reopen {
                            this.provider_command(Command::EditModel(model), cx);
                            if let Some(editor) = this.provider_management.model_editor.as_mut() {
                                editor.renamed_from = renamed_from;
                            }
                        }
                    }
                    Err(error) => {
                        this.provider_management.pending_model_reopen = None;
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
                    let service = ProviderSettingsService::new(path);
                    runtime.block_on(service.network(request, cancel))
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
        let focus = match &command {
            Command::EditModel(model) => {
                self.provider_management
                    .selected
                    .as_ref()
                    .and_then(|provider| {
                        self.provider_management
                            .model_focuses
                            .get(&(provider.clone(), model.clone()))
                    })
            }
            _ => self.provider_management.action_focuses.get(&id),
        }
        .cloned();
        let destructive = matches!(
            command,
            Command::DeleteModel(_, _) | Command::ConfirmDelete(_)
        );
        let primary = matches!(command, Command::SaveModel);
        settings_action_chrome(colors)
            .id(id)
            .interaction_surface(
                if enabled && primary {
                    colors.brand_primary
                } else {
                    gpui_kit::transparent_black().into()
                },
                if primary {
                    colors.brand_primary_strong
                } else {
                    colors.bg_hover
                },
                if primary {
                    colors.brand_primary_strong
                } else {
                    colors.bg_active
                },
                enabled,
            )
            .border_color(if destructive || primary {
                colors.border_subtle
            } else {
                gpui_kit::transparent_black().into()
            })
            .key_context("ProviderControl")
            .focusable()
            .tab_stop(enabled)
            .when_some(focus.as_ref(), |button, focus| button.track_focus(focus))
            .text_color(if enabled {
                if destructive {
                    colors.danger
                } else if primary {
                    colors.brand_on_accent
                } else {
                    colors.text_primary
                }
            } else {
                colors.text_tertiary
            })
            .when(enabled && primary, |button| button.bg(colors.brand_primary))
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
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
    fn provider_icon_button(
        &mut self,
        id: String,
        label: &'static str,
        icon: crate::icons::Icon,
        command: Command,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let keyboard = command.clone();
        let focus = self
            .provider_management
            .menu_focuses
            .entry(id.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let pointer_focus = focus.clone();
        let keyboard_focus = focus.clone();
        div()
            .id(id.clone())
            .aria_label(label)
            .track_focus(&focus)
            .size(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .interactive(colors, enabled, false)
            .tooltip(move |_, cx| crate::icons::tooltip(label, cx))
            .when(enabled, |button| {
                button
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.provider_management.menu_return_focus =
                                Some(pointer_focus.clone());
                            window.focus(&pointer_focus, cx);
                            this.provider_command(command.clone(), cx)
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.provider_management.menu_return_focus =
                                    Some(keyboard_focus.clone());
                                this.provider_command(keyboard.clone(), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
            })
            .focus_visible(move |style| style.bg(colors.bg_active))
            .child(crate::icons::icon(
                icon,
                if enabled {
                    colors.text_secondary
                } else {
                    colors.text_tertiary
                },
            ))
            .into_any_element()
    }

    fn model_capability_summary(&self, provider: &str, model: &str) -> String {
        let profile = match &self.reasoning {
            ReasoningSettingsProjection::Ready { profiles, .. } => profiles
                .iter()
                .find(|profile| profile.provider == provider && profile.model == model),
            _ => None,
        };
        match profile.map(|profile| profile.support) {
            Some(ReasoningSupport::Required) => "始终开启思考".into(),
            Some(ReasoningSupport::Optional) => "支持思考控制".into(),
            Some(ReasoningSupport::Unsupported) => "不支持思考控制".into(),
            _ => "思考：提供方默认".into(),
        }
    }

    fn provider_connection_row(
        &self,
        label: &'static str,
        value: impl IntoElement,
        colors: vega_theme::ThemeColors,
    ) -> Div {
        div()
            .min_w_0()
            .flex()
            .gap_3()
            .child(
                div()
                    .w(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT * 2.))
                    .flex_shrink_0()
                    .text_color(colors.text_secondary)
                    .child(label),
            )
            .child(div().min_w_0().flex_1().child(value))
    }

    fn provider_object_menu(&self, id: &str, cx: &mut Context<Self>) -> gpui_kit::Stateful<Div> {
        let colors = theme(cx).colors;
        let id = id.to_string();
        div()
            .id(id.clone())
            .absolute()
            .top(relative(1.))
            .right_0()
            .min_w(px(Layout::SETTINGS_PROVIDER_LIST_WIDTH))
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(Layout::MENU_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .shadow_sm()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _: &gpui_kit::MouseDownEvent, _, cx| {
                this.provider_management.menu = None;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.provider_management.menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
    }

    pub(crate) fn catalog_capacity_editable(&self) -> bool {
        !self.provider_is_saving()
            && self
                .provider_management
                .model_editor
                .as_ref()
                .is_some_and(|editor| {
                    editor.source != PolicySource::Loading && editor.original.is_some()
                })
    }

    pub(crate) fn apply_catalog_capacity_candidate(
        &mut self,
        input: bool,
        value: u64,
        cx: &mut Context<Self>,
    ) {
        if !self.catalog_capacity_editable() {
            return;
        }
        if let Some(editor) = self.provider_management.model_editor.as_mut() {
            editor.unknown = false;
            let target = if input {
                &editor.input_limit
            } else {
                &editor.output_limit
            };
            target.update(cx, |input, cx| input.set_text(&value.to_string(), cx));
            cx.notify();
        }
    }

    pub(crate) fn render_model_basics(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let busy = self.provider_management.saving;
        let mut detail = settings_section();
        if let Some(editor) = &self.provider_management.model_editor {
            let can_edit = !busy && !editor.saving;
            detail = detail
                    .child(settings_field("模型 ID", None, editor.model_input.clone(), colors))
                    .when(editor.original.is_some(), |detail| {
                        let source = match editor.source {
                            PolicySource::Loading => "正在读取模型上下文配置…".to_string(),
                            PolicySource::AssumedDefault => format!(
                                "默认假设值：输入 {DEFAULT_MODEL_INPUT_LIMIT} token，输出 {DEFAULT_MODEL_OUTPUT_RESERVE} token；不是供应商验证容量。小窗口模型请按实际能力修改。"
                            ),
                            PolicySource::Saved => "已保存的模型容量；请按供应商真实能力核对。".to_string(),
                            PolicySource::Unknown => "容量未知：仍可发送，但不会按预算自动压缩。".to_string(),
                        };
                        detail
                            .child(
                                div()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_secondary)
                                    .child(div().when(editor.source != PolicySource::Loading, |row| row).child(source)),
                            )
                            .when(editor.legacy_present, |detail| {
                                detail.child(
                                    div()
                                        .text_size(px(Typography::METADATA))
                                        .text_color(colors.warning)
                                        .child("旧会话级上下文设置已保留，但不再生效；请在此配置模型容量。"),
                                )
                            })
                            .children(editor.renamed_from.as_ref().map(|previous| {
                                div()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.warning)
                                    .child(format!(
                                        "模型 ID 已从 {previous} 更改；旧 ID 的容量配置不会自动沿用。请检查并保存新模型容量。"
                                    ))
                            }))
                            .when(!editor.unknown, |detail| detail.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(settings_field("输入最大限制", Some("token".into()),
                                        div().child(editor.input_limit.clone()), colors))
                                    .child(settings_field("输出最大限制", Some("token".into()),
                                        div().child(editor.output_limit.clone()), colors)),
                            ))
                            .child(self.provider_button(
                                "model-context-auto".into(),
                                if editor.automatic {
                                    "自动压缩：开启"
                                } else {
                                    "自动压缩：关闭"
                                },
                                Command::ToggleModelAutomatic,
                                can_edit,
                                cx,
                            ))
                            .child(self.provider_button(
                                "model-context-unknown".into(),
                                if editor.unknown {
                                    "容量未知：已选择"
                                } else {
                                    "容量未知：未选择"
                                },
                                Command::ToggleModelUnknown,
                                can_edit,
                                cx,
                            ))
                    })
                    .children(editor.message.as_ref().map(|message| {
                        div()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child(message.clone())
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.provider_button(
                                "model-save".into(),
                                "保存模型设置",
                                Command::SaveModel,
                                can_edit,
                                cx,
                            ))
                            .child(self.provider_button(
                                "model-cancel".into(),
                                "取消",
                                Command::Cancel,
                                can_edit,
                                cx,
                            )),
                    );
        }
        detail
            .child(self.render_model_capability_image(cx))
            .child(self.render_model_catalog(cx))
            .into_any_element()
    }

    pub(crate) fn render_provider_management(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_provider_edit_focuses(cx);
        self.provider_management
            .model_focuses
            .retain(|(provider, model), _| {
                self.config.providers.iter().any(|candidate| {
                    candidate.name == *provider && candidate.models.contains(model)
                })
            });
        for provider in &self.config.providers {
            for model in &provider.models {
                self.provider_management
                    .model_focuses
                    .entry((provider.name.clone(), model.clone()))
                    .or_insert_with(|| cx.focus_handle().tab_stop(true));
            }
        }
        for id in ["provider-add", "provider-edit"] {
            self.provider_management
                .action_focuses
                .entry(id.into())
                .or_insert_with(|| cx.focus_handle().tab_stop(true));
        }
        let colors = theme(cx).colors;
        if self.provider_management.model_editor.is_some() {
            return div()
                .id("provider-model-configuration")
                .on_action(cx.listener(Self::provider_escape))
                .size_full()
                .overflow_y_scroll()
                .child(self.render_model_basics(cx))
                .scroll_edge_fade(colors.bg_base, ScrollFade::reading())
                .into_any_element();
        }
        let busy = self.provider_management.saving;
        let narrow = super::motion::narrow(window);
        let show_detail =
            !narrow || self.provider_management.detail_open || self.provider_management.form;
        let network_busy = self.provider_management.cancel.is_some();
        let mut list = div()
            .id("provider-list")
            .role(gpui_kit::Role::ListBox)
            .h_full()
            .track_scroll(&self.provider_management.list_scroll)
            .w(px(Layout::SETTINGS_PROVIDER_LIST_WIDTH))
            .when(narrow, |list| list.w_full())
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll()
            .child(
                settings_actions()
                    .child(self.provider_button(
                        "provider-add".into(),
                        "添加提供方",
                        Command::Add,
                        !busy,
                        cx,
                    ))
                    .child(self.provider_icon_button(
                        "provider-reload".into(),
                        "重新读取提供方",
                        crate::icons::Icon::Refresh,
                        Command::Reload,
                        !busy,
                        cx,
                    )),
            )
            .tracked_scroll_edge_fade(
                self.provider_management.list_scroll.clone(),
                colors.bg_base,
                ScrollFade::compact(),
            );
        for (index, p) in self.config.providers.clone().into_iter().enumerate() {
            let selected = self.provider_management.selected.as_ref() == Some(&p.name);
            let name = p.name.clone();
            let key_name = name.clone();
            list = list.child(
                self.motion_provider_row(
                    div()
                        .id(format!("provider-select:{:?}", p.name))
                        .role(gpui_kit::Role::ListBoxOption)
                        .aria_selected(selected)
                        .interaction_surface(
                            gpui_kit::transparent_black().into(),
                            if selected {
                                gpui_kit::transparent_black().into()
                            } else {
                                colors.bg_hover
                            },
                            colors.bg_active,
                            true,
                        )
                        .track_focus(&self.provider_edit_focuses[index].1)
                        .tab_stop(true)
                        .flex()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                        .border_1()
                        .border_color(gpui_kit::transparent_black())
                        .min_h(px(Typography::SIDEBAR_LINE_HEIGHT))
                        .cursor_pointer()
                        .focus_visible(move |style| style.border_color(colors.accent))
                        .on_drag(ProviderDrag(name.clone()), |drag, _, _, cx| {
                            cx.new(|_| drag.clone())
                        })
                        .on_drop(cx.listener(move |this, drag: &ProviderDrag, _, cx| {
                            if !this.provider_management.saving
                                && !this.provider_has_draft(cx)
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
                        .on_key_down(cx.listener(
                            move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.provider_command(Command::Select(key_name.clone()), cx);
                                    cx.stop_propagation();
                                }
                            },
                        ))
                        .child(
                            div()
                                .size(px(Layout::ICON_SIZE))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(if p.enabled {
                                    colors.success
                                } else {
                                    colors.text_tertiary
                                })
                                .child("●"),
                        )
                        .child(div().min_w_0().truncate().child(p.name.clone())),
                    &p.name,
                    selected,
                    colors.bg_active,
                    cx.entity_id(),
                ),
            );
        }
        list = list.child(self.provider_button(
            "provider-pricing-library".into(),
            "模型价格库",
            Command::PriceLibrary,
            !busy,
            cx,
        ));
        // Keep the flow intrinsically sized; the outer viewport owns scrolling.
        // A height-constrained flex column can shrink truncated text rows to zero.
        let mut detail = div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(Layout::SETTINGS_CONTENT_GAP));
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
            let help_focus_count = p.models.len() + 1;
            self.provider_test_help_focuses.truncate(help_focus_count);
            while self.provider_test_help_focuses.len() < help_focus_count {
                self.provider_test_help_focuses
                    .push(cx.focus_handle().tab_stop(true));
            }
            let help_focuses = self.provider_test_help_focuses.clone();
            let discover_help_open = self.provider_management.test_help_dismissed != Some(0)
                && (help_focuses[0].is_focused(window)
                    || self.provider_management.test_help_hovered == Some(0)
                    || self.provider_management.test_help_pinned == Some(0));
            let api_label = match p.api {
                vega_conversation::types::ProviderApi::ChatCompletions => "Chat Completions",
                vega_conversation::types::ProviderApi::Responses => "Responses",
            };
            let index = self
                .config
                .providers
                .iter()
                .position(|candidate| candidate.name == p.name)
                .unwrap_or(0);
            let mut provider_menu = self
                .provider_object_menu("provider-menu", cx)
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
                ));
            provider_menu = provider_menu.child(self.provider_button(
                "provider-menu-close".into(),
                "关闭菜单",
                Command::Menu("provider".into()),
                true,
                cx,
            ));
            detail = detail
                .when(narrow, |detail| {
                    detail.child(self.provider_button(
                        "provider-list-back".into(),
                        "返回提供方列表",
                        Command::BackToList,
                        !busy,
                        cx,
                    ))
                })
                .child(
                    settings_actions()
                        .child(
                            self.motion_title(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(Typography::HEADING_CARD))
                                    .font_weight(Typography::HEADING_CARD_WEIGHT)
                                    .child(p.name.clone()),
                                cx.entity_id(),
                            ),
                        )
                        .child(self.provider_button(
                            "provider-enabled".into(),
                            if p.enabled { "已启用" } else { "已禁用" },
                            Command::Enable,
                            !busy,
                            cx,
                        ))
                        .child(self.provider_button(
                            "provider-edit".into(),
                            "编辑",
                            Command::Edit,
                            !busy,
                            cx,
                        ))
                        .child(
                            div()
                                .relative()
                                .child(self.provider_icon_button(
                                    "provider-more".into(),
                                    "提供方操作",
                                    crate::icons::Icon::More,
                                    Command::Menu("provider".into()),
                                    !busy,
                                    cx,
                                ))
                                .when(
                                    self.provider_management.menu.as_deref() == Some("provider"),
                                    |host| {
                                        host.child(
                                            gpui_kit::deferred(provider_menu).with_priority(2),
                                        )
                                    },
                                ),
                        ),
                )
                .child(
                    settings_section()
                        .gap_2()
                        .child(self.provider_connection_row(
                            "地址",
                            div().min_w_0().child(p.base_url.clone()),
                            colors,
                        ))
                        .child(self.provider_connection_row("协议", div().child(api_label), colors))
                        .child(self.provider_connection_row(
                            "凭据",
                            div().child(if self.available_key_refs.contains(&p.key_ref) {
                                KEY_STORED_PLACEHOLDER
                            } else {
                                "需重新输入 API Key"
                            }),
                            colors,
                        )),
                )
                .child(
                    settings_actions()
                        .mt_4()
                        .relative()
                        .child(
                            div()
                                .flex_1()
                                .child(section_title("模型", colors.text_primary)),
                        )
                        .child(self.provider_button(
                            "provider-discover".into(),
                            "发现模型",
                            Command::Discover,
                            !busy && !network_busy && p.enabled,
                            cx,
                        ))
                        .child(self.provider_test_help_control(0, help_focuses[0].clone(), cx))
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
                        })
                        .when(discover_help_open, |row| {
                            row.child(self.provider_test_help_tooltip(0, colors, cx))
                        }),
                );
            for (index, model) in p.models.iter().enumerate() {
                let help_index = index + 1;
                let help_open = self.provider_management.test_help_dismissed != Some(help_index)
                    && (help_focuses[help_index].is_focused(window)
                        || self.provider_management.test_help_hovered == Some(help_index)
                        || self.provider_management.test_help_pinned == Some(help_index));
                let menu_key = format!("model-{index}");
                let menu = self
                    .provider_object_menu(&format!("model-menu-{index}"), cx)
                    .child(
                        settings_actions()
                            .relative()
                            .child(self.provider_button(
                                format!("model-test-{index}"),
                                "测试",
                                Command::Test(model.clone()),
                                !busy && !network_busy && p.enabled,
                                cx,
                            ))
                            .child(self.provider_test_help_control(
                                help_index,
                                help_focuses[help_index].clone(),
                                cx,
                            ))
                            .when(help_open, |row| {
                                row.child(self.provider_test_help_tooltip(help_index, colors, cx))
                            }),
                    )
                    .child(self.provider_button(
                        format!("model-delete-{index}"),
                        "删除",
                        Command::ConfirmDelete(model.clone()),
                        !busy,
                        cx,
                    ));
                detail = detail.child(
                    settings_section()
                        .gap_1()
                        .border_b_1()
                        .border_color(colors.border_subtle)
                        .py_2()
                        .child(
                            settings_actions()
                                .items_start()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .child(div().min_w_0().child(model.clone()))
                                        .child(
                                            div()
                                                .text_size(px(Typography::METADATA))
                                                .text_color(colors.text_secondary)
                                                .child(
                                                    self.model_capability_summary(&p.name, model),
                                                ),
                                        ),
                                )
                                .child(self.provider_button(
                                    format!("model-edit-{index}"),
                                    "配置",
                                    Command::EditModel(model.clone()),
                                    !busy,
                                    cx,
                                ))
                                .child(
                                    div()
                                        .relative()
                                        .child(self.provider_icon_button(
                                            format!("model-more-{index}"),
                                            "模型操作",
                                            crate::icons::Icon::More,
                                            Command::Menu(menu_key.clone()),
                                            !busy,
                                            cx,
                                        ))
                                        .when(
                                            self.provider_management.menu.as_ref()
                                                == Some(&menu_key),
                                            |host| {
                                                host.child(
                                                    gpui_kit::deferred(menu).with_priority(2),
                                                )
                                            },
                                        ),
                                ),
                        )
                        .children(self.provider_management.statuses.get(model).map(|status| {
                            div()
                                .text_color(colors.text_secondary)
                                .child(status.clone())
                        })),
                );
            }
            if let Some((provider, model)) = &self.provider_management.delete_model {
                detail = detail.child(
                    settings_section()
                        .child(settings_status(
                            format!("从 {provider} 删除模型 {model}？"),
                            colors.warning,
                            colors,
                        ))
                        .child(
                            settings_actions()
                                .child(self.provider_button(
                                    "model-delete-confirm".into(),
                                    "删除模型",
                                    Command::DeleteModel(provider.clone(), model.clone()),
                                    !busy,
                                    cx,
                                ))
                                .child(self.provider_button(
                                    "model-delete-cancel".into(),
                                    "取消",
                                    Command::Cancel,
                                    !busy,
                                    cx,
                                )),
                        ),
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
                            .child(div().min_w_0().flex_1().child(model)),
                    );
                }
                detail = detail.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
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
            detail = detail.child(settings_status(
                message.clone(),
                colors.text_secondary,
                colors,
            ));
        }
        self.restore_provider_return_focus(show_detail, narrow, window, cx);
        div()
            .id("provider-management")
            .size_full()
            .min_h_0()
            .on_action(cx.listener(Self::provider_escape))
            .flex()
            .items_start()
            .min_w_0()
            .gap(px(Layout::SETTINGS_PROVIDER_COLUMN_GAP))
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter"
                        && this
                            .provider_management
                            .model_editor
                            .as_ref()
                            .is_some_and(|editor| {
                                editor
                                    .model_input
                                    .read(cx)
                                    .focus_handle(cx)
                                    .is_focused(window)
                            })
                    {
                        this.provider_command(Command::SaveModel, cx);
                        cx.stop_propagation();
                        return;
                    }
                    if event.keystroke.key == "escape"
                        && (this.provider_management.menu.is_some()
                            || this.provider_management.form
                            || this.provider_management.model_editor.is_some()
                            || this.provider_management.candidates.is_some()
                            || this.provider_management.cancel.is_some())
                    {
                        this.provider_escape(&CloseSettings, window, cx);
                    }
                }),
            )
            .when(!narrow || !show_detail, |root| root.child(list))
            .when(show_detail, |root| {
                root.child(
                    div()
                        .id("provider-detail")
                        .track_scroll(&self.provider_management.detail_scroll)
                        .h_full()
                        .min_h_0()
                        .min_w_0()
                        .flex_1()
                        .overflow_y_scroll()
                        .child(detail)
                        .tracked_scroll_edge_fade(
                            self.provider_management.detail_scroll.clone(),
                            colors.bg_base,
                            ScrollFade::reading(),
                        ),
                )
            })
            .into_any_element()
    }
}
