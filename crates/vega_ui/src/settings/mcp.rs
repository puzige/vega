use crate::interaction::InteractionExt;
use crate::scroll_edge_fade::{ScrollFade, ScrollableFadeExt};
use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, FocusHandle, KeyDownEvent, MouseButton, MouseUpEvent, div, px};
use vega_conversation::types::{
    McpConnectionTest, McpEnvironmentVariable, McpOAuthDiscovery, McpOAuthPreparation,
    McpOAuthRegistration, McpOAuthStart, McpOAuthStepUpOffer, McpRemoteAuthorization,
    McpServerForm, McpServerHealth, McpServerTransport, McpServerView,
};
use vega_conversation::{McpServerSettingsService, McpSettingsError};
use vega_i18n::{t, t_args};
use vega_theme::{Layout, Typography, theme};

use super::{
    NextMcpAction, PreviousMcpAction, SettingsText, SettingsView, TextInput, section_title,
    settings_action_chrome, settings_field, settings_group, settings_section, settings_status,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpTransportChoice {
    Local,
    Remote,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpAuthChoice {
    None,
    Bearer,
    OAuth,
}

#[derive(Clone)]
pub(crate) enum McpEditor {
    New,
    Existing { id: String, revision: u64 },
}

#[derive(Clone)]
pub(crate) enum McpConfirmation {
    Enable { id: String, revision: u64 },
    Test { id: String, revision: u64 },
    Remove { id: String, revision: u64 },
    DiscoverOAuth { id: String, revision: u64 },
    DisconnectOAuth { id: String, revision: u64 },
}

impl McpConfirmation {
    fn id(&self) -> &str {
        match self {
            Self::Enable { id, .. }
            | Self::Test { id, .. }
            | Self::Remove { id, .. }
            | Self::DiscoverOAuth { id, .. }
            | Self::DisconnectOAuth { id, .. } => id,
        }
    }
}

#[derive(Clone)]
enum McpAction {
    Reload,
    Add(McpTransportChoice),
    Edit(String),
    Transport(McpTransportChoice),
    Auth(McpAuthChoice),
    ToggleLoopback,
    Save,
    SaveBearer,
    SaveLocalSecret,
    CopyCredentialBindings,
    CancelEditor,
    Disable {
        id: String,
        revision: u64,
    },
    Ask(McpConfirmation),
    Confirm,
    CancelConfirmation,
    PrepareOAuth {
        id: String,
        revision: u64,
        issuer: String,
    },
    PrepareStepUp {
        id: String,
        revision: u64,
    },
    AuthorizeOAuth,
    CancelOAuth,
}

enum McpOperation {
    CredentialBindings(String),
    Reload,
    Create(McpServerForm),
    Replace {
        id: String,
        revision: u64,
        form: McpServerForm,
    },
    SetBearer {
        id: String,
        revision: u64,
        secret: String,
    },
    SetLocalSecret {
        id: String,
        revision: u64,
        variable: String,
        secret: String,
    },
    Enable {
        id: String,
        revision: u64,
    },
    Disable {
        id: String,
        revision: u64,
    },
    Test {
        id: String,
        revision: u64,
    },
    Remove {
        id: String,
        revision: u64,
    },
    OAuthDiscovery {
        id: String,
        revision: u64,
    },
    OAuthPrepare {
        id: String,
        revision: u64,
        issuer: String,
    },
    OAuthStepUpPrepare {
        id: String,
        revision: u64,
    },
    OAuthBegin {
        flow_id: String,
    },
    OAuthFinish {
        flow_id: String,
    },
    OAuthDisconnect {
        id: String,
        revision: u64,
    },
}

enum McpOperationOutput {
    CredentialBindings(String),
    NoData,
    Tested(McpConnectionTest),
    OAuthDiscovered(McpOAuthDiscovery),
    OAuthPrepared(McpOAuthPreparation),
    OAuthStarted(McpOAuthStart),
    OAuthFinished,
}

pub(crate) struct McpSettingsState {
    pub(crate) service: Option<McpServerSettingsService>,
    pub(crate) servers: Vec<McpServerView>,
    pub(crate) loading: bool,
    pub(crate) busy: bool,
    revoke_in_flight: bool,
    pub(super) message: Option<SettingsText>,
    pub(super) message_error: bool,
    pub(crate) editor: Option<McpEditor>,
    pub(crate) confirmation: Option<McpConfirmation>,
    pub(crate) transport: McpTransportChoice,
    pub(crate) auth: McpAuthChoice,
    pub(crate) allow_loopback_http: bool,
    pub(crate) name_input: gpui_kit::Entity<TextInput>,
    pub(crate) executable_input: gpui_kit::Entity<TextInput>,
    pub(crate) args_input: gpui_kit::Entity<TextInput>,
    pub(crate) working_directory_input: gpui_kit::Entity<TextInput>,
    pub(crate) environment_input: gpui_kit::Entity<TextInput>,
    pub(crate) endpoint_input: gpui_kit::Entity<TextInput>,
    pub(crate) client_id_input: gpui_kit::Entity<TextInput>,
    pub(crate) bearer_input: gpui_kit::Entity<TextInput>,
    pub(crate) env_variable_input: gpui_kit::Entity<TextInput>,
    pub(crate) env_secret_input: gpui_kit::Entity<TextInput>,
    pub(crate) preview: HashMap<String, McpConnectionTest>,
    pub(super) step_up_offers: Vec<McpOAuthStepUpOffer>,
    pub(super) oauth_discovery: Option<(String, u64, McpOAuthDiscovery)>,
    pub(super) oauth_preparation: Option<McpOAuthPreparation>,
    pub(super) oauth_flow_id: Option<String>,
    pub(super) oauth_waiting: bool,
    focuses: HashMap<String, FocusHandle>,
    pub(super) generation: u64,
}

impl McpSettingsState {
    pub(crate) fn new(cx: &mut gpui_kit::Context<SettingsView>) -> Self {
        Self {
            service: None,
            servers: Vec::new(),
            loading: false,
            busy: false,
            revoke_in_flight: false,
            message: None,
            message_error: false,
            editor: None,
            confirmation: None,
            transport: McpTransportChoice::Local,
            auth: McpAuthChoice::None,
            allow_loopback_http: false,
            name_input: cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.mcp.name_placeholder")
                    .with_tab_stop(true)
            }),
            executable_input: cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.mcp.executable_placeholder")
                    .with_tab_stop(true)
            }),
            args_input: cx.new(|cx| {
                TextInput::new_multiline(cx, "", 2)
                    .with_placeholder_key("settings.mcp.args_placeholder")
                    .with_scroll_fade_on_base()
                    .with_row_bounds(2, 4)
                    .with_tab_stop(true)
            }),
            working_directory_input: cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.mcp.directory_placeholder")
                    .with_tab_stop(true)
            }),
            environment_input: cx.new(|cx| {
                TextInput::new_multiline(cx, "", 2)
                    .with_placeholder_key("settings.mcp.environment_placeholder")
                    .with_scroll_fade_on_base()
                    .with_row_bounds(2, 4)
                    .with_tab_stop(true)
            }),
            endpoint_input: cx
                .new(|cx| TextInput::new(cx, "https://example.com/mcp", false).with_tab_stop(true)),
            client_id_input: cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.mcp.client_id_placeholder")
                    .with_tab_stop(true)
            }),
            bearer_input: cx.new(|cx| {
                TextInput::new(cx, "", true)
                    .with_placeholder_key("settings.mcp.bearer_placeholder")
                    .with_tab_stop(true)
            }),
            env_variable_input: cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.mcp.variable_placeholder")
                    .with_tab_stop(true)
            }),
            env_secret_input: cx.new(|cx| {
                TextInput::new(cx, "", true)
                    .with_placeholder_key("settings.mcp.secret_placeholder")
                    .with_tab_stop(true)
            }),
            preview: HashMap::new(),
            step_up_offers: Vec::new(),
            oauth_discovery: None,
            oauth_preparation: None,
            oauth_flow_id: None,
            oauth_waiting: false,
            focuses: HashMap::new(),
            generation: 0,
        }
    }
}

fn mcp_error_label(error: McpSettingsError) -> SettingsText {
    match error {
        McpSettingsError::Invalid => SettingsText::key("settings.mcp.error.invalid"),
        McpSettingsError::Conflict => SettingsText::key("settings.mcp.error.conflict"),
        McpSettingsError::NotFound => SettingsText::key("settings.mcp.error.missing"),
        McpSettingsError::Capacity => SettingsText::key("settings.mcp.error.capacity"),
        McpSettingsError::Store => SettingsText::key("settings.mcp.error.store"),
        McpSettingsError::CredentialBindingRequired => {
            SettingsText::key("settings.mcp.error.bindings")
        }
        McpSettingsError::Credential => SettingsText::key("settings.mcp.error.credentials"),
        McpSettingsError::AuthorizationRequired => {
            SettingsText::key("settings.mcp.error.authorization")
        }
        McpSettingsError::CimdUnavailable => SettingsText::key("settings.mcp.error.cimd"),
        McpSettingsError::AuthorizationFailed => SettingsText::key("settings.mcp.error.oauth"),
        McpSettingsError::Connection => SettingsText::key("settings.mcp.error.connection"),
        McpSettingsError::UnsupportedTransport => SettingsText::key("settings.mcp.error.transport"),
        McpSettingsError::ConfirmationRequired => {
            SettingsText::key("settings.mcp.error.confirmation")
        }
    }
}

pub(super) fn health_label(health: &McpServerHealth) -> &'static str {
    match health {
        McpServerHealth::Disabled => t("settings.mcp.health.disabled"),
        McpServerHealth::PendingRemoval => t("settings.mcp.health.removing"),
        McpServerHealth::Disconnected => t("settings.mcp.health.disconnected"),
        McpServerHealth::Ready => t("settings.mcp.health.connected"),
        McpServerHealth::NeedsCredential => t("settings.mcp.health.credentials"),
        McpServerHealth::NeedsAuthorization => t("settings.mcp.health.authorization"),
        McpServerHealth::Error(code) if code == "unsupported_transport" => {
            t("settings.mcp.health.transport")
        }
        McpServerHealth::Error(_) => t("settings.mcp.health.failed"),
    }
}

pub(super) fn enable_result_message(row: Option<&McpServerView>) -> (SettingsText, bool) {
    match row {
        Some(row) if row.enabled => match &row.health {
            McpServerHealth::Disconnected | McpServerHealth::Ready => {
                (SettingsText::key("settings.mcp.enabled.ready"), false)
            }
            McpServerHealth::Error(code) if code == "unsupported_transport" => {
                (SettingsText::key("settings.mcp.enabled.transport"), true)
            }
            McpServerHealth::Error(_) => {
                (SettingsText::key("settings.mcp.enabled.connection"), true)
            }
            McpServerHealth::NeedsCredential => {
                (SettingsText::key("settings.mcp.enabled.credentials"), true)
            }
            McpServerHealth::NeedsAuthorization => (
                SettingsText::key("settings.mcp.enabled.authorization"),
                true,
            ),
            McpServerHealth::Disabled | McpServerHealth::PendingRemoval => {
                (SettingsText::key("settings.mcp.enabled.unconfirmed"), true)
            }
        },
        Some(_) => (SettingsText::key("settings.mcp.enabled.unconfirmed"), true),
        None => (SettingsText::key("settings.mcp.operation_completed"), false),
    }
}

fn mcp_async<T>(
    operation: impl std::future::Future<Output = Result<T, McpSettingsError>>,
) -> Result<T, McpSettingsError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| McpSettingsError::Connection)?
        .block_on(operation)
}

pub(super) fn mcp_confirmation_lines(row: &McpServerView) -> Vec<String> {
    match &row.form.transport {
        McpServerTransport::Local {
            executable,
            args,
            working_directory,
            environment,
        } => {
            let mut lines = vec![
                t_args(
                    "settings.mcp.confirm.local_program",
                    &[("path", &(executable.display()).to_string())],
                ),
                t_args(
                    "settings.mcp.confirm.working_directory",
                    &[(
                        "path",
                        &(working_directory
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| t("settings.mcp.owned_directory").into())),
                    )],
                ),
                t_args(
                    "settings.mcp.confirm.arguments",
                    &[("count", &(args.len()).to_string())],
                ),
            ];
            lines.extend(args.iter().enumerate().map(|(index, argument)| {
                // Escaped per argv entry so boundaries remain unambiguous.
                format!("[{index}] {argument:?}")
            }));
            lines.push(t_args(
                "settings.mcp.confirm.environment",
                &[(
                    "variables",
                    &(if environment.is_empty() {
                        t("settings.state.none").to_string()
                    } else {
                        environment
                            .iter()
                            .map(|entry| entry.variable.as_str())
                            .collect::<Vec<_>>()
                            .join(t("settings.list_separator"))
                    }),
                )],
            ));
            lines.push(t("settings.mcp.credentials_warning").into());
            lines
        }
        McpServerTransport::Remote {
            endpoint,
            allow_loopback_http,
            authorization,
        } => {
            let mode = match authorization {
                McpRemoteAuthorization::None => t("settings.mcp.auth.none"),
                McpRemoteAuthorization::Bearer => t("settings.mcp.auth.bearer_detail"),
                McpRemoteAuthorization::OAuth { .. } => t("settings.mcp.auth.oauth_detail"),
            };
            let mut lines = vec![
                t_args("settings.mcp.confirm.endpoint", &[("endpoint", endpoint)]),
                t_args("settings.mcp.confirm.authentication", &[("mode", mode)]),
            ];
            if *allow_loopback_http {
                lines.push(t("settings.mcp.loopback_warning").into());
            }
            lines
        }
    }
}

pub(super) fn mcp_oauth_preparation_lines(preparation: &McpOAuthPreparation) -> Vec<String> {
    let mut lines = vec![
        t_args(
            "settings.mcp.oauth.resource",
            &[("resource", &preparation.resource)],
        ),
        t_args(
            "settings.mcp.oauth.issuer",
            &[("issuer", &preparation.issuer)],
        ),
        t_args(
            "settings.mcp.oauth.callback",
            &[("address", &preparation.redirect_uri)],
        ),
        t_args(
            "settings.mcp.oauth.total_scopes",
            &[(
                "scopes",
                &(if preparation.requested_scopes.is_empty() {
                    t("settings.mcp.scopes_undeclared").to_string()
                } else {
                    preparation
                        .requested_scopes
                        .join(t("settings.list_separator"))
                }),
            )],
        ),
        match preparation.registration {
            McpOAuthRegistration::PreRegistered => t("settings.mcp.oauth.preregistered").into(),
            McpOAuthRegistration::DynamicRegistration => t("settings.mcp.oauth.dcr").into(),
            McpOAuthRegistration::CimdUnavailable => {
                t("settings.mcp.oauth.cimd_unavailable").into()
            }
            McpOAuthRegistration::Unavailable => {
                t("settings.mcp.oauth.registration_unavailable").into()
            }
        },
    ];
    if let Some(endpoint) = &preparation.registration_endpoint {
        lines.push(t_args(
            "settings.mcp.oauth.registration_endpoint",
            &[("endpoint", endpoint)],
        ));
    }
    if !preparation.step_up_added_scopes.is_empty() {
        lines.push(t_args(
            "settings.mcp.oauth.added_scopes",
            &[(
                "scopes",
                &(preparation
                    .step_up_added_scopes
                    .join(t("settings.list_separator"))),
            )],
        ));
        lines.push(t("settings.mcp.oauth.no_replay").into());
    }
    lines
}

pub(super) fn mcp_oauth_can_begin(registration: McpOAuthRegistration) -> bool {
    matches!(
        registration,
        McpOAuthRegistration::PreRegistered | McpOAuthRegistration::DynamicRegistration
    )
}

impl SettingsView {
    pub(super) fn refresh_mcp_localization(&self, cx: &mut Context<Self>) {
        for input in [
            &self.mcp.name_input,
            &self.mcp.executable_input,
            &self.mcp.args_input,
            &self.mcp.working_directory_input,
            &self.mcp.environment_input,
            &self.mcp.endpoint_input,
            &self.mcp.client_id_input,
            &self.mcp.bearer_input,
            &self.mcp.env_variable_input,
            &self.mcp.env_secret_input,
        ] {
            input.update(cx, |_, cx| cx.notify());
        }
    }

    /// Installs the *same* app-owned revocation domain used by the agent
    /// worker. No Settings instance is allowed to construct its own service.
    pub fn install_mcp_service(
        &mut self,
        service: Option<McpServerSettingsService>,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        self.cancel_mcp_oauth(cx);
        self.mcp.generation = self.mcp.generation.wrapping_add(1);
        self.mcp.service = service;
        self.mcp.servers.clear();
        self.mcp.step_up_offers.clear();
        self.mcp.preview.clear();
        self.mcp.editor = None;
        self.mcp.confirmation = None;
        self.mcp.busy = false;
        self.mcp.revoke_in_flight = false;
        self.mcp.loading = false;
        self.mcp.message = None;
        self.mcp.message_error = false;
        self.mcp.bearer_input.update(cx, TextInput::clear);
        self.mcp.env_secret_input.update(cx, TextInput::clear);
        if self.mcp.service.is_some() {
            self.mcp_operation(McpOperation::Reload, cx);
        } else {
            cx.notify();
        }
    }

    /// An open loopback listener is a short-lived authorization capability.
    /// Closing Settings, changing the service owner, or pressing Cancel must
    /// remove it even when the browser callback is still pending.
    pub(crate) fn cancel_mcp_oauth(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if let (Some(service), Some(flow_id)) =
            (self.mcp.service.as_ref(), self.mcp.oauth_flow_id.take())
            && let Err(error) = service.cancel_oauth(&flow_id)
        {
            self.mcp.message = Some(mcp_error_label(error));
            self.mcp.message_error = true;
        }
        self.mcp.oauth_discovery = None;
        self.mcp.oauth_preparation = None;
        self.mcp.oauth_waiting = false;
        self.mcp.generation = self.mcp.generation.wrapping_add(1);
        self.mcp.busy = false;
        self.mcp.loading = false;
        cx.notify();
    }

    fn mcp_begin_editor(
        &mut self,
        row: Option<McpServerView>,
        choice: McpTransportChoice,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        self.mcp.confirmation = None;
        self.mcp.editor = row.as_ref().map_or(Some(McpEditor::New), |row| {
            Some(McpEditor::Existing {
                id: row.id.clone(),
                revision: row.config_revision,
            })
        });
        let form = row.map(|row| row.form);
        self.mcp.transport = match form.as_ref().map(|form| &form.transport) {
            Some(McpServerTransport::Local { .. }) => McpTransportChoice::Local,
            Some(McpServerTransport::Remote { .. }) => McpTransportChoice::Remote,
            None => choice,
        };
        let name = form
            .as_ref()
            .map(|form| form.display_name.as_str())
            .unwrap_or("");
        self.mcp
            .name_input
            .update(cx, |input, cx| input.set_text(name, cx));
        self.mcp.bearer_input.update(cx, TextInput::clear);
        self.mcp.env_secret_input.update(cx, TextInput::clear);
        self.mcp.env_variable_input.update(cx, TextInput::clear);
        self.mcp.auth = McpAuthChoice::None;
        self.mcp.allow_loopback_http = false;
        for input in [
            self.mcp.executable_input.clone(),
            self.mcp.args_input.clone(),
            self.mcp.working_directory_input.clone(),
            self.mcp.environment_input.clone(),
            self.mcp.endpoint_input.clone(),
            self.mcp.client_id_input.clone(),
        ] {
            input.update(cx, TextInput::clear);
        }
        match form.map(|form| form.transport) {
            Some(McpServerTransport::Local {
                executable,
                args,
                working_directory,
                environment,
            }) => {
                self.mcp.executable_input.update(cx, |input, cx| {
                    input.set_text(&executable.to_string_lossy(), cx)
                });
                self.mcp
                    .args_input
                    .update(cx, |input, cx| input.set_text(&args.join("\n"), cx));
                self.mcp.working_directory_input.update(cx, |input, cx| {
                    input.set_text(
                        &working_directory
                            .map(|dir| dir.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        cx,
                    )
                });
                self.mcp.environment_input.update(cx, |input, cx| {
                    input.set_text(
                        &environment
                            .into_iter()
                            .map(|entry| entry.variable)
                            .collect::<Vec<_>>()
                            .join("\n"),
                        cx,
                    )
                });
            }
            Some(McpServerTransport::Remote {
                endpoint,
                allow_loopback_http,
                authorization,
            }) => {
                self.mcp
                    .endpoint_input
                    .update(cx, |input, cx| input.set_text(&endpoint, cx));
                self.mcp.allow_loopback_http = allow_loopback_http;
                self.mcp.auth = match authorization {
                    McpRemoteAuthorization::None => McpAuthChoice::None,
                    McpRemoteAuthorization::Bearer => McpAuthChoice::Bearer,
                    McpRemoteAuthorization::OAuth { client_id } => {
                        self.mcp.client_id_input.update(cx, |input, cx| {
                            input.set_text(client_id.as_deref().unwrap_or(""), cx)
                        });
                        McpAuthChoice::OAuth
                    }
                };
            }
            None => {}
        }
        cx.notify();
    }

    fn mcp_form(&self, cx: &gpui_kit::App) -> McpServerForm {
        let text = |input: &gpui_kit::Entity<TextInput>| input.read(cx).text().trim().to_owned();
        let transport = match self.mcp.transport {
            McpTransportChoice::Local => McpServerTransport::Local {
                executable: PathBuf::from(text(&self.mcp.executable_input)),
                args: self
                    .mcp
                    .args_input
                    .read(cx)
                    .text()
                    .lines()
                    .map(str::to_owned)
                    .collect(),
                working_directory: (!text(&self.mcp.working_directory_input).is_empty())
                    .then(|| PathBuf::from(text(&self.mcp.working_directory_input))),
                environment: self
                    .mcp
                    .environment_input
                    .read(cx)
                    .text()
                    .lines()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(|variable| McpEnvironmentVariable {
                        variable: variable.to_owned(),
                    })
                    .collect(),
            },
            McpTransportChoice::Remote => McpServerTransport::Remote {
                endpoint: text(&self.mcp.endpoint_input),
                allow_loopback_http: self.mcp.allow_loopback_http,
                authorization: match self.mcp.auth {
                    McpAuthChoice::None => McpRemoteAuthorization::None,
                    McpAuthChoice::Bearer => McpRemoteAuthorization::Bearer,
                    McpAuthChoice::OAuth => McpRemoteAuthorization::OAuth {
                        client_id: (!text(&self.mcp.client_id_input).is_empty())
                            .then(|| text(&self.mcp.client_id_input)),
                    },
                },
            },
        };
        McpServerForm {
            display_name: text(&self.mcp.name_input),
            transport,
        }
    }

    fn mcp_action(&mut self, action: McpAction, cx: &mut gpui_kit::Context<Self>) {
        let revoke = match &action {
            McpAction::Disable { .. }
            | McpAction::Ask(
                McpConfirmation::Remove { .. } | McpConfirmation::DisconnectOAuth { .. },
            )
            | McpAction::CancelOAuth => true,
            McpAction::Confirm => {
                matches!(
                    self.mcp.confirmation,
                    Some(McpConfirmation::Remove { .. } | McpConfirmation::DisconnectOAuth { .. })
                )
            }
            McpAction::CancelConfirmation => true,
            _ => false,
        };
        if (self.mcp.busy && !revoke)
            || (self.mcp.revoke_in_flight
                && revoke
                && !matches!(action, McpAction::CancelConfirmation))
            || (self.mcp.oauth_flow_id.is_some()
                && !matches!(
                    action,
                    McpAction::CancelOAuth
                        | McpAction::AuthorizeOAuth
                        | McpAction::Disable { .. }
                        | McpAction::Ask(
                            McpConfirmation::Remove { .. }
                                | McpConfirmation::DisconnectOAuth { .. }
                        )
                        | McpAction::Confirm
                        | McpAction::CancelConfirmation
                ))
            || self.mcp.service.is_none()
        {
            return;
        }
        match action {
            McpAction::Reload => self.mcp_operation(McpOperation::Reload, cx),
            McpAction::Add(choice) => self.mcp_begin_editor(None, choice, cx),
            McpAction::Edit(id) => {
                if let Some(row) = self.mcp.servers.iter().find(|row| row.id == id).cloned() {
                    self.mcp_begin_editor(Some(row), McpTransportChoice::Local, cx);
                }
            }
            McpAction::Transport(choice) => {
                self.mcp.transport = choice;
                cx.notify();
            }
            McpAction::Auth(choice) => {
                self.mcp.auth = choice;
                cx.notify();
            }
            McpAction::ToggleLoopback => {
                self.mcp.allow_loopback_http = !self.mcp.allow_loopback_http;
                cx.notify();
            }
            McpAction::Save => {
                let form = self.mcp_form(cx);
                if let Some(editor) = self.mcp.editor.clone() {
                    let operation = match editor {
                        McpEditor::New => McpOperation::Create(form),
                        McpEditor::Existing { id, revision } => {
                            McpOperation::Replace { id, revision, form }
                        }
                    };
                    self.mcp_operation(operation, cx);
                }
            }
            McpAction::SaveBearer => {
                if let Some(McpEditor::Existing { id, revision }) = self.mcp.editor.clone() {
                    let secret = self.mcp.bearer_input.read(cx).text().to_owned();
                    self.mcp.bearer_input.update(cx, TextInput::clear);
                    self.mcp_operation(
                        McpOperation::SetBearer {
                            id,
                            revision,
                            secret,
                        },
                        cx,
                    );
                }
            }
            McpAction::CopyCredentialBindings => {
                if let Some(McpEditor::Existing { id, .. }) = self.mcp.editor.clone() {
                    self.mcp_operation(McpOperation::CredentialBindings(id), cx);
                }
            }
            McpAction::SaveLocalSecret => {
                if let Some(McpEditor::Existing { id, revision }) = self.mcp.editor.clone() {
                    let variable = self
                        .mcp
                        .env_variable_input
                        .read(cx)
                        .text()
                        .trim()
                        .to_owned();
                    let secret = self.mcp.env_secret_input.read(cx).text().to_owned();
                    self.mcp.env_secret_input.update(cx, TextInput::clear);
                    self.mcp_operation(
                        McpOperation::SetLocalSecret {
                            id,
                            revision,
                            variable,
                            secret,
                        },
                        cx,
                    );
                }
            }
            McpAction::CancelEditor => {
                self.mcp.editor = None;
                self.mcp.bearer_input.update(cx, TextInput::clear);
                self.mcp.env_secret_input.update(cx, TextInput::clear);
                cx.notify();
            }
            McpAction::Disable { id, revision } => {
                if self.mcp.oauth_flow_id.is_some() {
                    self.cancel_mcp_oauth(cx);
                }
                self.mcp_operation(McpOperation::Disable { id, revision }, cx)
            }
            McpAction::Ask(action) => {
                if matches!(
                    action,
                    McpConfirmation::Remove { .. } | McpConfirmation::DisconnectOAuth { .. }
                ) && self.mcp.oauth_flow_id.is_some()
                {
                    // The user's revocation intent supersedes browser auth
                    // while the separate destructive confirmation is open.
                    self.cancel_mcp_oauth(cx);
                }
                self.mcp.editor = None;
                self.mcp.bearer_input.update(cx, TextInput::clear);
                self.mcp.env_secret_input.update(cx, TextInput::clear);
                self.mcp.confirmation = Some(action);
                cx.notify();
            }
            McpAction::Confirm => {
                if let Some(action) = self.mcp.confirmation.take() {
                    let operation = match action {
                        McpConfirmation::Enable { id, revision } => {
                            McpOperation::Enable { id, revision }
                        }
                        McpConfirmation::Test { id, revision } => {
                            McpOperation::Test { id, revision }
                        }
                        McpConfirmation::Remove { id, revision } => {
                            McpOperation::Remove { id, revision }
                        }
                        McpConfirmation::DiscoverOAuth { id, revision } => {
                            McpOperation::OAuthDiscovery { id, revision }
                        }
                        McpConfirmation::DisconnectOAuth { id, revision } => {
                            McpOperation::OAuthDisconnect { id, revision }
                        }
                    };
                    if matches!(
                        operation,
                        McpOperation::Remove { .. } | McpOperation::OAuthDisconnect { .. }
                    ) && self.mcp.oauth_flow_id.is_some()
                    {
                        self.cancel_mcp_oauth(cx);
                    }
                    self.mcp_operation(operation, cx);
                }
            }
            McpAction::CancelConfirmation => {
                self.mcp.confirmation = None;
                cx.notify();
            }
            McpAction::PrepareOAuth {
                id,
                revision,
                issuer,
            } => {
                self.mcp_operation(
                    McpOperation::OAuthPrepare {
                        id,
                        revision,
                        issuer,
                    },
                    cx,
                );
            }
            McpAction::PrepareStepUp { id, revision } => {
                self.mcp_operation(McpOperation::OAuthStepUpPrepare { id, revision }, cx);
            }
            McpAction::AuthorizeOAuth => {
                if let Some(preparation) = &self.mcp.oauth_preparation
                    && mcp_oauth_can_begin(preparation.registration)
                {
                    self.mcp_operation(
                        McpOperation::OAuthBegin {
                            flow_id: preparation.flow_id.clone(),
                        },
                        cx,
                    );
                }
            }
            McpAction::CancelOAuth => {
                self.cancel_mcp_oauth(cx);
                if !self.mcp.message_error {
                    self.mcp.message = Some(SettingsText::key("settings.mcp.oauth.canceled"));
                }
                cx.notify();
            }
        }
    }

    fn mcp_operation(&mut self, operation: McpOperation, cx: &mut gpui_kit::Context<Self>) {
        let Some(service) = self.mcp.service.clone() else {
            return;
        };
        // A user-triggered disable/remove supersedes an in-flight discovery.
        // Its service revokes first; the old completion must not overwrite the
        // newer Settings projection even if it returns late.
        self.mcp.generation = self.mcp.generation.wrapping_add(1);
        self.mcp.busy = true;
        self.mcp.revoke_in_flight = matches!(
            operation,
            McpOperation::Disable { .. }
                | McpOperation::Remove { .. }
                | McpOperation::OAuthDisconnect { .. }
        );
        self.mcp.loading = matches!(operation, McpOperation::Reload);
        self.mcp.message = None;
        self.mcp.message_error = false;
        let generation = self.mcp.generation;
        let is_edit = matches!(
            &operation,
            McpOperation::Create(_) | McpOperation::Replace { .. }
        );
        let tested_id = match &operation {
            McpOperation::Test { id, .. } => Some(id.clone()),
            _ => None,
        };
        let enabled_id = match &operation {
            McpOperation::Enable { id, .. } => Some(id.clone()),
            _ => None,
        };
        let discovered_context = match &operation {
            McpOperation::OAuthDiscovery { id, revision } => Some((id.clone(), *revision)),
            _ => None,
        };
        let cleanup_flow = match &operation {
            McpOperation::OAuthBegin { flow_id } | McpOperation::OAuthFinish { flow_id } => {
                Some(flow_id.clone())
            }
            _ => None,
        };
        let cleanup_service = service.clone();
        // MCP discovery can wait for an uncooperative external child/server.
        // Never synchronously block GPUI's executor: its test dispatcher, like
        // the UI lane in production, must remain able to dispatch a later
        // Disable/Remove while this operation is still in flight.
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("vega-mcp-settings".into())
            .spawn(move || {
                let result: Result<McpOperationOutput, McpSettingsError> = match operation {
                    McpOperation::CredentialBindings(id) => service
                        .local_credential_binding_template(&id)
                        .map(McpOperationOutput::CredentialBindings),
                    McpOperation::Reload => Ok(McpOperationOutput::NoData),
                    McpOperation::Create(form) => {
                        service.create(form).map(|_| McpOperationOutput::NoData)
                    }
                    McpOperation::Replace { id, revision, form } => service
                        .replace(&id, revision, form)
                        .map(|_| McpOperationOutput::NoData),
                    McpOperation::SetBearer {
                        id,
                        revision,
                        secret,
                    } => service
                        .set_bearer_secret(&id, revision, secret)
                        .map(|_| McpOperationOutput::NoData),
                    McpOperation::SetLocalSecret {
                        id,
                        revision,
                        variable,
                        secret,
                    } => service
                        .set_local_env_secret(&id, revision, &variable, secret)
                        .map(|_| McpOperationOutput::NoData),
                    McpOperation::Remove { id, revision } => service
                        .remove(&id, revision, true)
                        .map(|_| McpOperationOutput::NoData),
                    McpOperation::Enable { id, revision } => {
                        mcp_async(service.set_enabled(&id, revision, true, true))
                            .map(|_| McpOperationOutput::NoData)
                    }
                    McpOperation::Disable { id, revision } => {
                        mcp_async(service.set_enabled(&id, revision, false, false))
                            .map(|_| McpOperationOutput::NoData)
                    }
                    McpOperation::Test { id, revision } => {
                        mcp_async(service.test_connection(&id, revision, true))
                            .map(McpOperationOutput::Tested)
                    }
                    McpOperation::OAuthDiscovery { id, revision } => {
                        mcp_async(service.discover_oauth(&id, revision, true))
                            .map(McpOperationOutput::OAuthDiscovered)
                    }
                    McpOperation::OAuthPrepare {
                        id,
                        revision,
                        issuer,
                    } => mcp_async(service.prepare_oauth(&id, revision, &issuer))
                        .map(McpOperationOutput::OAuthPrepared),
                    McpOperation::OAuthStepUpPrepare { id, revision } => {
                        mcp_async(service.prepare_verified_step_up(&id, revision))
                            .map(McpOperationOutput::OAuthPrepared)
                    }
                    McpOperation::OAuthBegin { flow_id } => {
                        mcp_async(service.begin_oauth(&flow_id, true))
                            .map(McpOperationOutput::OAuthStarted)
                    }
                    McpOperation::OAuthFinish { flow_id } => {
                        mcp_async(service.finish_oauth(&flow_id))
                            .map(|_| McpOperationOutput::OAuthFinished)
                    }
                    McpOperation::OAuthDisconnect { id, revision } => service
                        .disconnect_oauth(&id, revision, true)
                        .map(|_| McpOperationOutput::NoData),
                };
                let _ = sender.send((result, service.list(), service.step_up_offers()));
            });
        if worker.is_err() {
            self.mcp.busy = false;
            self.mcp.revoke_in_flight = false;
            self.mcp.loading = false;
            self.mcp.message_error = true;
            self.mcp.message = Some(SettingsText::key("settings.mcp.worker_failed"));
            cx.notify();
            return;
        }
        // A channel waker fired from a real OS worker would violate GPUI's
        // deterministic test scheduler. Poll a nonblocking standard channel
        // on the app executor; this also avoids any cross-thread UI wake.
        let poller = cx.background_executor().clone();
        let completion = cx.background_executor().spawn(async move {
            loop {
                match receiver.try_recv() {
                    Ok(completed) => break completed,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        break (
                            Err(McpSettingsError::Connection),
                            Err(McpSettingsError::Connection),
                            Err(McpSettingsError::Connection),
                        );
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        poller.timer(std::time::Duration::from_millis(20)).await;
                    }
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let (result, refreshed, step_up_offers) = completion.await;
            let _ = this.update(cx, |this, cx| {
                if this.mcp.generation != generation {
                    // A stale prepare may have bound a loopback listener after
                    // Settings closed. Never leave that capability dangling.
                    let stale_flow = match &result {
                        Ok(McpOperationOutput::OAuthPrepared(prepared)) => Some(&prepared.flow_id),
                        Ok(McpOperationOutput::OAuthStarted(started)) => Some(&started.flow_id),
                        _ => cleanup_flow.as_ref(),
                    };
                    if let Some(flow_id) = stale_flow {
                        let _ = cleanup_service.cancel_oauth(flow_id);
                    }
                    return;
                }
                this.mcp.busy = false;
                this.mcp.revoke_in_flight = false;
                this.mcp.loading = false;
                match refreshed {
                    Ok(servers) => {
                        this.mcp.servers = servers;
                        this.mcp
                            .preview
                            .retain(|id, _| this.mcp.servers.iter().any(|row| &row.id == id));
                    }
                    Err(error) => {
                        this.mcp.message = Some(mcp_error_label(error));
                        this.mcp.message_error = true;
                    }
                }
                match step_up_offers {
                    Ok(offers) => this.mcp.step_up_offers = offers,
                    Err(error) if this.mcp.message.is_none() => {
                        this.mcp.message = Some(mcp_error_label(error));
                        this.mcp.message_error = true;
                    }
                    Err(_) => {}
                }
                match result {
                    Ok(output) => {
                        if is_edit {
                            this.mcp.editor = None;
                        }
                        match output {
                            McpOperationOutput::CredentialBindings(template) => {
                                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                    template,
                                ));
                            }
                            McpOperationOutput::NoData => {}
                            McpOperationOutput::Tested(preview) => {
                                if let Some(id) = tested_id {
                                    let count = preview.tool_names.len();
                                    this.mcp.preview.insert(id, preview);
                                    if this.mcp.message.is_none() {
                                        this.mcp.message = Some(SettingsText::args(
                                            "settings.mcp.test_result",
                                            &[("count", &count.to_string())],
                                        ));
                                    }
                                }
                            }
                            McpOperationOutput::OAuthDiscovered(discovery) => {
                                if let Some((id, revision)) = discovered_context {
                                    this.mcp.oauth_discovery = Some((id, revision, discovery));
                                    if this.mcp.message.is_none() {
                                        this.mcp.message = Some(SettingsText::key(
                                            "settings.mcp.oauth.discovered",
                                        ));
                                    }
                                }
                            }
                            McpOperationOutput::OAuthPrepared(preparation) => {
                                this.mcp.oauth_flow_id = Some(preparation.flow_id.clone());
                                this.mcp.oauth_preparation = Some(preparation);
                                if this.mcp.message.is_none() {
                                    this.mcp.message =
                                        Some(SettingsText::key("settings.mcp.oauth.prepared"));
                                }
                            }
                            McpOperationOutput::OAuthStarted(start) => {
                                this.mcp.oauth_preparation = None;
                                this.mcp.oauth_flow_id = Some(start.flow_id.clone());
                                this.mcp.oauth_waiting = true;
                                this.mcp.message =
                                    Some(SettingsText::key("settings.mcp.oauth.waiting"));
                                cx.open_url(&start.authorization_url);
                                this.mcp_operation(
                                    McpOperation::OAuthFinish {
                                        flow_id: start.flow_id,
                                    },
                                    cx,
                                );
                                return;
                            }
                            McpOperationOutput::OAuthFinished => {
                                this.mcp.oauth_discovery = None;
                                this.mcp.oauth_preparation = None;
                                this.mcp.oauth_flow_id = None;
                                this.mcp.oauth_waiting = false;
                                if this.mcp.message.is_none() {
                                    this.mcp.message =
                                        Some(SettingsText::key("settings.mcp.oauth.saved"));
                                }
                            }
                        }
                        if this.mcp.message.is_none() {
                            let row = enabled_id
                                .as_deref()
                                .and_then(|id| this.mcp.servers.iter().find(|row| row.id == id));
                            let (message, warning) = enable_result_message(row);
                            this.mcp.message = Some(message);
                            this.mcp.message_error = warning;
                        }
                        if let Some(McpEditor::Existing { id, revision }) = this.mcp.editor.as_mut()
                            && let Some(row) = this.mcp.servers.iter().find(|row| row.id == *id)
                        {
                            *revision = row.config_revision;
                        }
                    }
                    Err(error) => {
                        if let Some(flow_id) = cleanup_flow.as_ref() {
                            let _ = cleanup_service.cancel_oauth(flow_id);
                            this.mcp.oauth_preparation = None;
                            this.mcp.oauth_flow_id = None;
                            this.mcp.oauth_waiting = false;
                        }
                        this.mcp.message = Some(mcp_error_label(error));
                        this.mcp.message_error = true;
                        if matches!(
                            error,
                            McpSettingsError::Conflict | McpSettingsError::NotFound
                        ) {
                            this.mcp.editor = None;
                            this.mcp.confirmation = None;
                        } else if let Some(McpEditor::Existing { id, revision }) =
                            this.mcp.editor.as_mut()
                            && let Some(row) = this.mcp.servers.iter().find(|row| row.id == *id)
                        {
                            *revision = row.config_revision;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn mcp_button(
        &mut self,
        label: impl Into<String>,
        selector: impl Into<String>,
        action: McpAction,
        enabled: bool,
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let label = label.into();
        let selector = selector.into();
        let colors = theme(cx).colors;
        let selected = match &action {
            McpAction::Transport(choice) => self.mcp.transport == *choice,
            McpAction::Auth(choice) => self.mcp.auth == *choice,
            McpAction::ToggleLoopback => self.mcp.allow_loopback_http,
            _ => false,
        };
        let focus = self
            .mcp
            .focuses
            .entry(selector.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let keyboard_action = action.clone();
        let primary = matches!(
            action,
            McpAction::Save | McpAction::SaveBearer | McpAction::SaveLocalSecret
        );
        let destructive = matches!(action, McpAction::Ask(McpConfirmation::Remove { .. }));
        settings_action_chrome(colors)
            .id(selector.clone())
            .interaction_surface(
                if enabled && primary {
                    colors.brand_primary
                } else if selected {
                    colors.bg_active
                } else {
                    colors.bg_elevated
                },
                if primary {
                    colors.brand_primary_strong
                } else if selected {
                    colors.bg_active
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
            .track_focus(&focus)
            .tab_stop(true)
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(selected, |button| button.bg(colors.bg_active))
            .when(enabled && primary, |button| {
                button
                    .bg(colors.brand_primary)
                    .text_color(colors.brand_on_accent)
            })
            .when(enabled && destructive, |button| {
                button.text_color(colors.danger)
            })
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                            this.mcp_action(action.clone(), cx)
                        }),
                    )
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.mcp_action(keyboard_action.clone(), cx);
                            cx.stop_propagation();
                        }
                    }))
            })
            .when(!enabled, |button| button.text_color(colors.text_tertiary))
            .child(label)
            .into_any_element()
    }

    fn mcp_row(&mut self, row: McpServerView, cx: &mut gpui_kit::Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let enabled = !self.mcp.busy && !row.deleting && self.mcp.oauth_flow_id.is_none();
        let can_revoke = !row.deleting && !self.mcp.revoke_in_flight;
        let id = row.id.clone();
        let revision = row.config_revision;
        let is_oauth = matches!(
            &row.form.transport,
            McpServerTransport::Remote {
                authorization: McpRemoteAuthorization::OAuth { .. },
                ..
            }
        );
        let step_up_offer = self
            .mcp
            .step_up_offers
            .iter()
            .find(|offer| {
                is_oauth
                    && row.enabled
                    && offer.server_id == id
                    && offer.config_revision == revision
            })
            .cloned();
        let target = match &row.form.transport {
            McpServerTransport::Local { executable, .. } => t_args(
                "settings.mcp.local_summary",
                &[("path", &(executable.display()).to_string())],
            ),
            McpServerTransport::Remote { endpoint, .. } => {
                t_args("settings.mcp.remote_summary", &[("endpoint", endpoint)])
            }
        };
        let health = health_label(&row.health);
        let mut actions = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(self.mcp_button(
                t("settings.action.edit"),
                format!("mcp-edit-{id}"),
                McpAction::Edit(id.clone()),
                enabled,
                cx,
            ))
            .child(if row.enabled {
                self.mcp_button(
                    t("settings.action.disable"),
                    format!("mcp-disable-{id}"),
                    McpAction::Disable {
                        id: id.clone(),
                        revision,
                    },
                    can_revoke,
                    cx,
                )
            } else {
                self.mcp_button(
                    t("settings.action.enable"),
                    format!("mcp-enable-{id}"),
                    McpAction::Ask(McpConfirmation::Enable {
                        id: id.clone(),
                        revision,
                    }),
                    enabled && (!is_oauth || row.credential_configured),
                    cx,
                )
            })
            .child(self.mcp_button(
                t("settings.mcp.test"),
                format!("mcp-test-{id}"),
                McpAction::Ask(McpConfirmation::Test {
                    id: id.clone(),
                    revision,
                }),
                enabled && (!is_oauth || row.credential_configured),
                cx,
            ))
            .child(self.mcp_button(
                t("settings.action.remove"),
                format!("mcp-remove-{id}"),
                McpAction::Ask(McpConfirmation::Remove {
                    id: id.clone(),
                    revision,
                }),
                can_revoke,
                cx,
            ));
        if is_oauth {
            if step_up_offer.is_some() {
                actions = actions.child(self.mcp_button(
                    t("settings.mcp.oauth.view_step_up"),
                    format!("mcp-oauth-step-up-{id}"),
                    McpAction::PrepareStepUp {
                        id: id.clone(),
                        revision,
                    },
                    enabled,
                    cx,
                ));
            } else {
                actions = actions.child(self.mcp_button(
                    t("settings.mcp.oauth.discover"),
                    format!("mcp-oauth-discover-{id}"),
                    McpAction::Ask(McpConfirmation::DiscoverOAuth {
                        id: id.clone(),
                        revision,
                    }),
                    enabled,
                    cx,
                ));
            }
            if row.credential_configured {
                actions = actions.child(self.mcp_button(
                    t("settings.mcp.oauth.disconnect"),
                    format!("mcp-oauth-disconnect-{id}"),
                    McpAction::Ask(McpConfirmation::DisconnectOAuth {
                        id: id.clone(),
                        revision,
                    }),
                    can_revoke,
                    cx,
                ));
            }
        }
        let mut card = settings_group(colors)
            .id(format!("mcp-row-{id}"))
            .gap(px(Layout::SETTINGS_CONTENT_GAP))
            .child(
                div()
                    .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                    .child(row.form.display_name),
            )
            .child(div().text_color(colors.text_secondary).child(target))
            .child(
                div()
                    .id(format!("mcp-health-{id}"))
                    .text_color(
                        if matches!(
                            row.health,
                            McpServerHealth::Error(_)
                                | McpServerHealth::NeedsCredential
                                | McpServerHealth::NeedsAuthorization
                        ) {
                            colors.warning
                        } else {
                            colors.text_secondary
                        },
                    )
                    .child(health),
            )
            .child(div().text_color(colors.text_secondary).child(t_args(
                "settings.mcp.config_summary",
                &[
                    ("version", &revision.to_string()),
                    (
                        "credentials",
                        &(if row.credential_configured {
                            t("settings.state.configured")
                        } else {
                            t("settings.state.not_configured")
                        }),
                    ),
                ],
            )))
            .child(actions);
        if let Some(offer) = step_up_offer {
            card = card.child(
                div()
                    .id("mcp-oauth-step-up-notice")
                    .text_color(colors.warning)
                    .child(t_args(
                        "settings.mcp.oauth.step_up_warning",
                        &[(
                            "scopes",
                            &(offer.added_scopes.join(t("settings.list_separator"))),
                        )],
                    )),
            );
        }
        let tools = self
            .mcp
            .preview
            .get(&id)
            .map(|preview| preview.tool_names.as_slice())
            .unwrap_or(&row.tool_names);
        if !tools.is_empty() {
            card = card.child(div().text_color(colors.text_secondary).child(t_args(
                "settings.mcp.tools",
                &[("tools", &(tools.join(t("settings.list_separator"))))],
            )));
        }
        let rejected = self
            .mcp
            .preview
            .get(&id)
            .map(|preview| preview.rejected_tools.as_slice())
            .unwrap_or(&row.rejected_tools);
        if !rejected.is_empty() {
            card = card.child(
                div()
                    .id(format!("mcp-rejected-tools-{id}"))
                    .text_color(colors.warning)
                    .child(t_args(
                        "settings.mcp.rejected_tools",
                        &[("tools", &(rejected.join(t("settings.list_separator"))))],
                    )),
            );
        }
        if is_oauth {
            card = card.child(div().text_color(colors.warning).child(
                if row.credential_configured {
                    t("settings.mcp.oauth.configured")
                } else {
                    t("settings.mcp.oauth.not_authorized")
                },
            ));
        }
        card.into_any_element()
    }

    fn mcp_editor(&mut self, cx: &mut gpui_kit::Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let local = self.mcp.transport == McpTransportChoice::Local;
        let saved = matches!(self.mcp.editor, Some(McpEditor::Existing { .. }));
        let mut form = settings_group(colors)
            .id("mcp-editor")
            .child(section_title(
                if saved {
                    t("settings.mcp.edit")
                } else {
                    t("settings.mcp.add")
                },
                colors.text_primary,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(self.mcp_button(
                        t("settings.mcp.transport.local"),
                        "mcp-local-choice",
                        McpAction::Transport(McpTransportChoice::Local),
                        !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        t("settings.mcp.transport.remote"),
                        "mcp-remote-choice",
                        McpAction::Transport(McpTransportChoice::Remote),
                        !self.mcp.busy,
                        cx,
                    )),
            )
            .child(settings_field(
                t("settings.field.name"),
                None,
                self.mcp.name_input.clone(),
                colors,
            ));
        if local {
            // A multiline TextInput's flex child does not reserve its visual
            // rows in this Settings column. Read the row count here so edits
            // (including wrapped lines at narrow widths) relayout the form.
            let row_height = Typography::BODY * Typography::BODY_LINE_HEIGHT;
            let args_height = px(self.mcp.args_input.read(cx).visible_rows() as f32 * row_height);
            let environment_height =
                px(self.mcp.environment_input.read(cx).visible_rows() as f32 * row_height);
            form = form
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(t("settings.mcp.arguments_warning")),
                )
                .child(settings_field(
                    t("settings.field.executable"),
                    None,
                    self.mcp.executable_input.clone(),
                    colors,
                ))
                .child(settings_field(
                    t("settings.mcp.args"),
                    None,
                    div()
                        .id("mcp-args-frame")
                        .w_full()
                        .h(args_height)
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(self.mcp.args_input.clone()),
                    colors,
                ))
                .child(settings_field(
                    t("settings.mcp.working_directory"),
                    None,
                    div()
                        .id("mcp-working-directory-frame")
                        .w_full()
                        .child(self.mcp.working_directory_input.clone()),
                    colors,
                ))
                .child(settings_field(
                    t("settings.mcp.environment"),
                    None,
                    div()
                        .id("mcp-environment-frame")
                        .w_full()
                        .h(environment_height)
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(self.mcp.environment_input.clone()),
                    colors,
                ));
            if saved {
                form = form
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .child(t("settings.mcp.environment_warning")),
                    )
                    .child(settings_field(
                        t("settings.mcp.variable"),
                        None,
                        self.mcp.env_variable_input.clone(),
                        colors,
                    ))
                    .child(settings_field(
                        t("settings.mcp.secret"),
                        None,
                        self.mcp.env_secret_input.clone(),
                        colors,
                    ))
                    .child(self.mcp_button(
                        t("settings.mcp.save_environment_secret"),
                        "mcp-save-local-secret",
                        McpAction::SaveLocalSecret,
                        !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        t("settings.mcp.copy_bindings"),
                        "mcp-copy-credential-bindings",
                        McpAction::CopyCredentialBindings,
                        !self.mcp.busy,
                        cx,
                    ));
            }
        } else {
            form = form
                .child(settings_field(
                    t("settings.provider.service_url"),
                    None,
                    self.mcp.endpoint_input.clone(),
                    colors,
                ))
                .child(self.mcp_button(
                    if self.mcp.allow_loopback_http {
                        t("settings.mcp.loopback_on")
                    } else {
                        t("settings.mcp.loopback_off")
                    },
                    "mcp-loopback-choice",
                    McpAction::ToggleLoopback,
                    !self.mcp.busy,
                    cx,
                ))
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(t("settings.mcp.loopback_hint")),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.mcp_button(
                            t("settings.mcp.auth.none"),
                            "mcp-auth-none",
                            McpAction::Auth(McpAuthChoice::None),
                            !self.mcp.busy,
                            cx,
                        ))
                        .child(self.mcp_button(
                            t("settings.mcp.auth.bearer"),
                            "mcp-auth-bearer",
                            McpAction::Auth(McpAuthChoice::Bearer),
                            !self.mcp.busy,
                            cx,
                        ))
                        .child(self.mcp_button(
                            t("settings.mcp.auth.oauth"),
                            "mcp-auth-oauth",
                            McpAction::Auth(McpAuthChoice::OAuth),
                            !self.mcp.busy,
                            cx,
                        )),
                );
            if self.mcp.auth == McpAuthChoice::Bearer {
                form = form.child(
                    div()
                        .text_color(colors.warning)
                        .child(t("settings.mcp.bearer_warning")),
                );
                if saved {
                    form = form
                        .child(settings_field(
                            t("settings.mcp.bearer"),
                            None,
                            self.mcp.bearer_input.clone(),
                            colors,
                        ))
                        .child(self.mcp_button(
                            t("settings.mcp.save_bearer"),
                            "mcp-save-bearer",
                            McpAction::SaveBearer,
                            !self.mcp.busy,
                            cx,
                        ));
                }
            }
            if self.mcp.auth == McpAuthChoice::OAuth {
                form = form
                    .child(settings_field(
                        t("settings.mcp.oauth.client_id"),
                        None,
                        self.mcp.client_id_input.clone(),
                        colors,
                    ))
                    .child(
                        div()
                            .text_color(colors.warning)
                            .child(t("settings.mcp.oauth.save_warning")),
                    );
            }
        }
        form.child(
            div()
                .flex()
                .gap_2()
                .child(self.mcp_button(
                    t("settings.mcp.save_disabled"),
                    "mcp-save",
                    McpAction::Save,
                    !self.mcp.busy,
                    cx,
                ))
                .child(self.mcp_button(
                    t("settings.action.cancel"),
                    "mcp-cancel-editor",
                    McpAction::CancelEditor,
                    !self.mcp.busy,
                    cx,
                )),
        )
        .into_any_element()
    }

    fn mcp_confirmation(
        &mut self,
        action: McpConfirmation,
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let row = self
            .mcp
            .servers
            .iter()
            .find(|row| row.id == action.id())
            .cloned();
        let Some(row) = row else {
            return div().into_any_element();
        };
        let authority = mcp_confirmation_lines(&row);
        let label = match &action {
            McpConfirmation::Enable { .. } => t("settings.mcp.confirm.enable"),
            McpConfirmation::Test { .. } => t("settings.mcp.confirm.test"),
            McpConfirmation::Remove { .. } => t("settings.mcp.confirm.remove"),
            McpConfirmation::DiscoverOAuth { .. } => t("settings.mcp.confirm.discover"),
            McpConfirmation::DisconnectOAuth { .. } => t("settings.mcp.confirm.disconnect"),
        };
        settings_section()
            .id("mcp-confirmation")
            .p(px(Layout::SETTINGS_CONTENT_GAP))
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.warning)
            .child(section_title(
                t("settings.mcp.confirm.title"),
                colors.text_primary,
            ))
            .child(div().child(row.form.display_name))
            .child(
                div()
                    .id("mcp-confirmation-authority")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .max_h(px(160.))
                    .overflow_y_scroll()
                    .children(
                        authority
                            .into_iter()
                            .map(|line| div().text_color(colors.text_secondary).child(line)),
                    )
                    .scroll_edge_fade(colors.bg_base, ScrollFade::compact()),
            )
            .when(matches!(action, McpConfirmation::Remove { .. }), |panel| {
                panel.child(
                    div()
                        .text_color(colors.warning)
                        .child(t("settings.mcp.remove_warning")),
                )
            })
            .when(
                matches!(action, McpConfirmation::DiscoverOAuth { .. }),
                |panel| {
                    panel.child(
                        div()
                            .text_color(colors.warning)
                            .child(t("settings.mcp.discover_warning")),
                    )
                },
            )
            .when(
                matches!(action, McpConfirmation::DisconnectOAuth { .. }),
                |panel| {
                    panel.child(
                        div()
                            .text_color(colors.warning)
                            .child(t("settings.mcp.disconnect_warning")),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.mcp_button(
                        label,
                        "mcp-confirm",
                        McpAction::Confirm,
                        !self.mcp.busy
                            || matches!(
                                action,
                                McpConfirmation::Remove { .. }
                                    | McpConfirmation::DisconnectOAuth { .. }
                            ),
                        cx,
                    ))
                    .child(self.mcp_button(
                        t("settings.action.cancel"),
                        "mcp-cancel-confirm",
                        McpAction::CancelConfirmation,
                        true,
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn mcp_oauth_discovery(
        &mut self,
        id: String,
        revision: u64,
        discovery: McpOAuthDiscovery,
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let mut panel = div()
            .id("mcp-oauth-discovery")
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .child(section_title(
                t("settings.mcp.oauth.choose_server"),
                colors.text_primary,
            ))
            .child(t_args(
                "settings.mcp.oauth.protected_resource",
                &[("resource", &discovery.resource)],
            ))
            .child(t_args(
                "settings.mcp.oauth.requested_scopes",
                &[(
                    "scopes",
                    &(if discovery.requested_scopes.is_empty() {
                        t("settings.mcp.scopes_undeclared").to_string()
                    } else {
                        discovery
                            .requested_scopes
                            .join(t("settings.list_separator"))
                    }),
                )],
            ))
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .child(t("settings.mcp.oauth.choose_warning")),
            );
        if discovery.issuers.is_empty() {
            panel = panel.child(
                div()
                    .text_color(colors.warning)
                    .child(t("settings.mcp.oauth.no_verified_issuer")),
            );
        }
        for (index, issuer) in discovery.issuers.into_iter().enumerate() {
            panel = panel.child(self.mcp_button(
                issuer.clone(),
                format!("mcp-oauth-issuer-{index}"),
                McpAction::PrepareOAuth {
                    id: id.clone(),
                    revision,
                    issuer,
                },
                !self.mcp.busy,
                cx,
            ));
        }
        panel
            .child(self.mcp_button(
                t("settings.mcp.oauth.cancel"),
                "mcp-oauth-cancel",
                McpAction::CancelOAuth,
                true,
                cx,
            ))
            .into_any_element()
    }

    fn mcp_oauth_preparation(
        &mut self,
        preparation: McpOAuthPreparation,
        cx: &mut gpui_kit::Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let registration_allowed = mcp_oauth_can_begin(preparation.registration);
        let is_step_up = !preparation.step_up_added_scopes.is_empty();
        let reauthorization = is_step_up
            || self
                .mcp
                .oauth_discovery
                .as_ref()
                .and_then(|(id, _, _)| self.mcp.servers.iter().find(|row| row.id == *id))
                .is_some_and(|row| row.credential_configured);
        div()
            .id("mcp-oauth-preparation")
            .flex()
            .flex_col()
            .min_w_0()
            .flex_shrink_0()
            .gap(px(Layout::SETTINGS_CONTENT_GAP))
            .p(px(Layout::SETTINGS_CONTENT_GAP))
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.warning)
            .child(section_title(
                if is_step_up {
                    t("settings.mcp.oauth.review_step_up")
                } else {
                    t("settings.mcp.oauth.review_scope")
                },
                colors.text_primary,
            ))
            .child(
                div()
                    .id("mcp-oauth-preparation-details")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .max_h(px(200.))
                    .overflow_y_scroll()
                    .children(
                        mcp_oauth_preparation_lines(&preparation)
                            .into_iter()
                            .map(|line| div().text_color(colors.warning).child(line)),
                    )
                    .scroll_edge_fade(colors.bg_base, ScrollFade::compact()),
            )
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .child(t("settings.mcp.oauth.consent_warning")),
            )
            .when(reauthorization, |panel| {
                panel.child(
                    div()
                        .text_color(colors.warning)
                        .child(t("settings.mcp.oauth.reauthorize_warning")),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(self.mcp_button(
                        if is_step_up {
                            t("settings.mcp.oauth.consent_step_up")
                        } else {
                            t("settings.mcp.oauth.consent")
                        },
                        "mcp-oauth-authorize",
                        McpAction::AuthorizeOAuth,
                        registration_allowed && !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        t("settings.mcp.oauth.cancel"),
                        "mcp-oauth-cancel",
                        McpAction::CancelOAuth,
                        true,
                        cx,
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn render_mcp(&mut self, cx: &mut gpui_kit::Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let connected = self.mcp.service.is_some();
        let mut page = settings_section()
            .id("mcp-settings-content")
            .child(section_title(t("settings.mcp.title"), colors.text_primary))
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .child(t("settings.mcp.intro")),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(self.mcp_button(
                        t("settings.mcp.add_local"),
                        "mcp-add-local",
                        McpAction::Add(McpTransportChoice::Local),
                        connected && !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        t("settings.mcp.add_remote"),
                        "mcp-add-remote",
                        McpAction::Add(McpTransportChoice::Remote),
                        connected && !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        t("settings.mcp.refresh"),
                        "mcp-reload",
                        McpAction::Reload,
                        connected && !self.mcp.busy,
                        cx,
                    )),
            );
        if !connected {
            page = page.child(settings_status(
                t("settings.mcp.unavailable"),
                colors.warning,
                colors,
            ));
        } else if self.mcp.loading {
            page = page.child(settings_status(
                t("settings.mcp.loading"),
                colors.text_secondary,
                colors,
            ));
        } else if self.mcp.servers.is_empty() {
            page = page.child(settings_status(
                t("settings.mcp.empty"),
                colors.text_secondary,
                colors,
            ));
        }
        if self.mcp.busy && !self.mcp.loading {
            page = page.child(settings_status(
                if self.mcp.oauth_waiting {
                    t("settings.mcp.oauth.waiting_disabled")
                } else {
                    t("settings.mcp.processing")
                },
                colors.text_secondary,
                colors,
            ));
        }
        if let Some(message) = &self.mcp.message {
            page = page.child(
                settings_status(
                    message.render(),
                    if self.mcp.message_error {
                        colors.danger
                    } else {
                        colors.text_secondary
                    },
                    colors,
                )
                .id("mcp-message"),
            );
        }
        if let Some(confirmation) = self.mcp.confirmation.clone() {
            page = page.child(self.mcp_confirmation(confirmation, cx));
        }
        if self.mcp.oauth_waiting {
            page = page.child(self.mcp_button(
                t("settings.mcp.oauth.cancel_callback"),
                "mcp-oauth-cancel",
                McpAction::CancelOAuth,
                true,
                cx,
            ));
        } else if let Some(preparation) = self.mcp.oauth_preparation.clone() {
            page = page.child(self.mcp_oauth_preparation(preparation, cx));
        } else if let Some((id, revision, discovery)) = self.mcp.oauth_discovery.clone() {
            page = page.child(self.mcp_oauth_discovery(id, revision, discovery, cx));
        }
        for row in self.mcp.servers.clone() {
            page = page.child(self.mcp_row(row, cx));
        }
        if self.mcp.editor.is_some() {
            page = page.child(self.mcp_editor(cx));
        }
        page.into_any_element()
    }

    fn move_mcp_focus(
        &mut self,
        reverse: bool,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        if reverse {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
    }

    pub(crate) fn next_mcp_action(
        &mut self,
        _: &NextMcpAction,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        self.move_mcp_focus(false, window, cx);
    }

    pub(crate) fn previous_mcp_action(
        &mut self,
        _: &PreviousMcpAction,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        self.move_mcp_focus(true, window, cx);
    }
}
