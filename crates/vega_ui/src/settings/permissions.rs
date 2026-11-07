use super::*;
mod nsh_permission;
use vega_conversation::permission_contracts::DefaultPermissionService;
use vega_conversation::types::{
    DefaultPermissionChange, DefaultPermissionError, DefaultPermissionValues, PermissionMode,
};

pub(crate) struct DefaultPermissionState {
    service: Option<DefaultPermissionService>,
    operation: u64,
    phase: PermissionPhase,
    error: Option<DefaultPermissionError>,
    focus_pending: Option<PermissionFocus>,
    focus_sequence: u64,
    nsh_focus: Option<(u64, PermissionFocus)>,
    presentation: Option<nsh_permission::Presentation>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermissionPhase {
    Idle,
    Preparing(u64),
    Saving(u64),
    Reloading(u64),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermissionFocus {
    Picker,
    Cancel,
    Reload,
}

impl DefaultPermissionState {
    pub(crate) fn new(_cx: &mut Context<SettingsView>) -> Self {
        Self {
            service: None,
            operation: 0,
            phase: PermissionPhase::Idle,
            error: None,
            focus_pending: None,
            focus_sequence: 0,
            nsh_focus: None,
            presentation: None,
        }
    }

    pub(crate) fn saving(&self) -> bool {
        matches!(self.phase, PermissionPhase::Saving(_))
    }

    pub(crate) fn io_pending(&self) -> bool {
        matches!(
            self.phase,
            PermissionPhase::Preparing(_)
                | PermissionPhase::Saving(_)
                | PermissionPhase::Reloading(_)
        )
    }

    fn next_operation(&mut self) -> Option<u64> {
        let Some(operation) = self.operation.checked_add(1) else {
            self.error = Some(DefaultPermissionError::Unavailable);
            return None;
        };
        self.operation = operation;
        Some(operation)
    }
}

impl SettingsView {
    pub fn apply_permission_reset_defaults(
        &mut self,
        values: DefaultPermissionValues,
        cx: &mut Context<Self>,
    ) {
        self.close_default_permission_session();
        self.apply_default_permission_values(values);
        self.default_permission.error = None;
        cx.notify();
    }

    pub fn install_default_permission_service(
        &mut self,
        service: Option<DefaultPermissionService>,
        cx: &mut Context<Self>,
    ) {
        self.default_permission.service = service;
        cx.notify();
    }

    pub(crate) fn permission_picker_enabled(&self) -> bool {
        self.default_permission.service.is_some()
            && !self.permission_reset.projection.authority_blocked()
            && matches!(self.default_permission.phase, PermissionPhase::Idle)
            && self.default_permission.error.is_none()
            && !self.provider_management.saving
            && !self.codex_profile_saving
    }

    pub(crate) fn select_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        if !self.permission_picker_enabled() {
            return;
        }
        let Some(mode) = PermissionMode::parse(mode) else {
            return;
        };
        let Some(service) = self.default_permission.service.clone() else {
            return;
        };
        let Some(operation) = self.default_permission.next_operation() else {
            cx.notify();
            return;
        };
        let expected = DefaultPermissionValues::from(&self.config.defaults);
        self.mode_open = false;
        self.model_open = false;
        self.default_permission.phase = PermissionPhase::Preparing(operation);
        self.default_permission.focus_pending = Some(PermissionFocus::Cancel);
        let worker = cx
            .background_executor()
            .spawn(async move { service.prepare(&expected, mode) });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !matches!(this.default_permission.phase, PermissionPhase::Preparing(id) if id == operation) {
                    return;
                }
                if this.section != 1 || !cx.try_global::<SettingsOpen>().is_some_and(|open| open.0) {
                    this.discard_default_permission_intent();
                    return;
                }
                match result {
                    Ok(change) => this.save_default_permission(change, cx),
                    Err(error) => this.default_permission_failed(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save_default_permission(&mut self, change: DefaultPermissionChange, cx: &mut Context<Self>) {
        if self.provider_management.saving
            || self.codex_profile_saving
            || self.default_permission.saving()
        {
            return;
        }
        let Some(service) = self.default_permission.service.clone() else {
            self.default_permission_failed(DefaultPermissionError::Unavailable);
            return;
        };
        let Some(operation) = self.default_permission.next_operation() else {
            self.default_permission.phase = PermissionPhase::Idle;
            cx.notify();
            return;
        };
        self.default_permission.phase = PermissionPhase::Saving(operation);
        self.default_permission.focus_pending = Some(PermissionFocus::Picker);
        let worker = cx
            .background_executor()
            .spawn(async move { service.save(&change) });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !matches!(this.default_permission.phase, PermissionPhase::Saving(id) if id == operation) {
                    return;
                }
                match result {
                    Ok(values) => {
                        this.apply_default_permission_values(values);
                        this.default_permission.phase = PermissionPhase::Idle;
                        this.default_permission.error = None;
                        this.default_permission.focus_pending = Some(PermissionFocus::Picker);
                        cx.emit(SettingsSaved { replay_identity_changed: false });
                    }
                    Err(error) => this.default_permission_failed(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn reload_default_permission(&mut self, cx: &mut Context<Self>) {
        if self.default_permission.io_pending()
            || self.provider_management.saving
            || self.codex_profile_saving
        {
            return;
        }
        let Some(service) = self.default_permission.service.clone() else {
            return;
        };
        let Some(operation) = self.default_permission.next_operation() else {
            cx.notify();
            return;
        };
        self.default_permission.phase = PermissionPhase::Reloading(operation);
        self.default_permission.focus_pending = Some(PermissionFocus::Cancel);
        let worker = cx
            .background_executor()
            .spawn(async move { service.read() });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if !matches!(this.default_permission.phase, PermissionPhase::Reloading(id) if id == operation) {
                    return;
                }
                if this.section != 1 || !cx.try_global::<SettingsOpen>().is_some_and(|open| open.0) {
                    this.discard_default_permission_intent();
                    return;
                }
                match result {
                    Ok(values) => {
                        this.apply_default_permission_values(values);
                        this.default_permission.phase = PermissionPhase::Idle;
                        this.default_permission.error = None;
                        this.default_permission.focus_pending = Some(PermissionFocus::Picker);
                        cx.emit(SettingsSaved { replay_identity_changed: false });
                    }
                    Err(error) => this.default_permission_failed(error),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn apply_unrelated_config(&mut self, config: AppConfig) {
        let permission = DefaultPermissionValues::from(&self.config.defaults);
        self.config = config;
        self.apply_default_permission_values(permission);
    }

    pub fn default_permission_save_pending(&self) -> bool {
        self.default_permission.saving()
    }

    pub fn close_default_permission_session(&mut self) {
        self.discard_default_permission_intent();
    }

    pub(crate) fn apply_default_permission_values(&mut self, values: DefaultPermissionValues) {
        self.config.defaults.permission_mode = values.permission_mode;
        self.config.defaults.permission_revision = values.permission_revision;
        self.config.defaults.full_access_grant = values.full_access_grant;
    }

    fn default_permission_failed(&mut self, error: DefaultPermissionError) {
        self.default_permission.phase = PermissionPhase::Idle;
        self.default_permission.error = Some(error);
        self.default_permission.focus_pending = Some(PermissionFocus::Reload);
    }

    pub(crate) fn discard_default_permission_intent(&mut self) {
        if !self.default_permission.saving() {
            self.default_permission.phase = PermissionPhase::Idle;
            self.default_permission.focus_pending = None;
            self.default_permission.nsh_focus = None;
            self.mode_open = false;
        }
    }

    pub(crate) fn guard_default_permission_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.default_permission.saving() {
            return true;
        }
        if !matches!(self.default_permission.phase, PermissionPhase::Idle) || self.mode_open {
            self.discard_default_permission_intent();
            self.focus_permission_picker(window, cx);
            cx.notify();
            return true;
        }
        false
    }

    pub(crate) fn prepare_default_permission_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(focus) = self.default_permission.focus_pending.take() else {
            return;
        };
        if self.section != 1 || !cx.try_global::<SettingsOpen>().is_some_and(|open| open.0) {
            return;
        }
        self.focus_permission_presentation(focus, window, cx);
    }
}
