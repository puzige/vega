//! Issue #73 Settings surface. Every operation goes through the app-injected
//! conversation service; this view never reads MCP tables or credential files.

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
use vega_theme::{Layout, Typography, theme};

use super::{SettingsView, TextInput, section_title};

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
    pub(crate) message: Option<String>,
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
            name_input: cx.new(|cx| TextInput::new(cx, "服务器名称", false).with_tab_stop(true)),
            executable_input: cx
                .new(|cx| TextInput::new(cx, "可执行文件绝对路径", false).with_tab_stop(true)),
            args_input: cx.new(|cx| {
                TextInput::new_multiline(cx, "每行一个参数（不经 shell）", 2)
                    .with_row_bounds(2, 4)
                    .with_tab_stop(true)
            }),
            working_directory_input: cx
                .new(|cx| TextInput::new(cx, "可选：绝对工作目录", false).with_tab_stop(true)),
            environment_input: cx.new(|cx| {
                TextInput::new_multiline(cx, "可选：每行一个环境变量名", 2)
                    .with_row_bounds(2, 4)
                    .with_tab_stop(true)
            }),
            endpoint_input: cx
                .new(|cx| TextInput::new(cx, "https://example.com/mcp", false).with_tab_stop(true)),
            client_id_input: cx.new(|cx| {
                TextInput::new(cx, "可选：已预注册的 Client ID", false).with_tab_stop(true)
            }),
            bearer_input: cx.new(|cx| {
                TextInput::new(cx, "Bearer token（保存后单独录入）", true).with_tab_stop(true)
            }),
            env_variable_input: cx
                .new(|cx| TextInput::new(cx, "已声明的环境变量名", false).with_tab_stop(true)),
            env_secret_input: cx
                .new(|cx| TextInput::new(cx, "环境变量密钥", true).with_tab_stop(true)),
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

fn mcp_error_label(error: McpSettingsError) -> &'static str {
    match error {
        McpSettingsError::Invalid => "配置不合法；请检查 URL、绝对路径、参数和变量名",
        McpSettingsError::Conflict => "服务器配置已变化，列表已刷新；请重新打开编辑后重试",
        McpSettingsError::NotFound => "服务器已移除，列表已刷新",
        McpSettingsError::Capacity => "最多只能启用 8 个 MCP 服务器",
        McpSettingsError::Store => "本地配置存储失败，请重试或检查数据目录",
        McpSettingsError::Credential => "凭据存储不可用；已刷新状态，请重新录入",
        McpSettingsError::AuthorizationRequired => "该服务器需要显式授权，未建立连接",
        McpSettingsError::CimdUnavailable => {
            "该服务器仅接受公网 HTTPS Client ID Metadata；当前未配置公开身份，无法授权"
        }
        McpSettingsError::AuthorizationFailed => {
            "浏览器授权失败、被取消或已超时；请核对服务器状态后重新开始"
        }
        McpSettingsError::Connection => "连接或工具发现失败；服务器保持原有启用状态",
        McpSettingsError::ConfirmationRequired => "需要先确认此服务器的权限范围",
    }
}

pub(super) fn health_label(health: &McpServerHealth) -> &'static str {
    match health {
        McpServerHealth::Disabled => "未启用",
        McpServerHealth::PendingRemoval => "正在移除；凭据清理待重试",
        McpServerHealth::Disconnected => "已启用 · 任务开始时连接",
        McpServerHealth::Ready => "已连接",
        McpServerHealth::NeedsCredential => "缺少凭据",
        McpServerHealth::NeedsAuthorization => "需要授权",
        McpServerHealth::Error(_) => "连接失败",
    }
}

pub(super) fn enable_result_message(row: Option<&McpServerView>) -> (&'static str, bool) {
    match row {
        Some(row) if row.enabled => match &row.health {
            McpServerHealth::Disconnected | McpServerHealth::Ready => (
                "已启用；将在任务开始时连接，连接失败时该任务不会广告工具",
                false,
            ),
            McpServerHealth::Error(_) => (
                "已启用，但连接验证失败；任务开始时可重试，若仍失败则不会向模型提供工具",
                true,
            ),
            McpServerHealth::NeedsCredential => (
                "已启用，但缺少凭据；配置后下次任务可重试连接，失败时不会向模型提供工具",
                true,
            ),
            McpServerHealth::NeedsAuthorization => (
                "已启用，但需要授权；授权后下次任务可重试连接，未授权时不会向模型提供工具",
                true,
            ),
            McpServerHealth::Disabled | McpServerHealth::PendingRemoval => {
                ("启用状态未确认；请检查当前服务器状态", true)
            }
        },
        Some(_) => ("启用状态未确认；请检查当前服务器状态", true),
        None => ("操作已完成；请确认当前服务器列表", false),
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
                format!("本地程序：{}", executable.display()),
                format!(
                    "工作目录：{}",
                    working_directory
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "Vega 自有目录".into())
                ),
                format!("参数（{} 项）：", args.len()),
            ];
            lines.extend(args.iter().enumerate().map(|(index, argument)| {
                // Escaped per argv entry so boundaries remain unambiguous.
                format!("[{index}] {argument:?}")
            }));
            lines.push(format!(
                "环境变量名：{}（不展示密钥）",
                if environment.is_empty() {
                    "无".into()
                } else {
                    environment
                        .iter()
                        .map(|entry| entry.variable.as_str())
                        .collect::<Vec<_>>()
                        .join("、")
                }
            ));
            lines.push("此子进程不受 Vega 内置 bash 沙箱约束；请只用专用密钥字段录入凭据，不要把 token 写进命令参数。".into());
            lines
        }
        McpServerTransport::Remote {
            endpoint,
            allow_loopback_http,
            authorization,
        } => {
            let mode = match authorization {
                McpRemoteAuthorization::None => "无认证",
                McpRemoteAuthorization::Bearer => "独立 Bearer（非 OAuth）",
                McpRemoteAuthorization::OAuth { .. } => "OAuth（需另行显式授权）",
            };
            let mut lines = vec![format!("远程端点：{endpoint}"), format!("认证方式：{mode}")];
            if *allow_loopback_http {
                lines.push("已允许回环 HTTP：本机明文连接，仅限 127.0.0.1 / ::1 开发端点。".into());
            }
            lines
        }
    }
}

pub(super) fn mcp_oauth_preparation_lines(preparation: &McpOAuthPreparation) -> Vec<String> {
    let mut lines = vec![
        format!("资源：{}", preparation.resource),
        format!("发行方：{}", preparation.issuer),
        format!("本机回调地址：{}", preparation.redirect_uri),
        format!(
            "本次授权总权限：{}",
            if preparation.requested_scopes.is_empty() {
                "未声明".into()
            } else {
                preparation.requested_scopes.join("、")
            }
        ),
        match preparation.registration {
            McpOAuthRegistration::PreRegistered => {
                "使用已预注册 Client ID，不动态注册客户端。".into()
            }
            McpOAuthRegistration::DynamicRegistration => {
                "将使用已弃用的动态客户端注册（DCR）。确认后才向以下注册端点提交本机回调地址。"
                    .into()
            }
            McpOAuthRegistration::CimdUnavailable => {
                "仅支持公网 HTTPS Client ID Metadata；当前未配置公开身份，无法继续。".into()
            }
            McpOAuthRegistration::Unavailable => {
                "授权服务器未提供可用的客户端注册方式，无法继续。".into()
            }
        },
    ];
    if let Some(endpoint) = &preparation.registration_endpoint {
        lines.push(format!("客户端注册端点：{endpoint}"));
    }
    if !preparation.step_up_added_scopes.is_empty() {
        lines.push(format!(
            "本次新增权限（来自经验证的 403 挑战）：{}",
            preparation.step_up_added_scopes.join("、")
        ));
        lines.push("之前的工具调用已经失败，不会因授权升级而自动重放。".into());
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
            self.mcp.message = Some(mcp_error_label(error).into());
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
                    self.mcp.message = Some("已取消本次 OAuth 授权；不会启用 MCP 服务器".into());
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
            self.mcp.message = Some("无法启动 MCP 设置任务；没有执行操作，请稍后重试".into());
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
                        this.mcp.message = Some(mcp_error_label(error).into());
                        this.mcp.message_error = true;
                    }
                }
                match step_up_offers {
                    Ok(offers) => this.mcp.step_up_offers = offers,
                    Err(error) if this.mcp.message.is_none() => {
                        this.mcp.message = Some(mcp_error_label(error).into());
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
                            McpOperationOutput::NoData => {}
                            McpOperationOutput::Tested(preview) => {
                                if let Some(id) = tested_id {
                                    let count = preview.tool_names.len();
                                    this.mcp.preview.insert(id, preview);
                                    if this.mcp.message.is_none() {
                                        this.mcp.message = Some(format!(
                                            "连接测试发现 {count} 个工具；测试不改变启用状态"
                                        ));
                                    }
                                }
                            }
                            McpOperationOutput::OAuthDiscovered(discovery) => {
                                if let Some((id, revision)) = discovered_context {
                                    this.mcp.oauth_discovery = Some((id, revision, discovery));
                                    if this.mcp.message.is_none() {
                                        this.mcp.message = Some("已发现授权服务器；请选择要信任的发行方。尚未注册客户端或打开浏览器。".into());
                                    }
                                }
                            }
                            McpOperationOutput::OAuthPrepared(preparation) => {
                                this.mcp.oauth_flow_id = Some(preparation.flow_id.clone());
                                this.mcp.oauth_preparation = Some(preparation);
                                if this.mcp.message.is_none() {
                                    this.mcp.message = Some("授权详情已准备。检查发行方、权限和本机回调地址，再单独确认。".into());
                                }
                            }
                            McpOperationOutput::OAuthStarted(start) => {
                                this.mcp.oauth_preparation = None;
                                this.mcp.oauth_flow_id = Some(start.flow_id.clone());
                                this.mcp.oauth_waiting = true;
                                this.mcp.message = Some("已打开系统浏览器；等待 OAuth 回调。可随时取消，服务器仍为禁用状态。".into());
                                cx.open_url(&start.authorization_url);
                                this.mcp_operation(McpOperation::OAuthFinish {
                                    flow_id: start.flow_id,
                                }, cx);
                                return;
                            }
                            McpOperationOutput::OAuthFinished => {
                                this.mcp.oauth_discovery = None;
                                this.mcp.oauth_preparation = None;
                                this.mcp.oauth_flow_id = None;
                                this.mcp.oauth_waiting = false;
                                if this.mcp.message.is_none() {
                                    this.mcp.message = Some("OAuth 授权凭据已保存；服务器仍为禁用状态，请单独确认启用。".into());
                                }
                            }
                        }
                        if this.mcp.message.is_none() {
                            let row = enabled_id
                                .as_deref()
                                .and_then(|id| this.mcp.servers.iter().find(|row| row.id == id));
                            let (message, warning) = enable_result_message(row);
                            this.mcp.message = Some(message.into());
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
                        this.mcp.message = Some(mcp_error_label(error).into());
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
        let debug_name = if selector.starts_with("mcp-enable-") {
            "mcp-enable"
        } else if selector.starts_with("mcp-remove-") {
            "mcp-remove"
        } else if selector.starts_with("mcp-edit-") {
            "mcp-edit"
        } else if selector.starts_with("mcp-disable-") {
            "mcp-disable"
        } else if selector.starts_with("mcp-test-") {
            "mcp-test"
        } else if selector.starts_with("mcp-oauth-discover-") {
            "mcp-oauth-discover"
        } else if selector.starts_with("mcp-oauth-disconnect-") {
            "mcp-oauth-disconnect"
        } else if selector.starts_with("mcp-oauth-issuer-") {
            "mcp-oauth-issuer"
        } else if selector.starts_with("mcp-oauth-step-up-") {
            "mcp-oauth-step-up"
        } else {
            ""
        };
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
        div()
            .id(selector.clone())
            .debug_selector(move || {
                if debug_name.is_empty() {
                    selector.clone()
                } else {
                    debug_name.into()
                }
            })
            .track_focus(&focus)
            .tab_stop(true)
            .focus_visible(move |style| style.border_1().border_color(colors.accent))
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .when(selected, |button| button.bg(colors.bg_active))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover))
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
            McpServerTransport::Local { executable, .. } => {
                format!("本地 stdio · {}", executable.display())
            }
            McpServerTransport::Remote { endpoint, .. } => format!("远程 HTTP · {endpoint}"),
        };
        let health = health_label(&row.health);
        let mut actions = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(self.mcp_button(
                "编辑",
                format!("mcp-edit-{id}"),
                McpAction::Edit(id.clone()),
                enabled,
                cx,
            ))
            .child(if row.enabled {
                self.mcp_button(
                    "停用",
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
                    "启用",
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
                "测试连接 / 刷新工具",
                format!("mcp-test-{id}"),
                McpAction::Ask(McpConfirmation::Test {
                    id: id.clone(),
                    revision,
                }),
                enabled && (!is_oauth || row.credential_configured),
                cx,
            ))
            .child(self.mcp_button(
                "移除",
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
                    "查看权限升级",
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
                    "发现 OAuth 授权服务",
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
                    "断开本地授权",
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
        let mut card = div()
            .id(format!("mcp-row-{id}"))
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .child(
                div()
                    .font_weight(Typography::HEADING_BLOCK_WEIGHT)
                    .child(row.form.display_name),
            )
            .child(div().text_color(colors.text_secondary).child(target))
            .child(
                div()
                    .id(format!("mcp-health-{id}"))
                    .debug_selector(|| "mcp-health".into())
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
            .child(div().text_color(colors.text_secondary).child(format!(
                "配置版本 {} · 凭据{}",
                revision,
                if row.credential_configured {
                    "已配置"
                } else {
                    "未配置"
                }
            )))
            .child(actions);
        if let Some(offer) = step_up_offer {
            card = card.child(
                div()
                    .id("mcp-oauth-step-up-notice")
                    .debug_selector(|| "mcp-oauth-step-up-notice".into())
                    .text_color(colors.warning)
                    .child(format!(
                        "工具调用因权限不足而失败；服务器请求新增权限：{}。点击「查看权限升级」核对总范围；失败调用不会自动重放。",
                        offer.added_scopes.join("、")
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
            card = card.child(
                div()
                    .text_color(colors.text_secondary)
                    .child(format!("工具：{}", tools.join("、"))),
            );
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
                    .text_color(colors.warning)
                    .child(format!("未采纳的工具：{}", rejected.join("、"))),
            );
        }
        if is_oauth {
            card = card.child(div().text_color(colors.warning).child(
                if row.credential_configured {
                    "OAuth 凭据已本地配置；连接与外部工具调用仍需分别确认。"
                } else {
                    "OAuth 未授权；发现元数据不会自动注册或打开浏览器。"
                },
            ));
        }
        card.into_any_element()
    }

    fn mcp_editor(&mut self, cx: &mut gpui_kit::Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let local = self.mcp.transport == McpTransportChoice::Local;
        let saved = matches!(self.mcp.editor, Some(McpEditor::Existing { .. }));
        let mut form = div()
            .id("mcp-editor")
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .child(section_title(
                if saved {
                    "编辑 MCP 服务器"
                } else {
                    "新增 MCP 服务器"
                },
                colors.text_primary,
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.mcp_button(
                        "本地 stdio",
                        "mcp-local-choice",
                        McpAction::Transport(McpTransportChoice::Local),
                        !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        "远程 HTTP",
                        "mcp-remote-choice",
                        McpAction::Transport(McpTransportChoice::Remote),
                        !self.mcp.busy,
                        cx,
                    )),
            )
            .child(self.mcp.name_input.clone());
        if local {
            // A multiline TextInput's flex child does not reserve its visual
            // rows in this Settings column. Read the row count here so edits
            // (including wrapped lines at narrow widths) relayout the form.
            let row_height = Typography::BODY * Typography::BODY_LINE_HEIGHT;
            let args_height = px(self.mcp.args_input.read(cx).visible_rows() as f32 * row_height);
            let environment_height =
                px(self.mcp.environment_input.read(cx).visible_rows() as f32 * row_height);
            form = form
                .child(div().text_color(colors.text_secondary).child(
                    "程序独立运行，不受 Vega 内置 bash 沙箱约束；参数按行传入，不经过 shell。",
                ))
                .child(self.mcp.executable_input.clone())
                .child(
                    div()
                        .id("mcp-args-frame")
                        .debug_selector(|| "mcp-args-frame".into())
                        .w_full()
                        .h(args_height)
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(self.mcp.args_input.clone()),
                )
                .child(
                    div()
                        .id("mcp-working-directory-frame")
                        .debug_selector(|| "mcp-working-directory-frame".into())
                        .w_full()
                        .child(self.mcp.working_directory_input.clone()),
                )
                .child(
                    div()
                        .id("mcp-environment-frame")
                        .debug_selector(|| "mcp-environment-frame".into())
                        .w_full()
                        .h(environment_height)
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(self.mcp.environment_input.clone()),
                );
            if saved {
                form = form
                    .child(
                        div().text_color(colors.text_secondary).child(
                            "先在上方声明变量名并保存，再单独录入其密钥；不继承其他环境变量。",
                        ),
                    )
                    .child(self.mcp.env_variable_input.clone())
                    .child(self.mcp.env_secret_input.clone())
                    .child(self.mcp_button(
                        "保存此环境密钥",
                        "mcp-save-local-secret",
                        McpAction::SaveLocalSecret,
                        !self.mcp.busy,
                        cx,
                    ));
            }
        } else {
            form = form
                .child(self.mcp.endpoint_input.clone())
                .child(self.mcp_button(
                    if self.mcp.allow_loopback_http {
                        "允许回环 HTTP：开"
                    } else {
                        "允许回环 HTTP：关"
                    },
                    "mcp-loopback-choice",
                    McpAction::ToggleLoopback,
                    !self.mcp.busy,
                    cx,
                ))
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .child("非 HTTPS 仅允许显式开启的 127.0.0.1 / ::1 本机测试端点。"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.mcp_button(
                            "无认证",
                            "mcp-auth-none",
                            McpAction::Auth(McpAuthChoice::None),
                            !self.mcp.busy,
                            cx,
                        ))
                        .child(self.mcp_button(
                            "独立 Bearer",
                            "mcp-auth-bearer",
                            McpAction::Auth(McpAuthChoice::Bearer),
                            !self.mcp.busy,
                            cx,
                        ))
                        .child(self.mcp_button(
                            "OAuth",
                            "mcp-auth-oauth",
                            McpAction::Auth(McpAuthChoice::OAuth),
                            !self.mcp.busy,
                            cx,
                        )),
                );
            if self.mcp.auth == McpAuthChoice::Bearer {
                form = form.child(div().text_color(colors.warning).child(
                    "非 OAuth：仅对这一服务器的精确端点发送 Bearer，不使用 Provider API Key。",
                ));
                if saved {
                    form = form
                        .child(self.mcp.bearer_input.clone())
                        .child(self.mcp_button(
                            "保存 Bearer 密钥",
                            "mcp-save-bearer",
                            McpAction::SaveBearer,
                            !self.mcp.busy,
                            cx,
                        ));
                }
            }
            if self.mcp.auth == McpAuthChoice::OAuth {
                form = form.child(self.mcp.client_id_input.clone())
                    .child(div().text_color(colors.warning).child("保存配置不会授权或启用；可在服务器卡片中显式发现发行方、审查回调与权限，再打开系统浏览器。不能把 Bearer token 冒充 OAuth。"));
            }
        }
        form.child(
            div()
                .flex()
                .gap_2()
                .child(self.mcp_button(
                    "保存（默认禁用）",
                    "mcp-save",
                    McpAction::Save,
                    !self.mcp.busy,
                    cx,
                ))
                .child(self.mcp_button(
                    "取消",
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
            McpConfirmation::Enable { .. } => "确认启用并验证",
            McpConfirmation::Test { .. } => "确认连接测试",
            McpConfirmation::Remove { .. } => "确认移除并清除本地凭据",
            McpConfirmation::DiscoverOAuth { .. } => "确认发现授权元数据",
            McpConfirmation::DisconnectOAuth { .. } => "确认断开本地授权",
        };
        div().id("mcp-confirmation").debug_selector(|| "mcp-confirmation".into()).flex().flex_col().gap_2().px_3().py_3()
            .rounded(px(Layout::PANEL_RADIUS)).border_1().border_color(colors.warning)
            .child(section_title("确认 MCP 操作", colors.text_primary))
            .child(div().child(row.form.display_name))
            .child(div().id("mcp-confirmation-authority")
                .debug_selector(|| "mcp-confirmation-authority".into())
                .flex().flex_col().gap_2().max_h(px(160.)).overflow_y_scroll()
                .children(authority.into_iter().map(|line| div().text_color(colors.text_secondary).child(line))))
            .when(matches!(action, McpConfirmation::Remove { .. }), |panel| panel.child(div().text_color(colors.warning).child("移除保留历史工具审计，仅清除本地凭据；远程服务授权可能仍有效，必要时请到该服务撤销。")))
            .when(matches!(action, McpConfirmation::DiscoverOAuth { .. }), |panel| panel.child(div().text_color(colors.warning).child("将连接端点以发现 OAuth 授权服务器和权限范围；现在不会注册客户端、打开浏览器或启用 MCP。")))
            .when(matches!(action, McpConfirmation::DisconnectOAuth { .. }), |panel| panel.child(div().text_color(colors.warning).child("仅清除 Vega 本地 OAuth 凭据并禁用该服务器，不保证撤销远端授权；请到授权服务器另行撤销 grant。")))
            .child(div().flex().gap_2()
                .child(self.mcp_button(label, "mcp-confirm", McpAction::Confirm,
                    !self.mcp.busy || matches!(action, McpConfirmation::Remove { .. } | McpConfirmation::DisconnectOAuth { .. }), cx))
                .child(self.mcp_button("取消", "mcp-cancel-confirm", McpAction::CancelConfirmation, true, cx)))
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
        let mut panel =
            div()
                .id("mcp-oauth-discovery")
                .debug_selector(|| "mcp-oauth-discovery".into())
                .flex()
                .flex_col()
                .gap_2()
                .px_3()
                .py_3()
                .rounded(px(Layout::PANEL_RADIUS))
                .border_1()
                .border_color(colors.border_subtle)
                .child(section_title("选择 OAuth 授权服务器", colors.text_primary))
                .child(format!("受保护资源：{}", discovery.resource))
                .child(format!(
                    "请求权限：{}",
                    if discovery.requested_scopes.is_empty() {
                        "未声明".into()
                    } else {
                        discovery.requested_scopes.join("、")
                    }
                ))
                .child(div().text_color(colors.text_secondary).child(
                    "选择发行方会读取其元数据并准备本机回调；此步不会动态注册或打开浏览器。",
                ));
        if discovery.issuers.is_empty() {
            panel = panel.child(
                div()
                    .text_color(colors.warning)
                    .child("没有通过验证的授权服务器，不能继续。"),
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
                "取消授权",
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
            .debug_selector(|| "mcp-oauth-preparation".into())
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .rounded(px(Layout::PANEL_RADIUS))
            .border_1()
            .border_color(colors.warning)
            .child(section_title(
                if is_step_up {
                    "审查 OAuth 权限升级"
                } else {
                    "审查 OAuth 授权范围"
                },
                colors.text_primary,
            ))
            .child(
                div()
                    .id("mcp-oauth-preparation-details")
                    .debug_selector(|| "mcp-oauth-preparation-details".into())
                    .flex()
                    .flex_col()
                    .gap_2()
                    .max_h(px(200.))
                    .overflow_y_scroll()
                    .children(
                        mcp_oauth_preparation_lines(&preparation)
                            .into_iter()
                            .map(|line| div().text_color(colors.warning).child(line)),
                    ),
            )
            .child(div().text_color(colors.text_secondary).child(
                "同意后才可能注册客户端并打开系统浏览器；回调成功只保存凭据，仍须单独启用服务器。",
            ))
            .when(reauthorization, |panel| {
                panel.child(
                    div().text_color(colors.warning).child(
                        "重新授权将撤销旧本地 OAuth 凭据并禁用服务器；不会自动恢复此前的授权。",
                    ),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.mcp_button(
                        if is_step_up {
                            "同意新增权限并打开浏览器"
                        } else {
                            "同意并打开系统浏览器"
                        },
                        "mcp-oauth-authorize",
                        McpAction::AuthorizeOAuth,
                        registration_allowed && !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        "取消授权",
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
        let mut page = div()
            .id("mcp-settings-content")
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(section_title("MCP 服务器", colors.text_primary))
                    .child(self.mcp_button(
                        "新增本地",
                        "mcp-add-local",
                        McpAction::Add(McpTransportChoice::Local),
                        connected && !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        "新增远程",
                        "mcp-add-remote",
                        McpAction::Add(McpTransportChoice::Remote),
                        connected && !self.mcp.busy,
                        cx,
                    ))
                    .child(self.mcp_button(
                        "刷新服务器与权限请求",
                        "mcp-reload",
                        McpAction::Reload,
                        connected && !self.mcp.busy,
                        cx,
                    )),
            )
            .child(
                div()
                    .text_size(px(Typography::BODY))
                    .text_color(colors.text_secondary)
                    .child(
                        "服务器配置默认禁用；启用/测试需分别确认。每次外部工具调用仍须单独批准。",
                    ),
            );
        if !connected {
            page = page.child(
                div()
                    .text_color(colors.warning)
                    .child("MCP 服务不可用，请重启 Vega 或检查应用数据目录。"),
            );
        } else if self.mcp.loading {
            page = page.child(
                div()
                    .text_color(colors.text_secondary)
                    .child("正在加载 MCP 配置…"),
            );
        } else if self.mcp.servers.is_empty() {
            page = page.child(
                div()
                    .text_color(colors.text_secondary)
                    .child("尚未添加 MCP 服务器"),
            );
        }
        if self.mcp.busy && !self.mcp.loading {
            page = page.child(div().text_color(colors.text_secondary).child(
                if self.mcp.oauth_waiting {
                    "等待浏览器授权回调…完成后服务器仍保持禁用；可取消本次授权。"
                } else {
                    "正在处理 MCP 操作…"
                },
            ));
        }
        if let Some(message) = &self.mcp.message {
            page = page.child(
                div()
                    .id("mcp-message")
                    .debug_selector(|| "mcp-message".into())
                    .text_color(if self.mcp.message_error {
                        colors.danger
                    } else {
                        colors.text_secondary
                    })
                    .child(message.clone()),
            );
        }
        if let Some(confirmation) = self.mcp.confirmation.clone() {
            page = page.child(self.mcp_confirmation(confirmation, cx));
        }
        if self.mcp.oauth_waiting {
            page = page.child(self.mcp_button(
                "取消授权并关闭回调",
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
}
