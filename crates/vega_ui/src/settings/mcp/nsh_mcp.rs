use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{
    McpConnectionTest, McpOAuthDiscovery, McpOAuthPreparation, McpOAuthStepUpOffer,
    McpRemoteAuthorization, McpServerTransport, McpServerView,
};
use vega_theme::{Layout, ThemeColors, Typography};

use super::{
    McpAction, McpAuthChoice, McpConfirmation, McpEditor, McpOperation, McpTransportChoice,
    health_label, mcp_confirmation_lines, mcp_oauth_can_begin, mcp_oauth_preparation_lines,
};
use crate::nsh::{ProjectionId, Surface};
use crate::settings::SettingsView;

#[derive(Clone, PartialEq, Eq)]
struct Values {
    text: [String; 8],
    composing: [bool; 10],
    revision: u64,
    frozen: bool,
}

#[derive(Clone)]
pub(super) struct Draft {
    session: ProjectionId,
    values: Arc<Mutex<Values>>,
}
impl Draft {
    fn new(text: [String; 8]) -> Self {
        Self {
            session: ProjectionId::new(),
            values: Arc::new(Mutex::new(Values {
                text,
                composing: [false; 10],
                revision: 0,
                frozen: false,
            })),
        }
    }
    fn read(&self) -> Values {
        self.values
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    fn edit(&self, index: usize, text: Option<&str>, composing: Option<bool>) -> bool {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if values.frozen
            || index >= values.composing.len()
            || (text.is_some() && index >= values.text.len())
        {
            return false;
        }
        if let Some(text) = text {
            values.text[index] = text.into();
        }
        if let Some(composing) = composing {
            values.composing[index] = composing;
        }
        values.revision = values.revision.wrapping_add(1);
        true
    }
    fn claim(&self, observed: &Values) -> Option<[String; 8]> {
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *values != *observed || values.frozen || values.composing.contains(&true) {
            return None;
        }
        values.frozen = true;
        Some(values.text.clone())
    }
    pub(super) fn release(&self) {
        self.values
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .frozen = false;
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Authority {
    generation: u64,
    available: bool,
    busy: bool,
    loading: bool,
    revoking: bool,
    servers: Vec<McpServerView>,
    previews: HashMap<String, McpConnectionTest>,
    offers: Vec<McpOAuthStepUpOffer>,
    editor: Option<McpEditor>,
    confirmation: Option<McpConfirmation>,
    transport: McpTransportChoice,
    auth: McpAuthChoice,
    loopback: bool,
    discovery: Option<(String, u64, McpOAuthDiscovery)>,
    preparation: Option<McpOAuthPreparation>,
    flow: Option<String>,
    waiting: bool,
    expanded: HashSet<String>,
    message: Option<String>,
    message_error: bool,
}
#[derive(Clone)]
struct Editor {
    draft: Draft,
    observed: Values,
}
impl PartialEq for Editor {
    fn eq(&self, other: &Self) -> bool {
        self.draft.session.matches(&other.draft.session) && self.observed == other.observed
    }
}
#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    visible: bool,
    authority: Arc<Authority>,
    editor: Option<Editor>,
    colors: ThemeColors,
}
struct Decision {
    observed: Snapshot,
    action: McpAction,
    secret: Option<String>,
    reply: oneshot::Sender<bool>,
}
enum Request {
    Decide(Box<Decision>),
    Layout(f32),
}
pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone, PartialEq, Eq)]
struct Control {
    label: String,
    action: McpAction,
    enabled: bool,
    selected: Option<bool>,
}
#[derive(Clone, PartialEq, Eq)]
struct Block {
    key: String,
    title: String,
    lines: Vec<String>,
    warning: bool,
    controls: Vec<Control>,
}
fn block(key: impl Into<String>, title: impl Into<String>) -> Block {
    Block {
        key: key.into(),
        title: title.into(),
        lines: Vec::new(),
        warning: false,
        controls: Vec::new(),
    }
}
fn control(label: impl Into<String>, action: McpAction, enabled: bool) -> Control {
    Control {
        label: label.into(),
        action,
        enabled,
        selected: None,
    }
}
fn oauth(row: &McpServerView) -> bool {
    matches!(
        row.form.transport,
        McpServerTransport::Remote {
            authorization: McpRemoteAuthorization::OAuth { .. },
            ..
        }
    )
}

fn blocks(authority: &Authority) -> Vec<Block> {
    use vega_i18n::{t, t_args};
    let idle = authority.available && !authority.busy && authority.flow.is_none();
    let mut top = block("top", t("settings.mcp.title"));
    top.lines.push(t("settings.mcp.intro").into());
    top.controls = vec![
        control(
            t("settings.mcp.add_local"),
            McpAction::Add(McpTransportChoice::Local),
            idle,
        ),
        control(
            t("settings.mcp.add_remote"),
            McpAction::Add(McpTransportChoice::Remote),
            idle,
        ),
        control(t("settings.mcp.refresh"), McpAction::Reload, idle),
    ];
    if !authority.available {
        top.lines.push(t("settings.mcp.unavailable").into());
        top.warning = true;
    } else if authority.loading {
        top.lines.push(t("settings.mcp.loading").into());
    } else if authority.servers.is_empty() {
        top.lines.push(t("settings.mcp.empty").into());
    }
    if authority.busy && !authority.loading {
        top.lines.push(
            t(if authority.waiting {
                "settings.mcp.oauth.waiting_disabled"
            } else {
                "settings.mcp.processing"
            })
            .into(),
        );
    }
    let mut result = vec![top];
    if let Some(message) = &authority.message {
        let mut item = block("message", "");
        item.lines.push(message.clone());
        item.warning = authority.message_error;
        result.push(item);
    }
    if let Some(confirmation) = &authority.confirmation
        && let Some(row) = authority
            .servers
            .iter()
            .find(|row| row.id == confirmation.id())
    {
        let mut item = block("confirmation", t("settings.mcp.confirm.title"));
        item.warning = true;
        item.lines.push(row.form.display_name.clone());
        item.lines.extend(mcp_confirmation_lines(row));
        let (label, warning, revoke) = match confirmation {
            McpConfirmation::Enable { .. } => ("settings.mcp.confirm.enable", None, false),
            McpConfirmation::Test { .. } => ("settings.mcp.confirm.test", None, false),
            McpConfirmation::Remove { .. } => (
                "settings.mcp.confirm.remove",
                Some("settings.mcp.remove_warning"),
                true,
            ),
            McpConfirmation::DiscoverOAuth { .. } => (
                "settings.mcp.confirm.discover",
                Some("settings.mcp.discover_warning"),
                false,
            ),
            McpConfirmation::DisconnectOAuth { .. } => (
                "settings.mcp.confirm.disconnect",
                Some("settings.mcp.disconnect_warning"),
                true,
            ),
        };
        if let Some(warning) = warning {
            item.lines.push(t(warning).into());
        }
        let revision = match confirmation {
            McpConfirmation::Enable { revision, .. }
            | McpConfirmation::Test { revision, .. }
            | McpConfirmation::Remove { revision, .. }
            | McpConfirmation::DiscoverOAuth { revision, .. }
            | McpConfirmation::DisconnectOAuth { revision, .. } => *revision,
        };
        item.controls.push(control(
            t(label),
            McpAction::Confirm,
            authority.available
                && !row.deleting
                && row.config_revision == revision
                && (!authority.busy || revoke)
                && (!revoke || !authority.revoking),
        ));
        item.controls.push(control(
            t("settings.action.cancel"),
            McpAction::CancelConfirmation,
            authority.available,
        ));
        result.push(item);
    }
    if authority.waiting {
        let mut item = block("waiting", "");
        item.controls.push(control(
            t("settings.mcp.oauth.cancel_callback"),
            McpAction::CancelOAuth,
            authority.available && !authority.revoking,
        ));
        result.push(item);
    } else if let Some(preparation) = &authority.preparation {
        let step_up = !preparation.step_up_added_scopes.is_empty();
        let mut item = block(
            "preparation",
            t(if step_up {
                "settings.mcp.oauth.review_step_up"
            } else {
                "settings.mcp.oauth.review_scope"
            }),
        );
        item.warning = true;
        item.lines.extend(mcp_oauth_preparation_lines(preparation));
        item.lines
            .push(t("settings.mcp.oauth.consent_warning").into());
        if step_up
            || authority
                .discovery
                .as_ref()
                .and_then(|(id, _, _)| authority.servers.iter().find(|row| row.id == *id))
                .is_some_and(|row| row.credential_configured)
        {
            item.lines
                .push(t("settings.mcp.oauth.reauthorize_warning").into());
        }
        item.controls.push(control(
            t(if step_up {
                "settings.mcp.oauth.consent_step_up"
            } else {
                "settings.mcp.oauth.consent"
            }),
            McpAction::AuthorizeOAuth,
            authority.available && !authority.busy && mcp_oauth_can_begin(preparation.registration),
        ));
        item.controls.push(control(
            t("settings.mcp.oauth.cancel"),
            McpAction::CancelOAuth,
            authority.available && !authority.revoking,
        ));
        result.push(item);
    } else if let Some((id, revision, discovery)) = &authority.discovery {
        let mut item = block("discovery", t("settings.mcp.oauth.choose_server"));
        item.lines.push(t_args(
            "settings.mcp.oauth.protected_resource",
            &[("resource", &discovery.resource)],
        ));
        item.lines.push(t_args(
            "settings.mcp.oauth.requested_scopes",
            &[(
                "scopes",
                &if discovery.requested_scopes.is_empty() {
                    t("settings.mcp.scopes_undeclared").into()
                } else {
                    discovery
                        .requested_scopes
                        .join(t("settings.list_separator"))
                },
            )],
        ));
        item.lines
            .push(t("settings.mcp.oauth.choose_warning").into());
        if discovery.issuers.is_empty() {
            item.lines
                .push(t("settings.mcp.oauth.no_verified_issuer").into());
            item.warning = true;
        }
        for issuer in &discovery.issuers {
            item.controls.push(control(
                issuer,
                McpAction::PrepareOAuth {
                    id: id.clone(),
                    revision: *revision,
                    issuer: issuer.clone(),
                },
                idle,
            ));
        }
        item.controls.push(control(
            t("settings.mcp.oauth.cancel"),
            McpAction::CancelOAuth,
            authority.available && !authority.revoking,
        ));
        result.push(item);
    }
    for row in &authority.servers {
        let id = &row.id;
        let revision = row.config_revision;
        let enabled = idle && !row.deleting;
        let revoke = authority.available && !row.deleting && !authority.revoking;
        let mut item = block(format!("server-{id}"), &row.form.display_name);
        item.lines.push(match &row.form.transport {
            McpServerTransport::Local { executable, .. } => t_args(
                "settings.mcp.local_summary",
                &[("path", &executable.display().to_string())],
            ),
            McpServerTransport::Remote { endpoint, .. } => {
                t_args("settings.mcp.remote_summary", &[("endpoint", endpoint)])
            }
        });
        item.lines.push(health_label(&row.health).into());
        item.lines.push(t_args(
            "settings.mcp.config_summary",
            &[
                ("version", &revision.to_string()),
                (
                    "credentials",
                    t(if row.credential_configured {
                        "settings.state.configured"
                    } else {
                        "settings.state.not_configured"
                    }),
                ),
            ],
        ));
        item.controls.push(control(
            t("settings.action.edit"),
            McpAction::Edit(id.clone()),
            enabled,
        ));
        item.controls.push(if row.enabled {
            control(
                t("settings.action.disable"),
                McpAction::Disable {
                    id: id.clone(),
                    revision,
                },
                revoke,
            )
        } else {
            control(
                t("settings.action.enable"),
                McpAction::Ask(McpConfirmation::Enable {
                    id: id.clone(),
                    revision,
                }),
                enabled && (!oauth(row) || row.credential_configured),
            )
        });
        item.controls.push(control(
            t("settings.mcp.test"),
            McpAction::Ask(McpConfirmation::Test {
                id: id.clone(),
                revision,
            }),
            enabled && (!oauth(row) || row.credential_configured),
        ));
        item.controls.push(control(
            t("settings.action.remove"),
            McpAction::Ask(McpConfirmation::Remove {
                id: id.clone(),
                revision,
            }),
            revoke,
        ));
        if oauth(row) {
            if let Some(offer) = authority.offers.iter().find(|offer| {
                row.enabled && offer.server_id == *id && offer.config_revision == revision
            }) {
                item.lines.push(t_args(
                    "settings.mcp.oauth.step_up_warning",
                    &[(
                        "scopes",
                        &offer.added_scopes.join(t("settings.list_separator")),
                    )],
                ));
                item.controls.push(control(
                    t("settings.mcp.oauth.view_step_up"),
                    McpAction::PrepareStepUp {
                        id: id.clone(),
                        revision,
                    },
                    enabled,
                ));
            } else {
                item.controls.push(control(
                    t("settings.mcp.oauth.discover"),
                    McpAction::Ask(McpConfirmation::DiscoverOAuth {
                        id: id.clone(),
                        revision,
                    }),
                    enabled,
                ));
            }
            if row.credential_configured {
                item.controls.push(control(
                    t("settings.mcp.oauth.disconnect"),
                    McpAction::Ask(McpConfirmation::DisconnectOAuth {
                        id: id.clone(),
                        revision,
                    }),
                    revoke,
                ));
            }
            item.lines.push(
                t(if row.credential_configured {
                    "settings.mcp.oauth.configured"
                } else {
                    "settings.mcp.oauth.not_authorized"
                })
                .into(),
            );
        }
        let tools = authority
            .previews
            .get(id)
            .map_or(&row.tool_names, |preview| &preview.tool_names);
        if !tools.is_empty() {
            item.lines.push(t_args(
                "settings.mcp.tools",
                &[("tools", &tools.join(t("settings.list_separator")))],
            ));
        }
        let rejected = authority
            .previews
            .get(id)
            .map_or(&row.rejected_tools, |preview| &preview.rejected_tools);
        if !rejected.is_empty() {
            let expanded = authority.expanded.contains(id);
            item.controls.push(control(
                t_args(
                    if expanded {
                        "settings.mcp.hide_rejected"
                    } else {
                        "settings.mcp.show_rejected"
                    },
                    &[("count", &rejected.len().to_string())],
                ),
                McpAction::ToggleRejected(id.clone()),
                true,
            ));
            if expanded {
                item.lines.extend(rejected.iter().cloned());
            }
        }
        result.push(item);
    }
    result
}

fn editor_controls(authority: &Authority) -> Vec<Control> {
    use vega_i18n::t;
    if authority.editor.is_none() {
        return Vec::new();
    }
    let enabled = authority.available && !authority.busy && authority.flow.is_none();
    let mut controls = vec![
        control(
            t("settings.mcp.transport.local"),
            McpAction::Transport(McpTransportChoice::Local),
            enabled,
        ),
        control(
            t("settings.mcp.transport.remote"),
            McpAction::Transport(McpTransportChoice::Remote),
            enabled,
        ),
    ];
    controls[0].selected = Some(authority.transport == McpTransportChoice::Local);
    controls[1].selected = Some(authority.transport == McpTransportChoice::Remote);
    let saved = matches!(authority.editor, Some(McpEditor::Existing { .. }));
    if authority.transport == McpTransportChoice::Local {
        if saved {
            controls.push(control(
                t("settings.mcp.save_environment_secret"),
                McpAction::SaveLocalSecret,
                enabled,
            ));
            controls.push(control(
                t("settings.mcp.copy_bindings"),
                McpAction::CopyCredentialBindings,
                enabled,
            ));
        }
    } else {
        let mut loopback = control(
            t(if authority.loopback {
                "settings.mcp.loopback_on"
            } else {
                "settings.mcp.loopback_off"
            }),
            McpAction::ToggleLoopback,
            enabled,
        );
        loopback.selected = Some(authority.loopback);
        controls.push(loopback);
        for (choice, key) in [
            (McpAuthChoice::None, "settings.mcp.auth.none"),
            (McpAuthChoice::Bearer, "settings.mcp.auth.bearer"),
            (McpAuthChoice::OAuth, "settings.mcp.auth.oauth"),
        ] {
            let mut item = control(t(key), McpAction::Auth(choice), enabled);
            item.selected = Some(authority.auth == choice);
            controls.push(item);
        }
        if saved && authority.auth == McpAuthChoice::Bearer {
            controls.push(control(
                t("settings.mcp.save_bearer"),
                McpAction::SaveBearer,
                enabled,
            ));
        }
    }
    controls.push(control(
        t("settings.mcp.save_disabled"),
        McpAction::Save,
        enabled,
    ));
    controls.push(control(
        t("settings.action.cancel"),
        McpAction::CancelEditor,
        enabled,
    ));
    controls
}

fn allowed(snapshot: &Snapshot, action: &McpAction) -> bool {
    if matches!(
        action,
        McpAction::Save
            | McpAction::SaveBearer
            | McpAction::SaveLocalSecret
            | McpAction::Transport(_)
            | McpAction::Auth(_)
            | McpAction::ToggleLoopback
    ) && snapshot
        .editor
        .as_ref()
        .is_none_or(|editor| editor.observed.frozen || editor.observed.composing.contains(&true))
    {
        return false;
    }
    blocks(&snapshot.authority)
        .iter()
        .flat_map(|block| &block.controls)
        .chain(editor_controls(&snapshot.authority).iter())
        .any(|control| control.enabled && control.action == *action)
}
fn accepts(current: &Snapshot, observed: &Snapshot, action: &McpAction) -> bool {
    current.visible
        && current.id.matches(&observed.id)
        && current.authority == observed.authority
        && match (&current.editor, &observed.editor) {
            (None, None) => true,
            (Some(current), Some(observed)) => {
                current.draft.session.matches(&observed.draft.session)
                    && current.observed == observed.observed
            }
            _ => false,
        }
        && allowed(current, action)
}

impl SettingsView {
    pub(super) fn reset_mcp_draft(&mut self, cx: &App) {
        let inputs = [
            &self.mcp.name_input,
            &self.mcp.executable_input,
            &self.mcp.args_input,
            &self.mcp.working_directory_input,
            &self.mcp.environment_input,
            &self.mcp.endpoint_input,
            &self.mcp.client_id_input,
            &self.mcp.env_variable_input,
        ];
        self.mcp.nsh_draft = self.mcp.editor.as_ref().map(|_| {
            Draft::new(std::array::from_fn(|index| {
                inputs[index].read(cx).text().into()
            }))
        });
    }
    fn mcp_snapshot(&self, id: ProjectionId, cx: &App) -> Snapshot {
        Snapshot {
            id,
            visible: self.section == 5
                && cx
                    .try_global::<crate::settings::SettingsOpen>()
                    .is_some_and(|open| open.0),
            authority: Arc::new(Authority {
                generation: self.mcp.generation,
                available: self.mcp.service.is_some(),
                busy: self.mcp.busy,
                loading: self.mcp.loading,
                revoking: self.mcp.revoke_in_flight,
                servers: self.mcp.servers.clone(),
                previews: self.mcp.preview.clone(),
                offers: self.mcp.step_up_offers.clone(),
                editor: self.mcp.editor.clone(),
                confirmation: self.mcp.confirmation.clone(),
                transport: self.mcp.transport,
                auth: self.mcp.auth,
                loopback: self.mcp.allow_loopback_http,
                discovery: self.mcp.oauth_discovery.clone(),
                preparation: self.mcp.oauth_preparation.clone(),
                flow: self.mcp.oauth_flow_id.clone(),
                waiting: self.mcp.oauth_waiting,
                expanded: self.mcp.expanded_rejections.clone(),
                message: self.mcp.message.as_ref().map(|message| message.render()),
                message_error: self.mcp.message_error,
            }),
            editor: self
                .mcp
                .editor
                .as_ref()
                .and(self.mcp.nsh_draft.as_ref())
                .map(|draft| Editor {
                    observed: draft.read(),
                    draft: draft.clone(),
                }),
            colors: vega_theme::theme(cx).colors,
        }
    }
    pub(in crate::settings) fn sync_mcp_presentation(&self, cx: &App) {
        if !self.mcp.busy
            && let Some(draft) = &self.mcp.nsh_draft
        {
            draft.release();
        }
        if let Some(page) = &self.mcp.presentation {
            page.surface.publish(self.mcp_snapshot(page.id.clone(), cx));
        }
    }
    pub(crate) fn render_mcp(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.mcp.editor.is_some() && self.mcp.nsh_draft.is_none() {
            self.reset_mcp_draft(cx);
        }
        if self.mcp.presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new(self.mcp_snapshot(id.clone(), cx), render, window, cx);
            self.mcp.presentation = Some(Presentation {
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let mut accepted =
                                        this.mcp.presentation.as_ref().is_some_and(|page| {
                                            accepts(
                                                &this.mcp_snapshot(page.id.clone(), cx),
                                                &command.observed,
                                                &command.action,
                                            )
                                        });
                                    if accepted {
                                        if matches!(
                                            command.action,
                                            McpAction::Save
                                                | McpAction::SaveBearer
                                                | McpAction::SaveLocalSecret
                                        ) {
                                            if let Some(editor) = &command.observed.editor {
                                                if let Some(values) =
                                                    editor.draft.claim(&editor.observed)
                                                {
                                                    match command.action {
                                                        McpAction::Save => {
                                                            let inputs = [
                                                                &this.mcp.name_input,
                                                                &this.mcp.executable_input,
                                                                &this.mcp.args_input,
                                                                &this.mcp.working_directory_input,
                                                                &this.mcp.environment_input,
                                                                &this.mcp.endpoint_input,
                                                                &this.mcp.client_id_input,
                                                                &this.mcp.env_variable_input,
                                                            ];
                                                            for (input, value) in
                                                                inputs.into_iter().zip(values)
                                                            {
                                                                input.update(cx, |input, cx| {
                                                                    input.set_text(&value, cx)
                                                                });
                                                            }
                                                            this.mcp_action(McpAction::Save, cx);
                                                        }
                                                        McpAction::SaveBearer
                                                        | McpAction::SaveLocalSecret => {
                                                            if let (
                                                                Some(McpEditor::Existing {
                                                                    id,
                                                                    revision,
                                                                }),
                                                                Some(secret),
                                                            ) = (
                                                                this.mcp.editor.clone(),
                                                                command.secret.filter(|secret| {
                                                                    !secret.is_empty()
                                                                }),
                                                            ) {
                                                                let operation = if matches!(
                                                                    command.action,
                                                                    McpAction::SaveBearer
                                                                ) {
                                                                    McpOperation::SetBearer {
                                                                        id,
                                                                        revision,
                                                                        secret,
                                                                    }
                                                                } else {
                                                                    McpOperation::SetLocalSecret {
                                                                        id,
                                                                        revision,
                                                                        variable: values[7]
                                                                            .trim()
                                                                            .into(),
                                                                        secret,
                                                                    }
                                                                };
                                                                this.mcp_operation(operation, cx);
                                                            } else {
                                                                accepted = false;
                                                                editor.draft.release();
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                } else {
                                                    accepted = false;
                                                }
                                            } else {
                                                accepted = false;
                                            }
                                        } else {
                                            this.mcp_action(command.action, cx);
                                        }
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.mcp.presentation
                                        && (height - page.height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            this.sync_mcp_presentation(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.mcp.presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.mcp_snapshot(page.id.clone(), cx);
        let Some(page) = &mut self.mcp.presentation else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        gpui_kit::div()
            .w_full()
            .flex_shrink_0()
            .h(gpui_kit::px(page.height))
            .child(page.surface.entity())
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Actions {
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
    edits: Signal<u64>,
}
impl Actions {
    fn snapshot(self) -> Snapshot {
        self.edits.get();
        let mut snapshot = self.state.get();
        if let Some(editor) = &mut snapshot.editor {
            editor.observed = editor.draft.read();
        }
        snapshot
    }
    fn send(
        self,
        commands: mpsc::Sender<Request>,
        action: McpAction,
        authority: Arc<Authority>,
        secret: Option<String>,
    ) {
        let mut observed = self.snapshot();
        observed.authority = authority;
        if self.pending.get_untracked() || !observed.visible || !allowed(&observed, &action) {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            let accepted = commands
                .send(Request::Decide(Box::new(Decision {
                    observed,
                    action,
                    secret,
                    reply,
                })))
                .await
                .is_ok()
                && matches!(response.await, Ok(true));
            self.failed.set(!accepted);
            self.pending.set(false);
        });
    }
}
fn button(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    item: impl Fn() -> Control + Copy + 'static,
    authority: impl Fn() -> Arc<Authority> + Copy + 'static,
    secret: Option<Signal<String>>,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(AccessibilityRole::Button)
        .focusable(true)
        .label(move || item().label)
        .accessibility_state(move || AccessibilityState {
            selected: item().selected == Some(true),
            ..Default::default()
        })
        .disabled(move || {
            let snapshot = actions.snapshot();
            actions.pending.get()
                || !item().enabled
                || snapshot.authority != authority()
                || !allowed(&snapshot, &item().action)
                || secret.is_some_and(|secret| secret.get().is_empty())
        })
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px_3()
        .py_2()
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .bg(move || {
            crate::nsh::color(if item().selected == Some(true) {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_elevated
            })
        })
        .on_press(move |_: &PressEvent| {
            let value = secret.map(|secret| {
                let value = secret.get_untracked();
                secret.set(String::new());
                value
            });
            actions.send(commands.clone(), item().action, authority(), value);
        })
        .child(text(move || item().label))
        .into_element()
}
fn field(
    actions: Actions,
    draft: Draft,
    index: usize,
    key: &'static str,
    secret: Option<Signal<String>>,
) -> AnyElement {
    let value = secret.unwrap_or_else(|| signal(draft.read().text[index].clone()));
    let editing = draft.clone();
    let composing = draft.clone();
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .w_full()
        .gap_1()
        .child(text(move || actions.state.with(|_| vega_i18n::t(key))))
        .child(
            nsh_base::input()
                .element_ref(&target)
                .label(move || actions.state.with(|_| vega_i18n::t(key)))
                .value(move || value.get())
                .masked(secret.is_some())
                .multiline(matches!(index, 2 | 4))
                .readonly(move || {
                    actions.pending.get() || actions.state.get().authority.busy || {
                        actions.edits.get();
                        draft.read().frozen
                    }
                })
                .w_full()
                .border(px(1.))
                .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                .px_3()
                .py_2()
                .border_color(move || {
                    crate::nsh::color(if focused.get() {
                        actions.state.get().colors.accent
                    } else {
                        actions.state.get().colors.border_subtle
                    })
                })
                .bg(move || crate::nsh::color(actions.state.get().colors.bg_elevated))
                .on_change(move |text: &str| {
                    if editing.edit(
                        index,
                        if secret.is_some() { None } else { Some(text) },
                        None,
                    ) {
                        value.set(text.into());
                        actions
                            .edits
                            .update(|revision| *revision = revision.wrapping_add(1));
                    }
                })
                .on_composition_change(move |composition: &Composition| {
                    if composing.edit(index, None, Some(composition.composing)) {
                        actions
                            .edits
                            .update(|revision| *revision = revision.wrapping_add(1));
                    }
                }),
        )
        .into_element()
}
fn form(actions: Actions, commands: mpsc::Sender<Request>, editor: Editor) -> AnyElement {
    let bearer = signal(String::new());
    let environment = signal(String::new());
    let state = actions.state;
    let draft = editor.draft;
    let clear = draft.clone();
    effect(move || {
        if !state.get().visible {
            bearer.set(String::new());
            environment.set(String::new());
            clear.edit(8, None, Some(false));
            clear.edit(9, None, Some(false));
        }
    });
    let local = move || state.get().authority.transport == McpTransportChoice::Local;
    let saved = move || {
        matches!(
            state.get().authority.editor,
            Some(McpEditor::Existing { .. })
        )
    };
    let local_draft = draft.clone();
    let remote_draft = draft.clone();
    let controls = commands.clone();
    view()
        .w_full()
        .gap_3()
        .p(px(Layout::SETTINGS_GROUP_PADDING))
        .border(px(1.))
        .rounded(px(Layout::PANEL_RADIUS))
        .border_color(move || crate::nsh::color(state.get().colors.border_subtle))
        .child(text(move || {
            state.with(|_| {
                vega_i18n::t(if saved() {
                    "settings.mcp.edit"
                } else {
                    "settings.mcp.add"
                })
            })
        }))
        .child(field(actions, draft, 0, "settings.field.name", None))
        .child(when(local, move || {
            let draft = local_draft.clone();
            let secrets = draft.clone();
            view()
                .w_full()
                .gap_3()
                .child(text(move || {
                    state.with(|_| vega_i18n::t("settings.mcp.arguments_warning"))
                }))
                .child(field(
                    actions,
                    draft.clone(),
                    1,
                    "settings.field.executable",
                    None,
                ))
                .child(field(actions, draft.clone(), 2, "settings.mcp.args", None))
                .child(field(
                    actions,
                    draft.clone(),
                    3,
                    "settings.mcp.working_directory",
                    None,
                ))
                .child(field(actions, draft, 4, "settings.mcp.environment", None))
                .child(when(saved, move || {
                    view()
                        .w_full()
                        .gap_3()
                        .child(text(move || {
                            state.with(|_| vega_i18n::t("settings.mcp.environment_warning"))
                        }))
                        .child(field(
                            actions,
                            secrets.clone(),
                            7,
                            "settings.mcp.variable",
                            None,
                        ))
                        .child(field(
                            actions,
                            secrets.clone(),
                            9,
                            "settings.mcp.secret",
                            Some(environment),
                        ))
                }))
        }))
        .child(when(
            move || !local(),
            move || {
                let draft = remote_draft.clone();
                let oauth_draft = draft.clone();
                let secret_draft = draft.clone();
                view()
                    .w_full()
                    .gap_3()
                    .child(field(
                        actions,
                        draft,
                        5,
                        "settings.provider.service_url",
                        None,
                    ))
                    .child(text(move || {
                        state.with(|_| vega_i18n::t("settings.mcp.loopback_hint"))
                    }))
                    .child(when(
                        move || state.get().authority.auth == McpAuthChoice::Bearer,
                        move || {
                            view()
                                .w_full()
                                .gap_3()
                                .child(text(move || {
                                    state.with(|_| vega_i18n::t("settings.mcp.bearer_warning"))
                                }))
                                .child(when(saved, {
                                    let draft = secret_draft.clone();
                                    move || {
                                        field(
                                            actions,
                                            draft.clone(),
                                            8,
                                            "settings.mcp.bearer",
                                            Some(bearer),
                                        )
                                    }
                                }))
                        },
                    ))
                    .child(when(
                        move || state.get().authority.auth == McpAuthChoice::OAuth,
                        move || {
                            view()
                                .w_full()
                                .gap_3()
                                .child(field(
                                    actions,
                                    oauth_draft.clone(),
                                    6,
                                    "settings.mcp.oauth.client_id",
                                    None,
                                ))
                                .child(text(move || {
                                    state.with(|_| vega_i18n::t("settings.mcp.oauth.save_warning"))
                                }))
                        },
                    ))
            },
        ))
        .child(each(
            move || {
                editor_controls(&state.get().authority)
                    .into_iter()
                    .map(|control| (control, state.get().authority))
                    .collect::<Vec<_>>()
            },
            |(control, _)| match &control.action {
                McpAction::Transport(McpTransportChoice::Local) => 0,
                McpAction::Transport(McpTransportChoice::Remote) => 1,
                McpAction::Auth(McpAuthChoice::None) => 2,
                McpAction::Auth(McpAuthChoice::Bearer) => 3,
                McpAction::Auth(McpAuthChoice::OAuth) => 4,
                McpAction::ToggleLoopback => 5,
                McpAction::SaveBearer => 6,
                McpAction::SaveLocalSecret => 7,
                McpAction::CopyCredentialBindings => 8,
                McpAction::Save => 9,
                _ => 10,
            },
            move |item, _| {
                let secret = match item.get_untracked().0.action {
                    McpAction::SaveBearer => Some(bearer),
                    McpAction::SaveLocalSecret => Some(environment),
                    _ => None,
                };
                button(
                    actions,
                    controls.clone(),
                    move || item.get().0,
                    move || item.get().1,
                    secret,
                )
            },
        ))
        .into_element()
}
fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
        edits: signal(0),
    };
    let layout = commands.clone();
    let rows_commands = commands.clone();
    let forms_commands = commands.clone();
    let measured = signal(0.0f32);
    let session = signal_with(None::<ProjectionId>, SignalEquals::Never);
    let revision = signal(0u64);
    let forms = signal_with(Vec::<(u64, Editor)>::new(), SignalEquals::Never);
    effect(move || {
        let editor = state.get().editor;
        let previous = session.get_untracked();
        match editor {
            Some(editor)
                if previous
                    .as_ref()
                    .is_none_or(|session| !session.matches(&editor.draft.session)) =>
            {
                session.set(Some(editor.draft.session.clone()));
                let next = revision.get_untracked().wrapping_add(1);
                revision.set(next);
                forms.set(vec![(next, editor)]);
            }
            None if previous.is_some() => {
                session.set(None);
                forms.set(Vec::new());
            }
            _ => {}
        }
    });
    view()
        .w_full()
        .flex_shrink_0()
        .gap(px(Layout::SETTINGS_SECTION_GAP))
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (height - measured.get_untracked()).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(height)).await.is_err() {
                        actions.failed.set(true);
                    }
                });
            }
        })
        .child(each(
            move || {
                let snapshot = state.get();
                blocks(&snapshot.authority)
                    .into_iter()
                    .map(|block| (block, snapshot.authority.clone()))
                    .collect::<Vec<_>>()
            },
            |(block, _)| block.key.clone(),
            move |entry, _| {
                let block = computed(move || entry.get().0);
                let commands = rows_commands.clone();
                view()
                    .w_full()
                    .gap_2()
                    .p(px(Layout::SETTINGS_GROUP_PADDING))
                    .border(px(1.))
                    .rounded(px(Layout::PANEL_RADIUS))
                    .border_color(move || {
                        crate::nsh::color(if block.get().warning {
                            state.get().colors.warning
                        } else {
                            state.get().colors.border_subtle
                        })
                    })
                    .child(text(move || block.get().title))
                    .child(each(
                        move || block.get().lines,
                        |line| line.clone(),
                        move |line, _| {
                            text(move || line.get()).text_color(move || {
                                crate::nsh::color(if block.get().warning {
                                    state.get().colors.warning
                                } else {
                                    state.get().colors.text_secondary
                                })
                            })
                        },
                    ))
                    .child(each(
                        move || {
                            let (block, authority) = entry.get();
                            block
                                .controls
                                .into_iter()
                                .enumerate()
                                .map(|(index, control)| (index, control, authority.clone()))
                                .collect::<Vec<_>>()
                        },
                        |(index, _, _)| *index,
                        move |item, _| {
                            button(
                                actions,
                                commands.clone(),
                                move || item.get().1,
                                move || item.get().2,
                                None,
                            )
                        },
                    ))
            },
        ))
        .child(each(
            move || forms.get(),
            |(revision, _)| *revision,
            move |editor, _| form(actions, forms_commands.clone(), editor.get_untracked().1),
        ))
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        Snapshot {
            id: ProjectionId::new(),
            visible: true,
            authority: Arc::new(Authority {
                generation: 1,
                available: true,
                busy: true,
                loading: false,
                revoking: false,
                servers: vec![McpServerView {
                    id: "server".into(),
                    config_revision: 4,
                    enabled: true,
                    deleting: false,
                    form: vega_conversation::types::McpServerForm {
                        display_name: "Server".into(),
                        transport: McpServerTransport::Remote {
                            endpoint: "https://example.test/mcp".into(),
                            allow_loopback_http: false,
                            authorization: McpRemoteAuthorization::None,
                        },
                    },
                    credential_configured: false,
                    health: vega_conversation::types::McpServerHealth::Disconnected,
                    tool_names: vec![],
                    rejected_tools: vec![],
                }],
                previews: HashMap::new(),
                offers: vec![],
                editor: None,
                confirmation: None,
                transport: McpTransportChoice::Remote,
                auth: McpAuthChoice::None,
                loopback: false,
                discovery: None,
                preparation: None,
                flow: None,
                waiting: false,
                expanded: HashSet::new(),
                message: None,
                message_error: false,
            }),
            editor: None,
            colors: vega_theme::Theme::light().colors,
        }
    }
    #[test]
    fn mcp_revocation_remains_available_while_testing_and_rejects_stale_revision() {
        let observed = snapshot();
        let disable = McpAction::Disable {
            id: "server".into(),
            revision: 4,
        };
        assert!(accepts(&observed, &observed, &disable));
        assert!(!allowed(&observed, &McpAction::Edit("server".into())));
        let remove = McpAction::Ask(McpConfirmation::Remove {
            id: "server".into(),
            revision: 4,
        });
        assert!(allowed(&observed, &remove));
        let mut current = observed.clone();
        Arc::make_mut(&mut current.authority).servers[0].config_revision += 1;
        assert!(!accepts(&current, &observed, &disable));
        assert!(!allowed(&current, &disable));
        current = observed.clone();
        Arc::make_mut(&mut current.authority).revoking = true;
        assert!(!allowed(&current, &remove));
        current = observed.clone();
        current.visible = false;
        assert!(!accepts(&current, &observed, &disable));
    }
    #[test]
    fn mcp_draft_claim_preserves_argument_boundaries_and_blocks_composition_or_replay() {
        let mut text = std::array::from_fn(|_| String::new());
        text[2] = "--first\n\n two words ".into();
        let draft = Draft::new(text);
        let stale = draft.read();
        assert!(draft.edit(1, Some("/path with spaces/server"), None));
        assert!(draft.claim(&stale).is_none());
        assert!(draft.edit(8, None, Some(true)));
        assert!(draft.claim(&draft.read()).is_none());
        assert!(draft.edit(8, None, Some(false)));
        let claimed = draft.claim(&draft.read()).unwrap();
        assert_eq!(claimed[2], "--first\n\n two words ");
        assert_eq!(claimed[1], "/path with spaces/server");
        assert!(!draft.edit(0, Some("late"), None));
        assert!(draft.claim(&draft.read()).is_none());
        draft.release();
        assert!(draft.edit(0, Some("retry"), None));
        assert!(!draft.edit(8, Some("secret must not enter shared draft"), None));
        assert_eq!(draft.read().text.len(), 8);
    }
    #[test]
    fn mcp_oauth_issuer_and_confirmation_bind_exact_observed_authority() {
        let mut observed = snapshot();
        let authority = Arc::make_mut(&mut observed.authority);
        authority.busy = false;
        authority.discovery = Some((
            "server".into(),
            4,
            McpOAuthDiscovery {
                resource: "https://example.test".into(),
                issuers: vec!["https://issuer.test".into()],
                requested_scopes: vec!["read".into()],
            },
        ));
        let action = McpAction::PrepareOAuth {
            id: "server".into(),
            revision: 4,
            issuer: "https://issuer.test".into(),
        };
        assert!(accepts(&observed, &observed, &action));
        let mut current = observed.clone();
        Arc::make_mut(&mut current.authority)
            .discovery
            .as_mut()
            .unwrap()
            .2
            .issuers
            .clear();
        assert!(!allowed(&current, &action));
        assert!(!accepts(&current, &observed, &action));
        let authority = Arc::make_mut(&mut observed.authority);
        authority.confirmation = Some(McpConfirmation::Test {
            id: "server".into(),
            revision: 3,
        });
        assert!(!allowed(&observed, &McpAction::Confirm));
    }
}
