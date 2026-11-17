mod nsh_permission;

use crate::nsh::Surface;
use std::fmt;

use gpui_kit::{App, EventEmitter, Task};
use vega_conversation::agent::{CommandScopeLease, PERMISSION_TIMEOUT, PermissionLease};
use vega_conversation::types::{
    CodexAcpActivityIdentity, CodexArgumentPreview, CommandScopeDecision, CommandScopeRequest,
    PermissionDecision, PermissionOptionChoice, PermissionRequest,
};
use vega_theme::{ThemeColors, theme};

use crate::conversation_stream::display_width;

pub struct PermissionCardResolved;

#[derive(Clone, PartialEq, Eq)]
struct SafePermissionPrompt {
    tool: String,
    display_target: String,
    codex_identity: Option<(CodexAcpActivityIdentity, Option<CodexArgumentPreview>)>,
    danger_rule_id: Option<String>,
    danger_reason: Option<String>,
    allow_persistent: bool,
    acp_options: Option<Vec<PermissionOptionChoice>>,
}

impl SafePermissionPrompt {
    fn from_request(
        request: &PermissionRequest,
        allow_persistent: bool,
        codex_identity: Option<(CodexAcpActivityIdentity, Option<CodexArgumentPreview>)>,
    ) -> Self {
        Self {
            tool: request.tool.clone(),
            display_target: request.display_target.clone(),
            codex_identity: codex_identity.filter(|(identity, _)| {
                request.tool == "codex_acp"
                    && identity.is_valid()
                    && identity.permission_target() == request.display_target
            }),
            danger_rule_id: request.danger_rule_id.clone(),
            danger_reason: request.danger_reason.clone(),
            allow_persistent: allow_persistent && request.external.is_none(),
            acp_options: request.acp_options.clone(),
        }
    }

    fn is_danger(&self) -> bool {
        self.danger_rule_id.is_some()
    }

    fn danger_label(&self) -> &str {
        let internal_danger = self.tool == "bash" && self.acp_options.is_none();
        let key = internal_danger
            .then_some(self.danger_rule_id.as_deref())
            .flatten()
            .and_then(|rule| match rule {
                "rm-root-recursive-force" => Some("ui.danger.root_recursive_delete"),
                "git-force-push" => Some("ui.danger.force_push"),
                "raw-device-write" => Some("ui.danger.raw_device_write"),
                "filesystem-format" => Some("ui.danger.filesystem_format"),
                "diskutil-destructive" => Some("ui.danger.diskutil_destructive"),
                _ => None,
            });
        key.map(vega_i18n::t).unwrap_or_else(|| {
            self.danger_reason
                .as_deref()
                .unwrap_or_else(|| vega_i18n::t("ui.dangerous_command"))
        })
    }

    fn display_tool(&self) -> &str {
        if self.tool == "codex_acp" {
            "Codex ACP"
        } else {
            &self.tool
        }
    }
}

impl fmt::Debug for SafePermissionPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafePermissionPrompt")
            .field("tool", &self.tool)
            .field("danger", &self.is_danger())
            .field("display_target", &"[REDACTED]")
            .finish()
    }
}

pub struct PermissionCard {
    projection_id: crate::nsh::ProjectionId,
    prompt: SafePermissionPrompt,
    lease: Option<PermissionLease>,
    scope_lease: Option<CommandScopeLease>,
    scope_rows: Option<Vec<String>>,
    presentation: Option<nsh_permission::Presentation>,
    resolved: bool,
    _timeout_task: Task<()>,
}

#[derive(Clone)]
pub(crate) struct PermissionSnapshot {
    pub id: crate::nsh::ProjectionId,
    pub title: String,
    pub command: String,
    pub details: Vec<String>,
    pub danger: Option<String>,
    pub persistent_warning: Option<String>,
    pub resolved: bool,
    pub decisions: Vec<PermissionChoice>,
    pub note_allowed: bool,
    pub enter_action: Option<PermissionAction>,
    pub initial_choice: usize,
    pub colors: ThemeColors,
    pub note_placeholder: String,
    pub failed: String,
}

#[derive(Clone)]
pub(crate) struct PermissionChoice {
    pub label: String,
    pub action: PermissionAction,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum PermissionAction {
    Once,
    Always,
    Deny,
    Acp(String),
}

pub(crate) struct PermissionCommand {
    pub id: crate::nsh::ProjectionId,
    pub action: PermissionAction,
    pub note: String,
    pub composing: bool,
    pub escape: bool,
}

impl PermissionCard {
    pub(crate) fn nsh_snapshot(&self, cx: &App) -> PermissionSnapshot {
        let title = if self.scope_rows.is_some() {
            vega_i18n::t("ui.command_scope_title").to_string()
        } else {
            vega_i18n::t_args(
                if self.prompt.is_danger() {
                    "ui.dangerous_command_requires_confirmation_tool"
                } else {
                    "ui.permission_required_tool"
                },
                &[("tool", self.prompt.display_tool())],
            )
        };
        let decisions: Vec<PermissionChoice> = if let Some(options) = &self.prompt.acp_options {
            options
                .iter()
                .map(|option| PermissionChoice {
                    label: option.name.clone(),
                    action: PermissionAction::Acp(option.option_id.clone()),
                })
                .collect()
        } else {
            let mut choices = vec![PermissionChoice {
                label: vega_i18n::t(if self.scope_rows.is_some() {
                    "ui.command_scope_allow"
                } else {
                    "ui.allow_once"
                })
                .into(),
                action: PermissionAction::Once,
            }];
            if self.prompt.allow_persistent && self.scope_rows.is_none() {
                choices.push(PermissionChoice {
                    label: vega_i18n::t("ui.always_allow").into(),
                    action: PermissionAction::Always,
                });
            }
            choices.push(PermissionChoice {
                label: vega_i18n::t("ui.reject").into(),
                action: PermissionAction::Deny,
            });
            choices
        };
        PermissionSnapshot {
            id: self.projection_id.clone(),
            title,
            command: self
                .prompt
                .codex_identity
                .as_ref()
                .map(|(identity, preview)| {
                    crate::tool_card::codex_identity_display(identity, *preview)
                })
                .unwrap_or_else(|| self.prompt.display_target.clone()),
            details: self.scope_rows.clone().unwrap_or_default(),
            danger: self
                .prompt
                .is_danger()
                .then(|| self.prompt.danger_label().to_owned()),
            persistent_warning: (self.prompt.is_danger() && self.prompt.allow_persistent).then(
                || {
                    vega_i18n::t("ui.always_allow_still_requires_confirmation_for_future_dangerous")
                        .into()
                },
            ),
            resolved: self.is_resolved(),
            initial_choice: if self.prompt.acp_options.is_some() {
                0
            } else if self.prompt.is_danger() || self.scope_rows.is_some() {
                decisions.len().saturating_sub(1)
            } else {
                0
            },
            enter_action: if self.scope_rows.is_some() || self.prompt.acp_options.is_some() {
                None
            } else if self.prompt.is_danger() {
                Some(PermissionAction::Deny)
            } else {
                Some(PermissionAction::Once)
            },
            decisions,
            colors: theme(cx).colors,
            note_placeholder: vega_i18n::t("ui.reason_for_rejection_optional").into(),
            failed: vega_i18n::t("ui.command_failed").into(),
            note_allowed: self.scope_rows.is_none() && self.prompt.acp_options.is_none(),
        }
    }

    pub(crate) fn apply_nsh_command(
        &mut self,
        command: PermissionCommand,
        cx: &mut gpui_kit::Context<Self>,
    ) -> bool {
        self.sync_external_resolution(cx);
        if !self.projection_id.matches(&command.id) || self.is_resolved() || command.composing {
            return false;
        }
        if command.escape {
            self.deny_with_note(command.note, cx);
            return self.is_resolved();
        }
        if !self
            .nsh_snapshot(cx)
            .decisions
            .iter()
            .any(|choice| choice.action == command.action)
        {
            return false;
        }
        match command.action {
            PermissionAction::Once => self.allow_once(cx),
            PermissionAction::Always => self.resolve(PermissionDecision::Always, cx),
            PermissionAction::Deny if self.scope_rows.is_some() => {
                self.resolve_scope(CommandScopeDecision::Deny, cx);
            }
            PermissionAction::Deny => {
                let note = command.note.trim().to_owned();
                self.resolve(
                    PermissionDecision::Deny {
                        note: (!note.is_empty()).then_some(note),
                    },
                    cx,
                );
            }
            PermissionAction::Acp(option_id) => {
                self.resolve(PermissionDecision::AcpOption { option_id }, cx);
            }
        }
        self.is_resolved()
    }

    pub(crate) fn new(
        request: &PermissionRequest,
        lease: PermissionLease,
        allow_persistent: bool,
        codex_identity: Option<(CodexAcpActivityIdentity, Option<CodexArgumentPreview>)>,
        cx: &mut gpui_kit::Context<Self>,
    ) -> Self {
        Self::from_prompt(
            SafePermissionPrompt::from_request(request, allow_persistent, codex_identity),
            Some(lease),
            None,
            None,
            cx,
        )
    }

    pub(crate) fn new_scope(
        request: &CommandScopeRequest,
        lease: CommandScopeLease,
        cx: &mut gpui_kit::Context<Self>,
    ) -> Self {
        let prompt = SafePermissionPrompt {
            tool: "bash".into(),
            display_target: request.command().into(),
            codex_identity: None,
            danger_rule_id: None,
            danger_reason: None,
            allow_persistent: false,
            acp_options: None,
        };
        let mut rows = Vec::new();
        for text in [
            vega_i18n::t_args(
                "ui.command_scope_location",
                &[("path", request.project_root())],
            ),
            vega_i18n::t_args(
                "ui.command_scope_timeout",
                &[("milliseconds", &request.timeout_ms().to_string())],
            ),
            vega_i18n::t_args(
                "ui.command_scope_reason",
                &[("reason", request.scope().reason())],
            ),
        ] {
            rows.extend(wrap_command_rows(&text));
        }
        if request.scope().network() {
            let key = if request.bindings().is_empty() {
                "ui.command_scope_network_direct"
            } else {
                "ui.command_scope_network_broker"
            };
            rows.extend(wrap_command_rows(vega_i18n::t(key)));
            let mut origins = request
                .bindings()
                .iter()
                .map(|binding| binding.origin.as_str())
                .collect::<Vec<_>>();
            origins.sort_unstable();
            origins.dedup();
            for origin in origins {
                rows.extend(wrap_command_rows(origin));
            }
        }
        if !request.directories().is_empty() {
            rows.extend(wrap_command_rows(vega_i18n::t(
                "ui.command_scope_directories",
            )));
            for directory in request.directories() {
                rows.extend(wrap_command_rows(&directory.path().to_string_lossy()));
            }
            rows.extend(wrap_command_rows(vega_i18n::t(
                "ui.command_scope_directory_risk",
            )));
        }
        Self::from_prompt(prompt, None, Some(lease), Some(rows), cx)
    }

    fn from_prompt(
        prompt: SafePermissionPrompt,
        lease: Option<PermissionLease>,
        scope_lease: Option<CommandScopeLease>,
        scope_rows: Option<Vec<String>>,
        cx: &mut gpui_kit::Context<Self>,
    ) -> Self {
        let timeout_task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PERMISSION_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                this.timeout(cx);
            });
        });
        Self {
            projection_id: crate::nsh::ProjectionId::new(),
            prompt,
            lease,
            scope_lease,
            scope_rows,
            presentation: None,
            resolved: false,
            _timeout_task: timeout_task,
        }
    }

    pub fn identity(&self) -> (&str, &str) {
        (&self.prompt.tool, &self.prompt.display_target)
    }

    pub fn is_resolved(&self) -> bool {
        self.resolved
            || if self.scope_rows.is_some() {
                self.scope_lease
                    .as_ref()
                    .is_none_or(CommandScopeLease::is_resolved)
            } else {
                self.lease.as_ref().is_none_or(PermissionLease::is_resolved)
            }
    }

    pub(crate) fn timeout(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if self.scope_rows.is_some() {
            self.resolve_scope(CommandScopeDecision::Timeout, cx);
        } else {
            self.resolve(PermissionDecision::Timeout, cx);
        }
    }

    fn allow_once(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if self.scope_rows.is_some() {
            self.resolve_scope(CommandScopeDecision::AllowOnce, cx);
        } else {
            self.resolve(PermissionDecision::Once, cx);
        }
    }

    fn resolve_scope(&mut self, decision: CommandScopeDecision, cx: &mut gpui_kit::Context<Self>) {
        if self.resolved || self.scope_rows.is_none() {
            return;
        }
        if let Some(lease) = &mut self.scope_lease {
            lease.respond(decision);
        }
        self.scope_lease.take();
        self.resolved = true;
        cx.emit(PermissionCardResolved);
        cx.notify();
    }

    fn resolve(&mut self, decision: PermissionDecision, cx: &mut gpui_kit::Context<Self>) {
        if self.resolved
            || self.scope_rows.is_some()
            || (!self.prompt.allow_persistent && matches!(decision, PermissionDecision::Always))
        {
            return;
        }
        if let Some(lease) = self.lease.as_mut() {
            lease.respond(decision);
        }
        self.lease.take();
        self.resolved = true;
        cx.emit(PermissionCardResolved);
        cx.notify();
    }

    fn deny_with_note(&mut self, note: String, cx: &mut gpui_kit::Context<Self>) {
        if self.scope_rows.is_some() {
            self.resolve_scope(CommandScopeDecision::Deny, cx);
            return;
        }
        if let Some(option) = self.prompt.acp_options.as_ref().and_then(|options| {
            options.iter().find(|option| {
                option
                    .kind
                    .as_deref()
                    .is_some_and(|kind| kind.starts_with("reject"))
            })
        }) {
            self.resolve(
                PermissionDecision::AcpOption {
                    option_id: option.option_id.clone(),
                },
                cx,
            );
            return;
        }
        if self.prompt.acp_options.is_some() {
            self.resolve(PermissionDecision::Timeout, cx);
            return;
        }
        let note = note.trim().to_owned();
        self.resolve(
            PermissionDecision::Deny {
                note: (!note.is_empty()).then_some(note),
            },
            cx,
        );
    }

    fn sync_external_resolution(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if !self.resolved && self.is_resolved() {
            self.lease.take();
            self.scope_lease.take();
            self.resolved = true;
            cx.emit(PermissionCardResolved);
            cx.notify();
        }
    }
}

impl EventEmitter<PermissionCardResolved> for PermissionCard {}

fn wrap_command_rows(command: &str) -> Vec<String> {
    const MAX_CHARS: usize = 80;
    let mut rows = Vec::new();
    for physical in command.split('\n') {
        if physical.is_empty() {
            rows.push(String::new());
            continue;
        }
        let mut row = String::new();
        let mut width = 0usize;
        for character in physical.chars() {
            let character_width = display_width(&character.to_string());
            if !row.is_empty() && width + character_width > MAX_CHARS {
                rows.push(std::mem::take(&mut row));
                width = 0;
            }
            row.push(character);
            width += character_width;
            if width == MAX_CHARS {
                rows.push(std::mem::take(&mut row));
                width = 0;
            }
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

impl fmt::Debug for PermissionCard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PermissionCard")
            .field("prompt", &self.prompt)
            .field("resolved", &self.is_resolved())
            .finish()
    }
}
