use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;
use vega_conversation::ProviderSettingsService;
use vega_conversation::types::{
    ProviderNetworkAction, ProviderNetworkOutcome, ProviderNetworkRequest, ProviderPatchAction,
    ProviderPatchRequest,
};
use vega_store::context_compaction::{DEFAULT_MODEL_INPUT_LIMIT, DEFAULT_MODEL_OUTPUT_RESERVE};

mod draft;
pub(in crate::settings) use draft::DraftStamp as ProviderDraftStamp;
#[path = "model_configuration/provider_bridge.rs"]
pub(super) mod navigation_bridge;
mod nsh_providers;

fn provider_test_help_copy() -> &'static str {
    t("settings.provider.test_help")
}

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
    statuses: BTreeMap<String, SettingsText>,
    message: Option<SettingsText>,
    delete_model: Option<(String, String)>,
    detail_open: bool,
    model_editor: Option<ModelEditor>,
    next_model_context_request: u64,
    pending_model_reopen: Option<(String, Option<String>)>,
    pending_command: Option<Command>,
    return_focus: Option<super::motion::ReturnFocus>,
    presentation: Option<nsh_providers::Presentation>,
    nsh_draft: Option<draft::Draft>,
    save_intent: u64,
    consumed_secret_revision: Option<u64>,
    draft_inputs: Vec<gpui_kit::EntityId>,
    pending_capacity: Option<[String; 2]>,
    submission_claimed: bool,
    intent_stamp: Option<ProviderDraftStamp>,
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
    message: Option<SettingsText>,
    renamed_from: Option<String>,
}
impl ProviderManagement {
    fn return_to_list(&mut self) {
        self.detail_open = false;
        self.form = false;
        if let Some(draft) = self.nsh_draft.take() {
            draft.retire();
        }
        self.draft_inputs.clear();
        self.intent_stamp = None;
        self.pending_capacity = None;
        self.consumed_secret_revision = None;
    }
}

impl Drop for ProviderManagement {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
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
    ConfirmDelete(String),
    BackToList,
    PriceLibrary,
}

impl SettingsView {
    fn model_context_error(error: String) -> SettingsText {
        SettingsText::from_known(
            error,
            &[
                "app.model_context.database_unavailable",
                "app.model_context.read_failed",
                "app.model_context.save_failed",
            ],
        )
    }

    pub(super) fn refresh_provider_localization(&self, cx: &mut Context<Self>) {
        if let Some(editor) = &self.provider_management.model_editor {
            for input in [
                &editor.model_input,
                &editor.input_limit,
                &editor.output_limit,
            ] {
                input.update(cx, |_, cx| cx.notify());
            }
        }
    }

    pub(super) fn model_editor_visible(&self) -> bool {
        self.section == 0 && self.provider_management.model_editor.is_some()
    }

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

    fn remember_provider_list_return_focus(&mut self) {
        let provider = self.provider_management.selected.clone();
        let index = self
            .config
            .providers
            .iter()
            .position(|candidate| Some(&candidate.name) == provider.as_ref())
            .unwrap_or(0);
        self.provider_management.return_focus =
            Some(super::motion::ReturnFocus::ProviderList { provider, index });
    }

    pub(super) fn remember_model_return_focus(&mut self) {
        if let Some((provider, model)) = self.model_settings.clone() {
            self.provider_management.detail_open = true;
            self.provider_management.return_focus =
                Some(super::motion::ReturnFocus::Model { provider, model });
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
        state
            .statuses
            .retain(|_, value| !value.is_key("settings.provider.testing"));
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
        self.rebuild_reasoning_focuses(cx);
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
            || self.provider_nsh_dirty(cx)
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
            self.reset_provider_nsh_draft(cx);
            self.provider_command(command, cx);
        }
    }

    pub(crate) fn discard_provider_draft(&mut self, cx: &mut Context<Self>) {
        if let Some(stamp) = self.provider_nsh_stamp() {
            self.discard_provider_nsh_observed(&stamp, cx);
        } else {
            self.key_input.update(cx, TextInput::clear);
            self.provider_management.form = false;
        }
    }

    pub(crate) fn close_model_configuration(&mut self, cx: &mut Context<Self>) {
        self.provider_management.delete_model = None;
        self.model_settings = None;
        self.provider_management.model_editor = None;
        self.provider_management.form = false;
        self.reset_provider_nsh_draft(cx);
        self.key_input.update(cx, TextInput::clear);
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
                | Command::AddModel
                | Command::EditModel(_)
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
            Command::ConfirmDelete(model) => {
                self.provider_management.delete_model = self
                    .selected_provider()
                    .filter(|provider| provider.models.contains(&model))
                    .map(|provider| (provider.name, model));
            }
            Command::BackToList => {
                self.remember_provider_list_return_focus();
                self.provider_management.return_to_list();
                self.key_input.update(cx, TextInput::clear);
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
                if let Some(draft) = self.provider_management.nsh_draft.take() {
                    draft.retire();
                }
                self.provider_management.draft_inputs.clear();
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
                    self.reset_provider_nsh_draft(cx);
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
                self.key_input.update(cx, TextInput::clear);
                self.remember_model_return_focus();
                if self.provider_management.return_focus.is_none() {
                    self.remember_provider_list_return_focus();
                }
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
                self.provider_management.form = false;
                let original = if let Command::EditModel(model) = command {
                    Some(model)
                } else {
                    None
                };
                self.model_settings = self
                    .selected_provider()
                    .and_then(|provider| original.clone().map(|model| (provider.name, model)));
                let input = cx.new(|cx| {
                    TextInput::new(cx, "", false)
                        .with_placeholder_key("settings.field.model_id")
                        .with_tab_stop(true)
                });
                if let Some(model) = &original {
                    input.update(cx, |input, cx| input.set_text(model, cx));
                }
                let input_limit = cx.new(|cx| {
                    TextInput::new(cx, "", false)
                        .with_placeholder_key("settings.provider.input_limit_placeholder")
                        .with_tab_stop(true)
                });
                let output_limit = cx.new(|cx| {
                    TextInput::new(cx, "", false)
                        .with_placeholder_key("settings.provider.output_limit_placeholder")
                        .with_tab_stop(true)
                });
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
                        self.error = Some(SettingsText::key("settings.provider.invalid_model"));
                    } else if let Some(mut p) = self.selected_provider() {
                        if p.models
                            .iter()
                            .any(|m| m == value.trim() && Some(m) != original.as_ref())
                        {
                            self.error = Some(SettingsText::key("settings.provider.model_exists"));
                        } else if original.as_deref() == Some(value.trim()) {
                            self.save_model_context(cx);
                        } else {
                            if original.is_some() && self.model_context_values_changed(cx) {
                                self.error =
                                    Some(SettingsText::key("settings.provider.save_id_first"));
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
        self.sync_provider_editor_lifecycle(cx);
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
            self.error = Some(SettingsText::key("settings.provider.context_loading_retry"));
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
                    self.error = Some(SettingsText::key("settings.provider.invalid_capacity"));
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
        editor.message = Some(SettingsText::key("settings.provider.context_saving"));
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
                editor.message = Some(Self::model_context_error(error));
            }
        }
        self.provider_management.pending_capacity = self
            .provider_management
            .model_editor
            .as_ref()
            .filter(|editor| editor.source != PolicySource::Loading)
            .map(|editor| {
                [
                    editor.input_limit.read(cx).text().to_string(),
                    editor.output_limit.read(cx).text().to_string(),
                ]
            });
        self.sync_loaded_provider_capacity(cx);
        cx.notify();
    }

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
                editor.message = Some(SettingsText::key("settings.provider.context_saved"));
                self.error = None;
            }
            Err(error) => {
                self.error = Some(Self::model_context_error(error.clone()));
                editor.message = Some(Self::model_context_error(error));
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
            self.error = Some(SettingsText::key("settings.config.path_unavailable"));
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
        ]
        .map(|input| input.read(cx).text().to_string());
        let key_revision = self.key_input.read(cx).content_revision();
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
                        this.apply_unrelated_config(config);
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
                        ];
                        if this.key_input.read(cx).content_revision() == key_revision
                            && inputs
                                .iter()
                                .zip(&draft)
                                .all(|(input, text)| input.read(cx).text() == text)
                        {
                            this.provider_management.form = false;
                            this.provider_management.form_base = None;
                            this.reset_provider_nsh_draft(cx);
                            for input in inputs {
                                input.update(cx, TextInput::clear);
                            }
                            this.key_input.update(cx, TextInput::clear);
                        }
                        this.provider_management.statuses.clear();
                        this.provider_management.message = None;
                        this.error = None;
                    }
                    Err(error) => {
                        if generation == this.provider_management.generation {
                            this.error = Some(SettingsText::UnsavedProvider(error));
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
                        this.apply_unrelated_config(config);
                        this.provider_management.statuses.clear();
                        this.provider_management.message = None;
                        this.error = None;
                    }
                    Err(error) => {
                        if generation == this.provider_management.generation {
                            this.error = Some(SettingsText::Provider(error));
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
                        this.apply_unrelated_config(config);
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
                            this.error = Some(SettingsText::Provider(error));
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
            state.statuses.insert(
                model.clone(),
                SettingsText::key("settings.provider.testing"),
            );
        }
        state.message = Some(SettingsText::key("settings.provider.connecting"));
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
                        SettingsText::args(
                            "settings.provider.models_discovered",
                            &[("count", &(models.len()).to_string())],
                        )
                    }
                    Ok(ProviderNetworkOutcome::ModelTestSucceeded) => {
                        SettingsText::key("settings.provider.connection_success")
                    }
                    Err(error) => SettingsText::Provider(error.clone()),
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
    fn model_capability_summary(&self, provider: &str, model: &str) -> String {
        let profile = match &self.reasoning {
            ReasoningSettingsProjection::Ready { profiles, .. } => profiles
                .iter()
                .find(|profile| profile.provider == provider && profile.model == model),
            _ => None,
        };
        match profile.map(|profile| profile.support) {
            Some(ReasoningSupport::Required) => t("settings.provider.reasoning.required").into(),
            Some(ReasoningSupport::Optional) => t("settings.provider.reasoning.optional").into(),
            Some(ReasoningSupport::Unsupported) => {
                t("settings.provider.reasoning.unsupported").into()
            }
            _ => t("settings.provider.reasoning.default").into(),
        }
    }

    pub(crate) fn catalog_capacity_editable(&self) -> bool {
        !self.provider_is_saving()
            && self
                .provider_management
                .nsh_draft
                .as_ref()
                .is_none_or(|draft| {
                    let values = draft.read();
                    values.valid && !values.frozen && !values.composing.contains(&true)
                })
            && self
                .provider_management
                .model_editor
                .as_ref()
                .is_some_and(|editor| {
                    editor.source != PolicySource::Loading && editor.original.is_some()
                })
    }

    pub(super) fn catalog_capacity_fence(
        &self,
        cx: &App,
    ) -> Option<(u64, String, String, String, bool, bool)> {
        if !self.catalog_capacity_editable() {
            return None;
        }
        self.provider_management
            .model_editor
            .as_ref()
            .map(|editor| {
                let values = self
                    .provider_management
                    .nsh_draft
                    .as_ref()
                    .map(|draft| draft.read());
                (
                    editor.request_id,
                    values
                        .as_ref()
                        .map(|v| v.text[0].clone())
                        .unwrap_or_else(|| editor.model_input.read(cx).text().to_string()),
                    values
                        .as_ref()
                        .map(|v| v.text[1].clone())
                        .unwrap_or_else(|| editor.input_limit.read(cx).text().to_string()),
                    values
                        .as_ref()
                        .map(|v| v.text[2].clone())
                        .unwrap_or_else(|| editor.output_limit.read(cx).text().to_string()),
                    editor.automatic,
                    editor.unknown,
                )
            })
    }

    pub(in crate::settings) fn apply_catalog_capacity_observed(
        &mut self,
        stamp: &ProviderDraftStamp,
        input: bool,
        value: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.catalog_capacity_editable()
            || self
                .provider_management
                .nsh_draft
                .as_ref()
                .is_none_or(|draft| {
                    !draft.replace_observed(stamp, if input { 1 } else { 2 }, value.to_string())
                })
        {
            return false;
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
            true
        } else {
            false
        }
    }

    pub(crate) fn render_model_basics(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let basics = self.render_provider_surface(window, cx);
        let capabilities = self.render_model_capability_image(window, cx);
        let catalog = self.render_model_catalog(window, cx);
        div()
            .id("provider-model-native-host")
            .w_full()
            .h_full()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(Layout::SETTINGS_CONTENT_GAP))
            .child(basics)
            .child(capabilities)
            .child(catalog)
            .into_any_element()
    }

    pub(crate) fn render_provider_management(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.provider_management.model_editor.is_some() {
            self.render_model_basics(window, cx)
        } else {
            self.render_provider_surface(window, cx)
        }
    }
}
