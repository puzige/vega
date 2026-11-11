use std::sync::Arc;

use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::ProviderApi;
use vega_i18n::{t, t_args};
use vega_store::config::ProviderConfig;
use vega_theme::{Layout, ThemeColors, Typography};

use super::ProviderDraftStamp as DraftStamp;
use super::draft::{Draft, DraftValues};
use super::{Command, PolicySource};
use crate::nsh::{ProjectionId, Surface};
use crate::settings::{SettingsText, SettingsView};

#[derive(Clone, PartialEq, Eq)]
struct ProviderRow {
    name: String,
    url: String,
    api: ProviderApi,
    enabled: bool,
    credential: bool,
    models: Vec<(String, String)>,
}
#[derive(Clone, PartialEq, Eq)]
struct ModelState {
    original: Option<String>,
    source: PolicySource,
    automatic: bool,
    unknown: bool,
    legacy: bool,
    renamed: Option<String>,
    message: Option<String>,
    saving: bool,
}
#[derive(Clone, PartialEq, Eq)]
struct Authority {
    rows: Vec<ProviderRow>,
    selected: Option<String>,
    route: Option<(String, String)>,
    generation: u64,
    operation: u64,
    section: usize,
    form: bool,
    existing: bool,
    retained_key: bool,
    api: ProviderApi,
    detail: bool,
    saving: bool,
    network: bool,
    path: bool,
    reload_needed: bool,
    candidates: Option<Vec<String>>,
    checked: Vec<String>,
    statuses: Vec<(String, String)>,
    message: Option<String>,
    delete: Option<(String, String)>,
    model: Option<ModelState>,
    unavailable: bool,
    save_intent: u64,
    language: &'static str,
}
#[derive(Clone)]
struct Editor {
    draft: Draft,
    values: DraftValues,
}
#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    version: ProjectionId,
    visible: bool,
    guard_focused: bool,
    authority: Arc<Authority>,
    editor: Option<Editor>,
    colors: ThemeColors,
    narrow: bool,
    focus: Option<(u64, String)>,
}
impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.version.matches(&other.version)
            && self.visible == other.visible
            && self.guard_focused == other.guard_focused
            && self.authority == other.authority
            && self.narrow == other.narrow
            && self.focus == other.focus
            && match (&self.editor, &other.editor) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.draft.session.matches(&b.draft.session) && a.values == b.values
                }
                _ => false,
            }
    }
}
#[derive(Clone, PartialEq, Eq)]
enum Action {
    Command(Command),
    Api(ProviderApi),
    Submit(u64),
    Move(String, String),
    Escape,
}
struct Decision {
    observed: Snapshot,
    action: Action,
    secret: Option<String>,
    reply: oneshot::Sender<bool>,
}
enum Request {
    Decide(Box<Decision>),
    Changed,
    Layout(f32),
}
#[derive(Clone, PartialEq)]
struct ControllerFence {
    providers: Vec<ProviderConfig>,
    base: Option<ProviderConfig>,
    inputs: Vec<(u64, u64)>,
    request: u64,
    pending: Option<(u8, usize)>,
    pending_close: bool,
    config_path: Option<std::path::PathBuf>,
    pending_command: Option<Command>,
}
pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    version: ProjectionId,
    fence: ControllerFence,
    height: f32,
    narrow: bool,
    focus: Option<(u64, String)>,
    intent_fence: Option<ControllerFence>,
}
#[derive(Clone, PartialEq, Eq)]
struct Control {
    label: String,
    action: Action,
    enabled: bool,
    selected: Option<bool>,
    reason: String,
}
fn control(label: impl Into<String>, command: Command, enabled: bool) -> Control {
    Control {
        label: label.into(),
        action: Action::Command(command),
        enabled,
        selected: None,
        reason: String::new(),
    }
}
fn selected_row(a: &Authority) -> Option<&ProviderRow> {
    a.rows
        .iter()
        .find(|row| Some(&row.name) == a.selected.as_ref())
}
fn form_ready(s: &Snapshot) -> bool {
    s.editor.as_ref().is_some_and(|editor| {
        let v = &editor.values;
        v.valid
            && !v.frozen
            && !v.composing.contains(&true)
            && !v.text[0].trim().is_empty()
            && !v.text[1].trim().is_empty()
            && (s.authority.existing || v.secret_present || s.authority.retained_key)
    })
}
fn controls(s: &Snapshot) -> Vec<Control> {
    let a = &s.authority;
    if a.unavailable {
        return Vec::new();
    }
    let idle = !a.saving;
    let editable = idle
        && s.editor.as_ref().is_none_or(|e| {
            e.values.valid && !e.values.frozen && !e.values.composing.contains(&true)
        });
    let mut out = Vec::new();
    if let Some(model) = &a.model {
        let mut save = control(
            t("settings.provider.save_model"),
            Command::SaveModel,
            editable && !model.saving,
        );
        if !save.enabled {
            save.reason = t("settings.provider.context_saving").into();
        }
        out.push(save);
        out.push(control(
            t("settings.action.cancel"),
            Command::Cancel,
            idle && !model.saving,
        ));
        if model.original.is_some() {
            for (command, enabled, key, checked) in [
                (
                    Command::ToggleModelAutomatic,
                    editable && !model.saving && model.source != PolicySource::Loading,
                    if model.automatic {
                        "settings.provider.compaction_on"
                    } else {
                        "settings.provider.compaction_off"
                    },
                    model.automatic,
                ),
                (
                    Command::ToggleModelUnknown,
                    editable && !model.saving && model.source != PolicySource::Loading,
                    if model.unknown {
                        "settings.provider.unknown_selected"
                    } else {
                        "settings.provider.unknown_unselected"
                    },
                    model.unknown,
                ),
            ] {
                let mut item = control(t(key), command, enabled);
                item.selected = Some(checked);
                if model.source == PolicySource::Loading {
                    item.reason = t("settings.provider.context_loading").into();
                }
                out.push(item);
            }
        }
        return out;
    }
    let show_list = !s.narrow || !a.detail;
    if show_list {
        out.push(control(t("settings.provider.add"), Command::Add, idle));
        if a.reload_needed {
            out.push(control(
                t("settings.provider.reload"),
                Command::Reload,
                idle && a.path,
            ));
        }
        out.push(control(
            t("settings.provider.price_library"),
            Command::PriceLibrary,
            idle,
        ));
        for row in &a.rows {
            let mut item = control(
                format!(
                    "{} — {}",
                    row.name,
                    t(if row.enabled {
                        "settings.state.enabled"
                    } else {
                        "settings.state.disabled"
                    })
                ),
                Command::Select(row.name.clone()),
                idle,
            );
            item.selected = Some(a.selected.as_ref() == Some(&row.name));
            out.push(item);
        }
    }
    if !s.narrow || a.detail {
        if s.narrow {
            out.push(control(
                t("settings.provider.back_list"),
                Command::BackToList,
                idle,
            ));
        }
        if a.form {
            for api in [ProviderApi::ChatCompletions, ProviderApi::Responses] {
                out.push(Control {
                    label: api_label(api).into(),
                    action: Action::Api(api),
                    enabled: editable,
                    selected: Some(a.api == api),
                    reason: String::new(),
                });
            }
            out.push(Control {
                label: t("settings.provider.save").into(),
                action: Action::Submit(0),
                enabled: editable && form_ready(s) && a.path,
                selected: None,
                reason: if !a.path {
                    t("settings.config.path_unavailable").into()
                } else if s
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.values.text[0].trim().is_empty())
                {
                    t("settings.field.name").into()
                } else if s
                    .editor
                    .as_ref()
                    .is_some_and(|e| e.values.text[1].trim().is_empty())
                {
                    t("settings.provider.service_url").into()
                } else if !form_ready(s) {
                    t("settings.provider.key_required").into()
                } else {
                    String::new()
                },
            });
            out.push(control(t("settings.action.cancel"), Command::Cancel, idle));
        } else if let Some(row) = selected_row(a) {
            let mut enabled = control(
                t(if row.enabled {
                    "settings.state.enabled"
                } else {
                    "settings.state.disabled"
                }),
                Command::Enable,
                idle,
            );
            enabled.selected = Some(row.enabled);
            out.push(enabled);
            out.push(control(t("settings.action.edit"), Command::Edit, idle));
            let position = a.rows.iter().position(|r| r.name == row.name).unwrap_or(0);
            out.push(control(
                t("settings.provider.move_up"),
                Command::Up,
                idle && position > 0,
            ));
            out.push(control(
                t("settings.provider.move_down"),
                Command::Down,
                idle && position + 1 < a.rows.len(),
            ));
            let mut discover = control(
                t("settings.provider.discover"),
                Command::Discover,
                idle && !a.network && row.enabled,
            );
            if !row.enabled {
                discover.reason = t("settings.state.disabled").into();
            }
            out.push(discover);
            out.push(control(
                t("settings.provider.add_model"),
                Command::AddModel,
                idle,
            ));
            if a.network {
                out.push(control(t("settings.action.stop"), Command::Cancel, idle));
            }
            for (model, _) in &row.models {
                out.push(control(
                    format!("{}: {model}", t("settings.provider.configure")),
                    Command::EditModel(model.clone()),
                    idle,
                ));
                let mut test = control(
                    format!("{}: {model}", t("settings.provider.test")),
                    Command::Test(model.clone()),
                    idle && !a.network && row.enabled,
                );
                if !row.enabled {
                    test.reason = t("settings.state.disabled").into();
                }
                out.push(test);
                out.push(control(
                    format!("{}: {model}", t("settings.action.delete")),
                    Command::ConfirmDelete(model.clone()),
                    idle,
                ));
            }
            if let Some((provider, model)) = &a.delete {
                out.push(control(
                    t("settings.provider.delete_model"),
                    Command::DeleteModel(provider.clone(), model.clone()),
                    idle,
                ));
                out.push(control(t("settings.action.cancel"), Command::Cancel, idle));
            }
            if let Some(candidates) = &a.candidates {
                for model in candidates {
                    let existing = row.models.iter().any(|(name, _)| name == model);
                    let mut item = control(
                        format!(
                            "{model} — {}",
                            if existing {
                                t("settings.provider.already_added")
                            } else if a.checked.contains(model) {
                                "☑"
                            } else {
                                "☐"
                            }
                        ),
                        Command::Candidate(model.clone()),
                        idle && !existing,
                    );
                    item.selected = Some(a.checked.contains(model));
                    out.push(item);
                }
                out.push(control(
                    t("settings.provider.add_selected"),
                    Command::Import,
                    idle && !a.checked.is_empty(),
                ));
                out.push(control(t("settings.action.cancel"), Command::Cancel, idle));
            }
        }
    }
    for item in &mut out {
        if !item.enabled && item.reason.is_empty() {
            item.reason = if !a.path {
                t("settings.config.path_unavailable")
            } else if a.saving {
                t("settings.provider.context_saving")
            } else if a.network {
                t("settings.provider.connecting")
            } else {
                ""
            }
            .into();
        }
    }
    out
}
fn api_label(api: ProviderApi) -> &'static str {
    t(match api {
        ProviderApi::ChatCompletions => "settings.provider.api.chat_completions",
        ProviderApi::Responses => "settings.provider.api.responses",
    })
}
fn allowed(s: &Snapshot, action: &Action) -> bool {
    match action {
        Action::Escape => {
            !s.authority.unavailable
                && !s.authority.saving
                && (s.authority.form
                    || s.authority.model.is_some()
                    || s.authority.network
                    || s.authority.candidates.is_some()
                    || s.authority.delete.is_some())
        }
        Action::Move(from, to) => {
            !s.authority.unavailable
                && !s.authority.saving
                && (!s.narrow || !s.authority.detail)
                && from != to
                && s.authority.rows.iter().any(|r| &r.name == from)
                && s.authority.rows.iter().any(|r| &r.name == to)
        }
        Action::Submit(intent) if *intent > 0 => {
            s.authority.form
                && s.authority.save_intent == *intent
                && !s.authority.unavailable
                && !s.authority.saving
                && s.authority.path
                && form_ready(s)
        }
        _ => controls(s)
            .iter()
            .any(|item| &item.action == action && item.enabled),
    }
}
fn provider_visible(section: usize, open: bool) -> bool {
    section == 0 && open
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: &Action) -> bool {
    current.visible
        && observed.visible
        && current.id.matches(&observed.id)
        && current.version.matches(&observed.version)
        && current.authority == observed.authority
        && match (&current.editor, &observed.editor) {
            (None, None) => true,
            (Some(c), Some(o)) => c.draft.session.matches(&o.draft.session) && c.values == o.values,
            _ => false,
        }
        && allowed(current, action)
}

impl SettingsView {
    pub(in crate::settings) fn provider_surface_focus_handle(
        &self,
        cx: &App,
    ) -> Option<gpui_kit::FocusHandle> {
        self.provider_management
            .presentation
            .as_ref()
            .map(|page| page.surface.focus_handle(cx))
    }

    fn provider_controller_fence(&self, cx: &App) -> ControllerFence {
        let mut inputs: Vec<_> = [
            &self.name_input,
            &self.base_url_input,
            &self.models_input,
            &self.key_input,
        ]
        .into_iter()
        .map(|i| (i.entity_id().as_u64(), i.read(cx).content_revision()))
        .collect();
        let request = if let Some(editor) = &self.provider_management.model_editor {
            inputs.extend(
                [
                    &editor.model_input,
                    &editor.input_limit,
                    &editor.output_limit,
                ]
                .into_iter()
                .map(|i| (i.entity_id().as_u64(), i.read(cx).content_revision())),
            );
            editor.request_id
        } else {
            0
        };
        ControllerFence {
            providers: self.config.providers.clone(),
            base: self.provider_management.form_base.clone(),
            inputs,
            request,
            pending: self.pending_section.map(|d| match d {
                super::super::SettingsDestination::Section(n) => (0, n),
                super::super::SettingsDestination::ModelSection(n) => (1, n),
                super::super::SettingsDestination::ProviderCommand => (2, 0),
            }),
            pending_close: self.pending_settings_close,
            config_path: self.config_path.clone(),
            pending_command: self.provider_management.pending_command.clone(),
        }
    }
    fn provider_snapshot(&self, cx: &App) -> Snapshot {
        let state = &self.provider_management;
        let page = state.presentation.as_ref();
        Snapshot {
            id: page.map(|p| p.id.clone()).unwrap_or_default(),
            version: page.map(|p| p.version.clone()).unwrap_or_default(),
            guard_focused: self.navigation_guard_control_focused(),
            visible: provider_visible(
                self.section,
                cx.try_global::<super::super::SettingsOpen>()
                    .is_some_and(|open| open.0),
            ),
            authority: Arc::new(Authority {
                rows: self
                    .config
                    .providers
                    .iter()
                    .map(|p| ProviderRow {
                        name: p.name.clone(),
                        url: p.base_url.clone(),
                        api: p.api,
                        enabled: p.enabled,
                        credential: self.available_key_refs.contains(&p.key_ref),
                        models: p
                            .models
                            .iter()
                            .map(|m| (m.clone(), self.model_capability_summary(&p.name, m)))
                            .collect(),
                    })
                    .collect(),
                selected: state.selected.clone(),
                route: self.model_settings.clone(),
                generation: state.generation,
                operation: state.operation,
                section: self.section,
                form: state.form,
                existing: state.form_base.is_some(),
                retained_key: !self.key_input.read(cx).text().is_empty(),
                api: state.form_api,
                detail: state.detail_open || state.form,
                saving: self.provider_is_saving(),
                network: state.cancel.is_some(),
                path: self.config_path.is_some(),
                reload_needed: self.error.as_ref().is_some_and(|error| {
                    matches!(error, SettingsText::ProviderRead(_))
                        || error.is_key("settings.config.load_failed")
                }),
                candidates: state.candidates.clone(),
                checked: state.checked.iter().cloned().collect(),
                statuses: state
                    .statuses
                    .iter()
                    .map(|(k, v)| (k.clone(), v.render()))
                    .collect(),
                message: state.message.as_ref().map(SettingsText::render),
                delete: state.delete_model.clone(),
                model: state.model_editor.as_ref().map(|e| ModelState {
                    original: e.original.clone(),
                    source: e.source,
                    automatic: e.automatic,
                    unknown: e.unknown,
                    legacy: e.legacy_present,
                    renamed: e.renamed_from.clone(),
                    message: e.message.as_ref().map(SettingsText::render),
                    saving: e.saving,
                }),
                unavailable: self
                    .model_settings
                    .as_ref()
                    .is_some_and(|(provider, model)| {
                        !self
                            .config
                            .providers
                            .iter()
                            .any(|p| &p.name == provider && p.models.contains(model))
                    }),
                save_intent: state.save_intent,
                language: vega_i18n::language(),
            }),
            editor: state.nsh_draft.as_ref().map(|draft| Editor {
                draft: draft.clone(),
                values: draft.read(),
            }),
            colors: vega_theme::theme(cx).colors,
            narrow: page.is_some_and(|p| p.narrow),
            focus: page.and_then(|p| p.focus.clone()),
        }
    }
    pub(in crate::settings) fn reset_provider_nsh_draft(&mut self, cx: &App) {
        if let Some(draft) = self.provider_management.nsh_draft.take() {
            draft.retire();
        }
        let text = if let Some(editor) = &self.provider_management.model_editor {
            Some(
                [
                    &editor.model_input,
                    &editor.input_limit,
                    &editor.output_limit,
                ]
                .map(|i| i.read(cx).text().to_string()),
            )
        } else if self.provider_management.form {
            Some(
                [&self.name_input, &self.base_url_input, &self.models_input]
                    .map(|i| i.read(cx).text().to_string()),
            )
        } else {
            None
        };
        self.provider_management.draft_inputs =
            if let Some(editor) = &self.provider_management.model_editor {
                [
                    &editor.model_input,
                    &editor.input_limit,
                    &editor.output_limit,
                ]
                .map(|i| i.entity_id())
                .to_vec()
            } else if self.provider_management.form {
                [&self.name_input, &self.base_url_input, &self.models_input]
                    .map(|i| i.entity_id())
                    .to_vec()
            } else {
                Vec::new()
            };
        self.provider_management.nsh_draft = text.map(Draft::new);
        self.provider_management.consumed_secret_revision = None;
        self.provider_management.pending_capacity = None;
        self.provider_management.intent_stamp = None;
    }
    pub(super) fn sync_provider_editor_lifecycle(&mut self, cx: &App) {
        let inputs = if let Some(editor) = &self.provider_management.model_editor {
            [
                &editor.model_input,
                &editor.input_limit,
                &editor.output_limit,
            ]
            .map(|i| i.entity_id())
            .to_vec()
        } else if self.provider_management.form {
            [&self.name_input, &self.base_url_input, &self.models_input]
                .map(|i| i.entity_id())
                .to_vec()
        } else {
            Vec::new()
        };
        if inputs != self.provider_management.draft_inputs {
            self.reset_provider_nsh_draft(cx);
        }
    }
    pub(super) fn sync_loaded_provider_capacity(&mut self, _cx: &App) {
        if let (Some(capacity), Some(draft)) = (
            &self.provider_management.pending_capacity,
            &self.provider_management.nsh_draft,
        ) {
            let values = draft.read();
            if values.valid
                && !values.frozen
                && !values.composing.contains(&true)
                && draft.replace_capacity(capacity)
            {
                self.provider_management.pending_capacity = None;
            }
        }
    }
    pub(in crate::settings) fn provider_nsh_stamp(&self) -> Option<DraftStamp> {
        self.provider_management
            .nsh_draft
            .as_ref()
            .map(Draft::stamp)
    }
    pub(in crate::settings) fn provider_nsh_dirty(&self, cx: &App) -> bool {
        let Some(draft) = &self.provider_management.nsh_draft else {
            return false;
        };
        let values = draft.read();
        if !values.valid {
            return false;
        }
        if values.composing.contains(&true) || values.secret_present {
            return true;
        }
        if let Some(editor) = &self.provider_management.model_editor {
            return values.text
                != [
                    &editor.model_input,
                    &editor.input_limit,
                    &editor.output_limit,
                ]
                .map(|i| i.read(cx).text().to_string());
        }
        self.provider_management.form
            && (values.text
                != [&self.name_input, &self.base_url_input, &self.models_input]
                    .map(|i| i.read(cx).text().to_string()))
    }
    pub(in crate::settings) fn discard_provider_nsh_draft(&mut self, stamp: &DraftStamp) -> bool {
        let Some(draft) = &self.provider_management.nsh_draft else {
            return false;
        };
        if !draft.invalidate_stamp(stamp) {
            return false;
        }
        self.provider_management.nsh_draft = None;
        true
    }
    pub(in crate::settings) fn discard_provider_nsh_observed(
        &mut self,
        stamp: &DraftStamp,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.discard_provider_nsh_draft(stamp) {
            return false;
        }
        self.key_input
            .update(cx, crate::text_input::TextInput::clear);
        if let Some((_, model)) = self.model_settings.clone() {
            self.provider_management.form = false;
            self.provider_management.model_editor = None;
            self.provider_command(Command::EditModel(model), cx);
        } else {
            self.provider_management.form = false;
            self.sync_provider_editor_lifecycle(cx);
        }
        cx.notify();
        true
    }
    pub(in crate::settings) fn provider_nsh_submission_claimed(&self) -> bool {
        self.provider_management.submission_claimed
    }
    pub(in crate::settings) fn provider_form_save_unavailable(&self, cx: &App) -> bool {
        self.provider_management.form && !allowed(&self.provider_snapshot(cx), &Action::Submit(0))
    }

    pub(in crate::settings) fn request_provider_nsh_save_observed(
        &mut self,
        stamp: &DraftStamp,
        cx: &mut Context<Self>,
    ) -> bool {
        if stamp.composing()
            || self.provider_nsh_stamp().as_ref() != Some(stamp)
            || self.provider_is_saving()
        {
            return false;
        }
        self.request_provider_nsh_save(cx);
        true
    }
    pub(in crate::settings) fn request_provider_nsh_save(&mut self, cx: &mut Context<Self>) {
        if self.provider_is_saving() {
            return;
        }
        if self.provider_management.form {
            let intent_fence = self.provider_controller_fence(cx);
            if let Some(page) = &mut self.provider_management.presentation {
                page.intent_fence = Some(intent_fence);
            }
            self.provider_management.intent_stamp = self.provider_nsh_stamp();
            self.provider_management.save_intent =
                self.provider_management.save_intent.wrapping_add(1).max(1);
            self.sync_provider_presentation(cx);
            cx.notify();
        } else {
            let snapshot = self.provider_snapshot(cx);
            let action = Action::Command(Command::SaveModel);
            if allowed(&snapshot, &action) {
                self.apply_provider_decision(snapshot, action, None, cx);
            }
        }
    }
    pub(in crate::settings) fn sync_provider_presentation(&mut self, cx: &mut Context<Self>) {
        self.sync_provider_editor_lifecycle(cx);
        self.sync_loaded_provider_capacity(cx);
        if !self.provider_is_saving()
            && let Some(draft) = &self.provider_management.nsh_draft
        {
            draft.release();
        }
        let fence = self.provider_controller_fence(cx);
        if let Some(page) = &mut self.provider_management.presentation
            && page.fence != fence
        {
            page.fence = fence;
            page.version = ProjectionId::new();
        }
        let snapshot = self.provider_snapshot(cx);
        if let Some(page) = &self.provider_management.presentation {
            page.surface.publish(snapshot);
        }
    }
    fn apply_provider_decision(
        &mut self,
        observed: Snapshot,
        action: Action,
        secret: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .provider_management
            .presentation
            .as_ref()
            .is_some_and(|page| page.fence != self.provider_controller_fence(cx))
            || !accepts(&self.provider_snapshot(cx), &observed, &action)
        {
            return false;
        }
        let needs_values = matches!(
            action,
            Action::Submit(_)
                | Action::Api(_)
                | Action::Command(
                    Command::SaveModel
                        | Command::ToggleModelAutomatic
                        | Command::ToggleModelUnknown
                )
        );
        if needs_values {
            if let Action::Submit(intent) = action
                && intent > 0
                && (self.provider_management.intent_stamp != self.provider_nsh_stamp()
                    || self
                        .provider_management
                        .presentation
                        .as_ref()
                        .and_then(|page| page.intent_fence.as_ref())
                        != Some(&self.provider_controller_fence(cx)))
            {
                return false;
            }
            let Some(editor) = observed.editor.as_ref() else {
                return false;
            };
            if matches!(action, Action::Submit(_))
                && secret
                    .as_ref()
                    .is_none_or(|key| !key.is_empty() != editor.values.secret_present)
            {
                return false;
            }
            let Some(values) = editor.draft.claim(&editor.values) else {
                return false;
            };
            let inputs = if let Some(model) = &self.provider_management.model_editor {
                [
                    model.model_input.clone(),
                    model.input_limit.clone(),
                    model.output_limit.clone(),
                ]
            } else {
                [
                    self.name_input.clone(),
                    self.base_url_input.clone(),
                    self.models_input.clone(),
                ]
            };
            for (input, value) in inputs.into_iter().zip(values) {
                if input.read(cx).text() != value {
                    input.update(cx, |input, cx| input.set_text(&value, cx));
                }
            }
            if matches!(action, Action::Submit(_)) {
                let Some(secret) = secret else {
                    editor.draft.release();
                    return false;
                };
                if !secret.is_empty()
                    || self.provider_management.consumed_secret_revision
                        != Some(editor.values.secret_revision)
                {
                    self.key_input
                        .update(cx, |input, cx| input.set_text(&secret, cx));
                }
                self.provider_management.consumed_secret_revision =
                    Some(editor.values.secret_revision);
                editor.draft.consume_secret();
                self.provider_management.submission_claimed = true;
                self.submit_provider(cx);
                self.provider_management.submission_claimed = false;
            } else if let Action::Api(api) = action {
                self.provider_management.form_api = api;
            } else if let Action::Command(command) = action {
                self.provider_command(command, cx);
            }
            if !self.provider_is_saving() {
                editor.draft.release();
            }
        } else {
            let claimed = if let Some(editor) = observed.editor.as_ref() {
                if !matches!(action, Action::Escape) {
                    if editor.draft.claim(&editor.values).is_none() {
                        return false;
                    }
                    Some(editor.draft.clone())
                } else {
                    None
                }
            } else {
                None
            };
            match action {
                Action::Escape => {
                    if self.provider_has_draft(cx) {
                        self.provider_management.pending_command = Some(Command::Cancel);
                        self.pending_section =
                            Some(super::super::SettingsDestination::ProviderCommand);
                    } else {
                        self.provider_command(Command::Cancel, cx);
                    }
                }
                Action::Command(command) => self.provider_command(command, cx),
                Action::Move(from, to) => {
                    let names: Vec<_> = self.config.providers.iter().map(|p| &p.name).collect();
                    let (Some(from), Some(to)) = (
                        names.iter().position(|p| **p == from),
                        names.iter().position(|p| **p == to),
                    ) else {
                        return false;
                    };
                    self.move_provider(from, to, cx);
                }
                _ => return false,
            }
            if !self.provider_is_saving()
                && let Some(draft) = claimed
            {
                draft.release();
            }
        }
        cx.notify();
        true
    }
    pub(super) fn render_provider_surface(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        self.sync_provider_editor_lifecycle(cx);
        if self.provider_management.presentation.is_none() {
            let snapshot = self.provider_snapshot(cx);
            let id = snapshot.id.clone();
            let version = snapshot.version.clone();
            let fence = self.provider_controller_fence(cx);
            let (surface, mut requests) = Surface::new_transparent(snapshot, render, window, cx);
            self.provider_management.presentation = Some(Presentation {
                surface,
                id,
                version,
                fence,
                height: Layout::SETTINGS_CONTROL_MIN_HEIGHT,
                narrow: super::super::motion::narrow(window),
                focus: None,
                intent_fence: None,
            });
            cx.spawn(async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update(cx, |this, cx| {
                            this.sync_provider_presentation(cx);
                            match request {
                                Request::Changed => {
                                    cx.notify();
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.provider_management.presentation
                                        && (page.height - height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                                Request::Decide(command) => {
                                    let accepted = this.apply_provider_decision(
                                        command.observed,
                                        command.action,
                                        command.secret,
                                        cx,
                                    );
                                    let _ = command.reply.send(accepted);
                                }
                            }
                            this.sync_provider_presentation(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        if let Some(page) = &mut self.provider_management.presentation {
            page.narrow = super::super::motion::narrow(window);
            if let Some(target) = self.provider_management.return_focus.take() {
                let key = match target {
                    super::super::motion::ReturnFocus::ProviderList { provider, index } => provider
                        .filter(|name| self.config.providers.iter().any(|p| &p.name == name))
                        .or_else(|| {
                            self.config
                                .providers
                                .get(index.min(self.config.providers.len().saturating_sub(1)))
                                .map(|p| p.name.clone())
                        })
                        .map(|p| format!("provider:{p}"))
                        .unwrap_or_else(|| "add".into()),
                    super::super::motion::ReturnFocus::Model { provider, model } => {
                        format!("model:{provider}:{model}")
                    }
                };
                let serial = page.focus.as_ref().map_or(1, |(n, _)| n.wrapping_add(1));
                page.focus = Some((serial, key));
            }
            page.surface.sync_environment(cx);
        }
        self.sync_provider_presentation(cx);
        let Some(page) = &self.provider_management.presentation else {
            return gpui_kit::div().into_any_element();
        };
        if self.section == 0 && self.section_focuses[0].is_focused(window) {
            page.surface.focus(window, cx);
        }
        if self.provider_management.model_editor.is_some() {
            gpui_kit::div()
                .w_full()
                .flex_shrink_0()
                .h(gpui_kit::px(page.height))
                .child(page.surface.entity())
                .into_any_element()
        } else {
            gpui_kit::div()
                .w_full()
                .h_full()
                .flex_1()
                .min_h_0()
                .child(page.surface.entity())
                .into_any_element()
        }
    }
}

#[derive(Clone, Copy)]
struct Actions {
    owner: nsh_core::element::Owner,
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
    edits: Signal<u64>,
    popup: Signal<bool>,
}
impl Actions {
    fn snapshot(self) -> Snapshot {
        self.edits.get_untracked();
        let mut s = self.state.get_untracked();
        if let Some(editor) = &mut s.editor {
            editor.values = editor.draft.read();
        }
        s
    }
    fn changed(self, commands: &mpsc::Sender<Request>) {
        self.edits
            .update(|revision| *revision = revision.wrapping_add(1));
        let commands = commands.clone();
        spawn(async move {
            if commands.send(Request::Changed).await.is_err() {
                self.failed.set(true);
            }
        });
    }
    fn send(
        self,
        commands: mpsc::Sender<Request>,
        authority: Arc<Authority>,
        version: ProjectionId,
        action: Action,
        secret: Option<Signal<String>>,
    ) {
        let mut observed = self.snapshot();
        observed.authority = authority;
        observed.version = version;
        if self.pending.get_untracked() || !observed.visible || !allowed(&observed, &action) {
            return;
        }
        let value = secret.map(|s| s.get_untracked());
        self.pending.set(true);
        self.failed.set(false);
        self.owner.run(|| {
            spawn(async move {
                let (reply, response) = oneshot::channel();
                let accepted = commands
                    .send(Request::Decide(Box::new(Decision {
                        observed,
                        action,
                        secret: value,
                        reply,
                    })))
                    .await
                    .is_ok()
                    && matches!(response.await, Ok(true));
                if accepted && let Some(secret) = secret {
                    secret.try_set(String::new());
                }
                self.failed.set(!accepted);
                self.pending.set(false);
            });
        });
    }
}
fn field_row(
    state: Signal<Snapshot>,
    key: &'static str,
    description: Option<String>,
    control: AnyElement,
) -> AnyElement {
    let mut label = view()
        .w(px(Layout::SETTINGS_LABEL_WIDTH))
        .max_w(relative(1.))
        .flex_shrink_0()
        .py_1()
        .gap_1()
        .child(text(move || state.with(|_| t(key))));
    if let Some(description) = description {
        label = label.child(
            text(description)
                .text_size(px(Typography::METADATA))
                .text_color(move || crate::nsh::color(state.get().colors.text_secondary)),
        );
    }
    view()
        .w_full()
        .min_w(px(0.))
        .flex_shrink_0()
        .flex_row()
        .flex_wrap()
        .items_start()
        .gap_x(px(Layout::SETTINGS_FIELD_GAP))
        .gap_y_2()
        .py(px(Layout::SETTINGS_ROW_PADDING_Y))
        .border_b_1()
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .child(label)
        .child(
            view()
                .flex_1()
                .basis(px(220.))
                .min_w(px(0.))
                .max_w(relative(1.))
                .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                .justify_center()
                .child(control),
        )
        .into_element()
}

fn heading(label: impl Into<String>) -> AnyElement {
    text(label.into())
        .text_size(px(Typography::HEADING_BLOCK))
        .font_weight(FontWeight(Typography::HEADING_BLOCK_WEIGHT.0 as u16))
        .into_element()
}

fn icon(kind: crate::icons::Icon, state: Signal<Snapshot>) -> AnyElement {
    let source = crate::icons::nsh_source(kind).unwrap_or_default();
    svg(nsh_core::element::ImageSrc::svg_markup(&source))
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .tint_color(move || crate::nsh::color(state.get().colors.text_secondary).to_hsla())
        .into_element()
}

fn button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    action: Action,
    focus_key: Option<String>,
    secret: Option<Signal<String>>,
) -> AnyElement {
    let primary = matches!(
        &action,
        Action::Submit(_) | Action::Command(Command::SaveModel)
    );

    let quiet = matches!(
        &action,
        Action::Command(Command::Add | Command::PriceLibrary)
    ) || (matches!(&action, Action::Command(Command::Cancel))
        && actions.state.get_untracked().authority.form);
    let rendered = computed(move || {
        actions.state.get();
        actions.edits.get();
        actions.snapshot()
    });
    let control_action = action.clone();
    let item = computed(move || {
        controls(&rendered.get())
            .into_iter()
            .find(|i| i.action == control_action)
            .unwrap_or_else(|| Control {
                label: String::new(),
                action: control_action.clone(),
                enabled: false,
                selected: None,
                reason: String::new(),
            })
    });
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    if let Some(key) = focus_key {
        let requested = computed(move || actions.state.get().focus);
        effect(move || {
            if requested.get().as_ref().is_some_and(|(_, k)| *k == key) {
                target.focus();
            }
        });
    }
    let control = view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(move || item.get().label)
        .hint(move || item.get().reason)
        .accessibility_state(move || AccessibilityState {
            selected: item.get().selected == Some(true),
            checked: item.get().selected.map(|v| {
                if v {
                    nsh_core::Checked::True
                } else {
                    nsh_core::Checked::False
                }
            }),
            ..Default::default()
        })
        .disabled(move || actions.pending.get() || !item.get().enabled)
        .self_start()
        .flex_row()
        .items_center()
        .justify_center()
        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .px(px(Layout::SETTINGS_CONTROL_PADDING_X))
        .py_1()
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                if quiet {
                    actions.state.get().colors.bg_base.opacity(0.)
                } else {
                    actions.state.get().colors.border_subtle
                }
            })
        })
        .bg(move || {
            let colors = actions.state.get().colors;
            crate::nsh::color(if primary {
                if actions.pending.get() || !item.get().enabled {
                    colors.bg_hover
                } else {
                    colors.brand_primary
                }
            } else if item.get().selected == Some(true) {
                colors.bg_active
            } else if quiet {
                colors.bg_base.opacity(0.)
            } else {
                colors.bg_elevated
            })
        })
        .text_color(move || {
            let colors = actions.state.get().colors;
            crate::nsh::color(if primary {
                if actions.pending.get() || !item.get().enabled {
                    colors.text_tertiary
                } else {
                    colors.brand_on_accent
                }
            } else {
                colors.text_primary
            })
        })
        .tooltip(text(move || item.get().reason))
        .on_press(move || {
            let s = rendered.get_untracked();
            actions.send(
                commands.clone(),
                s.authority,
                s.version,
                action.clone(),
                secret,
            );
        });
    control.child(text(move || item.get().label)).into_element()
}

fn field(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    draft: Draft,
    index: usize,
    key: &'static str,
    secret: Option<Signal<String>>,
    readonly: bool,
) -> AnyElement {
    let value = secret.unwrap_or_else(|| signal(draft.read().text[index].clone()));
    let form = actions.state.get_untracked().authority.form;
    let multiline = index == 2 && form;
    let placeholder = match (form, index) {
        (true, 1) => "settings.field.base_url",
        (true, 2) => "settings.provider.models_placeholder",
        (false, 1) => "settings.provider.input_limit_placeholder",
        (false, 2) => "settings.provider.output_limit_placeholder",
        _ => key,
    };
    let line_height = Typography::BODY * Typography::BODY_LINE_HEIGHT;
    let min_height = line_height * crate::settings::state::PROVIDER_MODELS_MIN_ROWS as f32;
    let max_height = line_height * crate::settings::state::PROVIDER_MODELS_MAX_ROWS as f32;
    let content_height = signal(
        (value.get_untracked().split('\n').count() as f32 * line_height)
            .clamp(min_height, max_height),
    );
    let sync = draft.clone();
    effect(move || {
        actions.state.get();
        actions.edits.get();
        if secret.is_none() {
            let latest = sync.read();
            value.set(latest.text[index].clone());
        }
    });
    let editing = draft.clone();
    let composing = draft.clone();
    let composition_commands = commands.clone();
    let submit_commands = commands.clone();
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    field_row(
        actions.state,
        key,
        secret.map(|_| t("settings.provider.keep_key").to_string()),
        nsh_base::input()
            .element_ref(&target)
            .label(move || actions.state.with(|_| t(key)))
            .placeholder(move || actions.state.with(|_| t(placeholder).to_owned()))
            .value(move || value.get())
            .masked(secret.is_some())
            .hint(move || {
                if actions
                    .state
                    .get()
                    .authority
                    .model
                    .as_ref()
                    .is_some_and(|m| m.source == PolicySource::Loading)
                {
                    t("settings.provider.context_loading")
                } else {
                    ""
                }
            })
            .multiline(multiline)
            .on_content_size_change(move |size: &Size<Points>| {
                if multiline && size.height.0.is_finite() {
                    content_height.set(size.height.0.clamp(min_height, max_height));
                }
            })
            .readonly(move || {
                readonly
                    || actions.state.get().authority.unavailable
                    || actions.pending.get()
                    || actions.state.get().authority.saving
                    || (index > 0
                        && secret.is_none()
                        && actions
                            .state
                            .get()
                            .authority
                            .model
                            .as_ref()
                            .is_some_and(|m| m.source == PolicySource::Loading))
                    || {
                        actions.edits.get();
                        let v = draft.read();
                        v.frozen || !v.valid
                    }
            })
            .w_full()
            .min_w(px(0.))
            .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
            .h(move || {
                if multiline {
                    Length::from(px(content_height.get() + 18.))
                } else {
                    Length::Auto
                }
            })
            .line_height(px(line_height))
            .px_3()
            .py(px(if multiline { 8. } else { 4. }))
            .border(px(1.))
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .border_color(move || {
                crate::nsh::color(if focused.get() && !actions.state.get().guard_focused {
                    actions.state.get().colors.accent
                } else {
                    actions.state.get().colors.border_subtle
                })
            })
            .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
            .on_submit(move |_: &str| {
                let s = actions.snapshot();
                if index == 0 && s.authority.model.is_some() {
                    actions.send(
                        submit_commands.clone(),
                        s.authority,
                        s.version,
                        Action::Command(Command::SaveModel),
                        None,
                    );
                }
            })
            .on_change(move |text: &str| {
                if editing.edit(index, Some(text), None) {
                    value.set(text.into());
                    actions.changed(&commands);
                }
            })
            .on_composition_change(move |composition: &Composition| {
                if composing.edit(index, None, Some(composition.composing)) {
                    actions.changed(&composition_commands);
                }
            })
            .into_element(),
    )
}
fn help(actions: Actions) -> AnyElement {
    cleanup(move || actions.popup.set(false));
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    let trigger = view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(move || {
            actions
                .state
                .with(|_| t("settings.provider.test_help_label"))
        })
        .hint(move || actions.state.with(|_| super::provider_test_help_copy()))
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .size(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .self_start()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .tooltip(text(move || {
            actions.state.with(|_| super::provider_test_help_copy())
        }))
        .child(icon(crate::icons::Icon::Help, actions.state));
    nsh_base::popover()
        .on_open_change(move |event: &nsh_base::PopoverEvent| {
            if let nsh_base::PopoverEvent::OpenChange(value, _) = event {
                actions.popup.set(*value);
            }
        })
        .trigger(trigger)
        .child(text(move || {
            actions.state.with(|_| super::provider_test_help_copy())
        }))
        .into_element()
}

fn model_basics(actions: Actions, commands: mpsc::Sender<Request>, editor: Editor) -> AnyElement {
    let state = actions.state;
    let capacity_commands = commands.clone();
    let capacity_draft = editor.draft.clone();
    let options = commands.clone();
    view()
        .w_full()
        .gap_3()
        .child(field(
            actions,
            commands.clone(),
            editor.draft,
            0,
            "settings.field.model_id",
            None,
            false,
        ))
        .child(when(
            move || {
                state
                    .get()
                    .authority
                    .model
                    .as_ref()
                    .is_some_and(|m| m.original.is_some())
            },
            move || {
                let commands = capacity_commands.clone();
                let input_draft = capacity_draft.clone();
                view()
                    .gap_3()
                    .child(text(move || {
                        state
                            .get()
                            .authority
                            .model
                            .as_ref()
                            .map(|model| match model.source {
                                PolicySource::Loading => {
                                    t("settings.provider.context_loading").into()
                                }
                                PolicySource::AssumedDefault => t_args(
                                    "settings.provider.context_assumed",
                                    &[
                                        ("input", &super::DEFAULT_MODEL_INPUT_LIMIT.to_string()),
                                        (
                                            "output",
                                            &super::DEFAULT_MODEL_OUTPUT_RESERVE.to_string(),
                                        ),
                                    ],
                                ),
                                PolicySource::Saved => {
                                    t("settings.provider.context_saved_hint").into()
                                }
                                PolicySource::Unknown => {
                                    t("settings.provider.context_unknown_hint").into()
                                }
                            })
                            .unwrap_or_default()
                    }))
                    .child(when(
                        move || {
                            state
                                .get()
                                .authority
                                .model
                                .as_ref()
                                .is_some_and(|m| m.legacy)
                        },
                        move || text(t("settings.provider.context_legacy")),
                    ))
                    .child(text(move || {
                        state
                            .get()
                            .authority
                            .model
                            .as_ref()
                            .and_then(|m| m.renamed.as_ref())
                            .map(|previous| {
                                t_args(
                                    "settings.provider.context_renamed",
                                    &[("previous", previous)],
                                )
                            })
                            .unwrap_or_default()
                    }))
                    .child(when(
                        move || {
                            state
                                .get()
                                .authority
                                .model
                                .as_ref()
                                .is_some_and(|m| !m.unknown)
                        },
                        move || {
                            view()
                                .gap_3()
                                .child(field(
                                    actions,
                                    commands.clone(),
                                    input_draft.clone(),
                                    1,
                                    "settings.provider.input_limit",
                                    None,
                                    false,
                                ))
                                .child(field(
                                    actions,
                                    commands.clone(),
                                    input_draft.clone(),
                                    2,
                                    "settings.provider.output_limit",
                                    None,
                                    false,
                                ))
                                .child(text(t("settings.provider.tokens_unit")))
                        },
                    ))
                    .child(button(
                        actions,
                        options.clone(),
                        Action::Command(Command::ToggleModelAutomatic),
                        None,
                        None,
                    ))
                    .child(button(
                        actions,
                        options.clone(),
                        Action::Command(Command::ToggleModelUnknown),
                        None,
                        None,
                    ))
            },
        ))
        .child(text(move || {
            state
                .get()
                .authority
                .model
                .as_ref()
                .and_then(|m| m.message.clone())
                .unwrap_or_default()
        }))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::SaveModel),
                    None,
                    None,
                ))
                .child(button(
                    actions,
                    commands,
                    Action::Command(Command::Cancel),
                    None,
                    None,
                )),
        )
        .into_element()
}

fn form(actions: Actions, commands: mpsc::Sender<Request>, editor: Editor) -> AnyElement {
    let secret = signal(String::new());
    let state = actions.state;
    let root = element_ref();
    let within = signal(false);
    root.on_focus_in(move || within.set(true));
    root.on_focus_out(move || within.set(false));
    let keyboard_commands = commands.clone();
    let intent_seen = signal(state.get_untracked().authority.save_intent);
    let save_commands = commands.clone();
    effect(move || {
        state.get();
        let s = actions.snapshot();
        let intent = s.authority.save_intent;
        if intent != intent_seen.get_untracked() {
            intent_seen.set(intent);
            if s.visible && !actions.pending.get_untracked() {
                actions.send(
                    save_commands.clone(),
                    s.authority.clone(),
                    s.version.clone(),
                    Action::Submit(intent),
                    Some(secret),
                );
            }
        }
    });
    let back_commands = commands.clone();
    let mut body = view()
        .w_full()
        .min_w(px(0.))
        .flex_shrink_0()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .border(px(1.))
        .rounded(px(Layout::PANEL_RADIUS))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .bg(move || crate::nsh::color(state.get().colors.bg_subtle))
        .child(when(
            move || {
                within.get()
                    && actions.state.get().visible
                    && actions
                        .state
                        .get()
                        .editor
                        .as_ref()
                        .is_some_and(|e| !e.draft.read().composing.contains(&true))
            },
            move || {
                let commands = keyboard_commands.clone();
                on_key_with(
                    "mod+enter",
                    KeyOptions {
                        when: nsh_core::keymap::When::Always,
                        ..Default::default()
                    },
                    move |_| {
                        let s = actions.snapshot();
                        actions.send(
                            commands.clone(),
                            s.authority,
                            s.version,
                            Action::Submit(0),
                            Some(secret),
                        );
                    },
                );
                view()
            },
        ))
        .child(when(
            move || state.get().narrow,
            move || {
                button(
                    actions,
                    back_commands.clone(),
                    Action::Command(Command::BackToList),
                    None,
                    None,
                )
            },
        ))
        .child(view().child(heading(t("settings.provider.add_update"))))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Api(ProviderApi::ChatCompletions),
                    None,
                    None,
                ))
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Api(ProviderApi::Responses),
                    None,
                    None,
                )),
        );
    body = body
        .child(field(
            actions,
            commands.clone(),
            editor.draft.clone(),
            0,
            "settings.field.name",
            None,
            false,
        ))
        .child(field(
            actions,
            commands.clone(),
            editor.draft.clone(),
            1,
            "settings.provider.service_url",
            None,
            false,
        ))
        .child(field(
            actions,
            commands.clone(),
            editor.draft.clone(),
            2,
            "settings.field.model_id",
            None,
            false,
        ))
        .child(
            text(move || {
                state.with(|_| {
                    t_args(
                        "settings.provider.models_hint",
                        &[("limit", &super::super::PROVIDER_MODEL_COUNT_MAX.to_string())],
                    )
                })
            })
            .text_size(px(Typography::BODY))
            .line_height(px((Typography::BODY * Typography::BODY_LINE_HEIGHT).round()))
            .text_color(move || crate::nsh::color(state.get().colors.text_tertiary)),
        )
        .child(
            text(move || state.with(|_| t("settings.provider.responses_hint")))
                .text_size(px(Typography::BODY))
                .line_height(px((Typography::BODY * Typography::BODY_LINE_HEIGHT).round()))
                .text_color(move || crate::nsh::color(state.get().colors.text_tertiary)),
        )
        .child(field(
            actions,
            commands.clone(),
            editor.draft,
            3,
            "settings.field.api_key",
            Some(secret),
            false,
        ))
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Submit(0),
                    None,
                    Some(secret),
                )),
        );
    view()
        .element_ref(&root)
        .w_full()
        .min_w_0()
        .gap(px(Layout::SETTINGS_CONTENT_GAP))
        .child(body)
        .child(button(
            actions,
            commands,
            Action::Command(Command::Cancel),
            None,
            None,
        ))
        .into_element()
}
fn menu(actions: Actions, commands: mpsc::Sender<Request>, model: Option<String>) -> AnyElement {
    let open = signal(false);
    cleanup(move || actions.popup.set(false));
    let label = if model.is_some() {
        t("settings.provider.model_actions")
    } else {
        t("settings.provider.actions")
    };
    let title = model
        .as_ref()
        .map(|m| format!("{label}: {m}"))
        .unwrap_or_else(|| label.into());
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    let trigger = view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .border(px(1.))
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .size(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
        .self_start()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .label(title.clone())
        .tooltip(text(title))
        .child(icon(crate::icons::Icon::More, actions.state));
    let mut content = view().gap_2().p_2();
    if let Some(model) = model {
        content = content
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(button(
                        actions,
                        commands.clone(),
                        Action::Command(Command::Test(model.clone())),
                        None,
                        None,
                    ))
                    .child(help(actions)),
            )
            .child(button(
                actions,
                commands,
                Action::Command(Command::ConfirmDelete(model)),
                None,
                None,
            ));
    } else {
        content = content
            .child(button(
                actions,
                commands.clone(),
                Action::Command(Command::Up),
                None,
                None,
            ))
            .child(button(
                actions,
                commands,
                Action::Command(Command::Down),
                None,
                None,
            ));
    }
    content = content.child(
        view()
            .role(AccessibilityRole::Button)
            .focusable(true)
            .label(t("settings.provider.close_menu"))
            .on_press(move || {
                open.set(false);
                actions.popup.set(false);
            })
            .child(text(t("settings.provider.close_menu"))),
    );
    nsh_base::popover()
        .open(move || Some(open.get()))
        .on_open_change(move |event: &nsh_base::PopoverEvent| {
            if let nsh_base::PopoverEvent::OpenChange(value, _) = event {
                open.set(*value);
                actions.popup.set(*value);
            }
        })
        .trigger(trigger)
        .child(content)
        .into_element()
}
fn provider_detail(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    row: ProviderRow,
) -> AnyElement {
    let s = actions.snapshot();
    let a = &s.authority;
    let mut body = view().w_full().min_w(px(0.)).gap_3();
    if s.narrow {
        body = body.child(button(
            actions,
            commands.clone(),
            Action::Command(Command::BackToList),
            None,
            None,
        ));
    }
    body = body
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .items_center()
                .child(heading(row.name.clone()))
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::Enable),
                    None,
                    None,
                ))
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::Edit),
                    Some("edit".into()),
                    None,
                ))
                .child(menu(actions, commands.clone(), None)),
        )
        .child(
            view()
                .child(field_row(
                    actions.state,
                    "settings.provider.address",
                    None,
                    text(row.url).into_element(),
                ))
                .child(field_row(
                    actions.state,
                    "settings.provider.protocol",
                    None,
                    text(api_label(row.api)).into_element(),
                ))
                .child(field_row(
                    actions.state,
                    "settings.provider.credentials",
                    None,
                    text(if row.credential {
                        super::super::key_stored_placeholder()
                    } else {
                        t("settings.provider.key_required")
                    })
                    .into_element(),
                )),
        )
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .items_center()
                .child(heading(t("settings.field.model")))
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::Discover),
                    None,
                    None,
                ))
                .child(help(actions))
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::AddModel),
                    None,
                    None,
                )),
        );
    if a.network {
        body = body.child(button(
            actions,
            commands.clone(),
            Action::Command(Command::Cancel),
            None,
            None,
        ));
    }
    for (model, summary) in &row.models {
        let mut item = view()
            .w_full()
            .gap_1()
            .child(
                view()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(
                        text(model.clone())
                            .font_weight(FontWeight(Typography::HEADING_CARD_WEIGHT.0 as u16)),
                    )
                    .child(button(
                        actions,
                        commands.clone(),
                        Action::Command(Command::EditModel(model.clone())),
                        Some(format!("model:{}:{model}", row.name)),
                        None,
                    ))
                    .child(menu(actions, commands.clone(), Some(model.clone()))),
            )
            .child(
                text(summary.clone())
                    .text_size(px(Typography::METADATA))
                    .text_color(move || {
                        crate::nsh::color(actions.state.get().colors.text_secondary)
                    }),
            );
        if let Some((_, status)) = a.statuses.iter().find(|(name, _)| name == model) {
            item = item.child(text(status.clone()));
        }
        body = body.child(item);
    }
    if let Some((provider, model)) = &a.delete {
        body = body.child(
            view()
                .gap_2()
                .child(text(t_args(
                    "settings.provider.confirm_delete_model",
                    &[("provider", provider), ("model", model)],
                )))
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::DeleteModel(provider.clone(), model.clone())),
                    None,
                    None,
                ))
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::Cancel),
                    None,
                    None,
                )),
        );
    }
    if let Some(candidates) = &a.candidates {
        for model in candidates {
            body = body.child(button(
                actions,
                commands.clone(),
                Action::Command(Command::Candidate(model.clone())),
                None,
                None,
            ));
        }
        body = body.child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::Import),
                    None,
                    None,
                ))
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::Cancel),
                    None,
                    None,
                )),
        );
    }
    if let Some(message) = &a.message {
        body = body.child(text(message.clone()));
    }
    body.into_element()
}
fn provider_list(actions: Actions, commands: mpsc::Sender<Request>) -> AnyElement {
    let s = actions.snapshot();
    let geometry = signal(Vec::<(String, f32, f32)>::new());
    let dragged = signal(None::<String>);
    let mut list = view().w_full().gap_2().child(
        view()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .child(button(
                actions,
                commands.clone(),
                Action::Command(Command::Add),
                Some("add".into()),
                None,
            ))
            .children(s.authority.reload_needed.then(|| {
                button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::Reload),
                    None,
                    None,
                )
            })),
    );
    for row in &s.authority.rows {
        let name = row.name.clone();
        let layout_name = name.clone();
        let start_name = name.clone();
        let end_name = name.clone();
        let authority = s.authority.clone();
        let version = s.version.clone();
        let drag_commands = commands.clone();
        let active = signal(false);
        let handle = element_ref();
        let handle_focused = signal(false);
        handle.on_focus_in(move || handle_focused.set(true));
        handle.on_focus_out(move || handle_focused.set(false));
        let keyboard_name = name.clone();
        let keyboard_order: Vec<_> = s.authority.rows.iter().map(|r| r.name.clone()).collect();
        let keyboard_authority = s.authority.clone();
        let keyboard_version = s.version.clone();
        let keyboard_commands = commands.clone();
        let target_name = name.clone();
        list = list.child(
            view()
                .flex_row()
                .items_center()
                .gap_1()
                .on_layout(move |bounds: &Bounds<Points>| {
                    geometry.update(|rows| {
                        rows.retain(|(n, _, _)| *n != layout_name);
                        rows.push((layout_name.clone(), bounds.origin.y.0, bounds.size.height.0));
                    });
                })
                .child(
                    view()
                        .element_ref(&handle)
                        .role(AccessibilityRole::Button)
                        .focusable(true)
                        .child(when(
                            move || handle_focused.get(),
                            move || {
                                for (key, delta) in [("up", -1_isize), ("down", 1_isize)] {
                                    let order = keyboard_order.clone();
                                    let name = keyboard_name.clone();
                                    let authority = keyboard_authority.clone();
                                    let version = keyboard_version.clone();
                                    let commands = keyboard_commands.clone();
                                    on_key_with(
                                        key,
                                        KeyOptions {
                                            when: nsh_core::keymap::When::NotEditing,
                                            ..Default::default()
                                        },
                                        move |_| {
                                            if let Some(index) =
                                                order.iter().position(|p| *p == name)
                                                && let Some(to) = index
                                                    .checked_add_signed(delta)
                                                    .and_then(|i| order.get(i))
                                            {
                                                actions.send(
                                                    commands.clone(),
                                                    authority.clone(),
                                                    version.clone(),
                                                    Action::Move(name.clone(), to.clone()),
                                                    None,
                                                );
                                            }
                                        },
                                    );
                                }
                                view()
                            },
                        ))
                        .label(format!(
                            "{}: {} / {}",
                            name,
                            t("settings.provider.move_up"),
                            t("settings.provider.move_down")
                        ))
                        .hint(format!(
                            "↑ {} / ↓ {}",
                            t("settings.provider.move_up"),
                            t("settings.provider.move_down")
                        ))
                        .disabled(s.authority.saving)
                        .claim_touches(TouchClaim::on_pan(PanActivation::default()))
                        .keep_touches(true)
                        .on_responder_grant(move |_: &ResponderEvent| {
                            active.set(true);
                            dragged.set(Some(start_name.clone()));
                        })
                        .on_responder_release(move |event: &ResponderEvent| {
                            active.set(false);
                            dragged.set(None);
                            let rows = geometry.get_untracked();
                            let origin = rows
                                .iter()
                                .find(|(n, _, _)| *n == end_name)
                                .map(|(_, y, h)| y + h / 2.);
                            if let Some(y) = origin.map(|y| y + event.gesture.delta.y.0)
                                && let Some((to, _, _)) =
                                    rows.iter().min_by(|(_, ay, ah), (_, by, bh)| {
                                        (ay + ah / 2. - y)
                                            .abs()
                                            .total_cmp(&(by + bh / 2. - y).abs())
                                    })
                            {
                                actions.send(
                                    drag_commands.clone(),
                                    authority.clone(),
                                    version.clone(),
                                    Action::Move(end_name.clone(), to.clone()),
                                    None,
                                );
                            }
                        })
                        .on_responder_terminate(move |_: &ResponderEvent| {
                            active.set(false);
                            dragged.set(None);
                        })
                        .border(px(1.))
                        .border_color(move || {
                            crate::nsh::color(if handle_focused.get() {
                                actions.state.get().colors.accent
                            } else {
                                actions.state.get().colors.border_subtle
                            })
                        })
                        .bg(move || {
                            crate::nsh::color(
                                if active.get() || dragged.get().as_ref() == Some(&target_name) {
                                    actions.state.get().colors.bg_active
                                } else {
                                    actions.state.get().colors.bg_elevated
                                },
                            )
                        })
                        .min_h(px(Layout::SETTINGS_CONTROL_MIN_HEIGHT))
                        .px_2()
                        .child(text("↕")),
                )
                .child(button(
                    actions,
                    commands.clone(),
                    Action::Command(Command::Select(name.clone())),
                    Some(format!("provider:{name}")),
                    None,
                )),
        );
    }
    list.child(button(
        actions,
        commands,
        Action::Command(Command::PriceLibrary),
        None,
        None,
    ))
    .into_element()
}
fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        owner: nsh_core::element::Owner::current(),
        state,
        pending: signal(false),
        failed: signal(false),
        edits: signal(0),
        popup: signal(false),
    };
    let measured = signal(0.);
    let layout = commands.clone();
    let route_commands = commands.clone();
    let body_state = computed(move || {
        let snapshot = state.get();
        let mut authority = (*snapshot.authority).clone();
        authority.checked.clear();
        (authority, snapshot.narrow)
    });
    let root = element_ref();
    let within = signal(false);
    root.on_focus_in(move || within.set(true));
    root.on_focus_out(move || within.set(false));
    let escape_commands = commands.clone();
    view()
        .element_ref(&root)
        .w_full()
        .h(move || {
            if state.get().authority.model.is_some() {
                Length::Auto
            } else {
                Length::from(percentage(100.))
            }
        })
        .gap_3()
        .text_size(px(Typography::BODY))
        .line_height(px(Typography::BODY * Typography::BODY_LINE_HEIGHT))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .child(when(
            move || {
                within.get()
                    && !actions.popup.get()
                    && state.get().visible
                    && state
                        .get()
                        .editor
                        .as_ref()
                        .is_none_or(|e| !e.draft.read().composing.contains(&true))
            },
            move || {
                let commands = escape_commands.clone();
                on_key_with(
                    "escape",
                    KeyOptions {
                        when: nsh_core::keymap::When::Always,
                        ..Default::default()
                    },
                    move |_| {
                        let s = actions.snapshot();
                        if allowed(&s, &Action::Escape) {
                            actions.send(
                                commands.clone(),
                                s.authority,
                                s.version,
                                Action::Escape,
                                None,
                            );
                        }
                    },
                );
            },
        ))
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (measured.get_untracked() - height).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    let _ = commands.send(Request::Layout(height)).await;
                });
            }
        })
        .child(when(
            move || actions.failed.get(),
            move || text(move || state.with(|_| t("settings.config.conflict"))),
        ))
        .child(each(
            move || {
                state
                    .get()
                    .editor
                    .iter()
                    .map(|e| e.draft.key)
                    .collect::<Vec<_>>()
            },
            |key| *key,
            move |_, _| {
                let s = actions.state.get_untracked();
                let Some(editor) = s.editor.clone() else {
                    return view().into_element();
                };
                if s.authority.model.is_some() {
                    model_basics(actions, route_commands.clone(), editor)
                } else {
                    let list_commands = route_commands.clone();
                    view()
                        .w_full()
                        .h_full()
                        .flex_row()
                        .items_start()
                        .gap(px(Layout::SETTINGS_PROVIDER_COLUMN_GAP))
                        .child(when(
                            move || !state.get().narrow,
                            move || {
                                let commands = list_commands.clone();
                                dynamic(move || {
                                    body_state.get();
                                    view()
                                        .w(px(Layout::SETTINGS_PROVIDER_LIST_WIDTH))
                                        .h_full()
                                        .child(
                                            scroll()
                                                .size_full()
                                                .child(provider_list(actions, commands.clone())),
                                        )
                                })
                            },
                        ))
                        .child(scroll().h_full().flex_1().min_w(px(0.)).child(form(
                            actions,
                            route_commands.clone(),
                            editor,
                        )))
                        .into_element()
                }
            },
        ))
        .child(when(
            move || state.get().authority.unavailable,
            move || text(t("settings.capabilities.error.missing")),
        ))
        .child(when(
            move || {
                !state.get().authority.unavailable
                    && state.get().authority.model.is_none()
                    && !state.get().authority.form
            },
            move || {
                let commands = commands.clone();
                dynamic(move || {
                    body_state.get();
                    let s = actions.snapshot();
                    let mut body = view()
                        .w_full()
                        .h_full()
                        .flex_row()
                        .items_start()
                        .gap(px(Layout::SETTINGS_PROVIDER_COLUMN_GAP));
                    if !s.narrow || !s.authority.detail {
                        let list = scroll()
                            .h_full()
                            .child(provider_list(actions, commands.clone()));
                        body = body.child(if s.narrow {
                            list.w_full()
                        } else {
                            list.w(px(Layout::SETTINGS_PROVIDER_LIST_WIDTH))
                        });
                    }
                    if (!s.narrow || s.authority.detail)
                        && let Some(row) = selected_row(&s.authority)
                    {
                        body = body.child(
                            scroll()
                                .h_full()
                                .flex_1()
                                .min_w(px(0.))
                                .child(provider_detail(actions, commands.clone(), row.clone())),
                        );
                    }
                    body
                })
            },
        ))
        .into_element()
}
