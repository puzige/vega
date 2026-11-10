use super::*;

/// the config — each time settings is opened).
pub struct SettingsView {
    pub(crate) catalog: super::model_catalog::CatalogUi,
    pub(crate) updater: vega_conversation::types::UpdateProjection,
    pub(super) updater_message: Option<SettingsText>,
    pub(super) updater_version: Option<SettingsText>,
    pub(super) updater_identity: crate::nsh::ProjectionId,
    pub(super) updater_revision: u64,
    pub(super) updater_presentation: Option<super::updater::Presentation>,
    pub(super) shell_navigation: Option<super::nsh_shell::Presentation>,
    pub(super) shell_heading: Option<super::nsh_shell::Presentation>,
    pub(super) settings_navigation_presentation:
        Option<super::model_configuration::nsh_navigation::Presentation>,
    pub(super) model_navigation_presentation:
        Option<super::model_configuration::nsh_navigation::Presentation>,
    pub(super) appearance_presentation: Option<super::nsh_appearance::Presentation>,
    pub(super) default_model_presentation: Option<super::nsh_default_model::Presentation>,
    pub(super) input_preferences_presentation: Option<super::nsh_input_preferences::Presentation>,
    pub(crate) provider_management: super::provider_management::ProviderManagement,
    pub(super) settings_motion: super::motion::SettingsMotion,
    pub(crate) section: usize,
    pub(crate) model_settings: Option<(String, String)>,
    pub(crate) settings_help_open: Option<String>,
    pub(crate) settings_help_focus: FocusHandle,
    pub(crate) pricing_advanced: bool,
    pub(crate) pending_section: Option<SettingsDestination>,
    pub(crate) pending_settings_close: bool,
    pub(crate) section_scrolls: [gpui_kit::ScrollHandle; 8],
    pub(crate) usage: super::usage::UsageState,
    pub(crate) section_focuses: [FocusHandle; 8],
    pub(crate) mcp: super::mcp::McpSettingsState,
    pub(crate) skills: super::skills::SkillsSettingsState,
    pub(crate) config: AppConfig,
    pub(crate) config_path: Option<std::path::PathBuf>,
    pub(crate) available_key_refs: Vec<String>,
    pub(crate) name_input: Entity<TextInput>,
    pub(crate) base_url_input: Entity<TextInput>,
    pub(crate) models_input: Entity<TextInput>,
    pub(crate) key_input: Entity<TextInput>,
    pub(crate) codex_name_input: Entity<TextInput>,
    pub(crate) codex_executable_input: Entity<TextInput>,
    pub(crate) codex_args_input: Entity<TextInput>,
    pub(crate) codex_profile_saving: bool,
    pub(super) agents_presentation: Option<super::agents::nsh_agents::Presentation>,
    pub(super) agents_nsh_draft: Option<super::agents::nsh_agents::Draft>,
    pub(crate) preparation: Entity<crate::cua_onboarding::model::PreparationModel>,
    pub(crate) preparation_owner: Option<crate::cua_onboarding::CallbackIdentity>,
    pub(crate) mode_open: bool,
    pub(super) palette_open: bool,
    pub(crate) default_permission: super::permissions::DefaultPermissionState,
    pub(crate) permission_reset: super::permission_reset::ResetState,
    pub(crate) model_open: bool,
    /// Inline error message (ui-spec §4.6: no modals); empty until an IO or
    /// local credential failure occurs.
    pub(super) error: Option<SettingsText>,
    pub(crate) pricing: PricingSettingsProjection,
    pub(crate) pricing_editor: Option<PricingEditor>,
    pub(crate) pricing_model_input: Entity<TextInput>,
    pub(crate) pricing_rate_inputs: [Entity<TextInput>; 8],
    pub(super) pricing_presentation: Option<super::nsh_pricing::Presentation>,
    pub(super) pricing_nsh_draft: Option<super::nsh_pricing::Draft>,
    /// Worker-owned independent reasoning capability/preferences projection.
    pub(crate) reasoning: ReasoningSettingsProjection,
    /// Focus handles for the explicit reasoning capability editor. These are
    /// rebuilt with the projection so Tab/Enter/Space use the same action
    /// route as the existing Settings controls.
    pub(crate) reasoning_focuses: Vec<(ReasoningFocusTarget, FocusHandle)>,
    /// Last failed edit retained for an explicit retry/reload decision.
    pub(crate) reasoning_draft: Option<ReasoningProfileProjection>,
    pub(crate) next_reasoning_operation: u64,
    pub(crate) model_capabilities: ModelCapabilitySettingsProjection,
    pub(crate) model_capability_models: Vec<ModelCapabilityProjection>,
    pub(crate) model_capability_draft: Option<ModelCapabilityOverride>,
    pub(super) capability_presentations: super::model_capabilities::Presentations,
    pub(crate) next_model_capability_operation: u64,
}

impl Focusable for SettingsView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.section == 0
            && let Some(focus) = self.provider_surface_focus_handle(cx)
        {
            return focus;
        }
        self.section_focuses[self.section].clone()
    }
}

impl EventEmitter<PricingMutationRequested> for SettingsView {}
impl EventEmitter<vega_conversation::types::PermissionResetRequested> for SettingsView {}
impl EventEmitter<PricingReloadRequested> for SettingsView {}
impl EventEmitter<PricingRetryRequested> for SettingsView {}
impl EventEmitter<PricingDiscardRequested> for SettingsView {}
impl EventEmitter<SettingsSaved> for SettingsView {}
impl EventEmitter<DesktopPreparationRequested> for SettingsView {}
impl EventEmitter<ModelContextLoadRequested> for SettingsView {}
impl EventEmitter<ModelContextSaveRequested> for SettingsView {}
impl EventEmitter<ReasoningProfileSaveRequested> for SettingsView {}
impl EventEmitter<ReasoningReloadRequested> for SettingsView {}
impl EventEmitter<ModelCapabilitySaveRequested> for SettingsView {}
impl EventEmitter<ModelCapabilityReloadRequested> for SettingsView {}

pub(crate) const PRICING_INPUT_BYTES_LIMIT: usize = 1024 * 1024;
pub(crate) const PROVIDER_MODELS_MIN_ROWS: usize = 2;
pub(crate) const PROVIDER_MODELS_MAX_ROWS: usize = 4;
impl SettingsView {
    /// Loads the current config and creates empty form inputs.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::from_path(
            vega_store::paths::AppPaths::resolve()
                .ok()
                .map(|paths| paths.config_file()),
            cx,
        )
    }

    /// Construct Settings using the same explicit config path as the app worker.
    pub fn from_path(path: Option<std::path::PathBuf>, cx: &mut Context<Self>) -> Self {
        let (config, error) = match path.as_deref().map(config::load_from) {
            Some(Ok(config)) => (config, None),
            _ => (
                AppConfig::default(),
                Some(t("settings.config.load_failed").into()),
            ),
        };
        let mut view = Self::from_config(config, error, cx);
        view.config_path = path;
        view.refresh_key_refs();
        view
    }

    fn refresh_key_refs(&mut self) {
        self.available_key_refs = self
            .config_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .and_then(|root| keystore::available_refs(root).ok())
            .unwrap_or_default();
    }

    /// Builds a settings entity from an owned configuration snapshot.
    ///
    /// Production rendering calls [`Self::new`], which reads the user config.
    /// App-level acceptance tests and other embedders can supply an owned
    /// snapshot so constructing the view never reads the host user's files.
    pub fn from_config(config: AppConfig, error: Option<String>, cx: &mut Context<Self>) -> Self {
        let preparation = crate::cua_onboarding::model::shared(cx);
        let preparation_owner = None;
        cx.observe(&preparation, |_, _, cx| cx.notify()).detach();
        cx.on_release(|view, cx| {
            if let Some(owner) = view.preparation_owner.take() {
                view.preparation
                    .update(cx, |model, cx| model.release(&owner, false, cx));
            }
        })
        .detach();
        cx.observe_global::<crate::conversation_stream::RunningMessageDefault>(|this, cx| {
            this.config.agent.running_message_behavior = cx
                .global::<crate::conversation_stream::RunningMessageDefault>()
                .0
                .as_str()
                .to_string();
            cx.notify();
        })
        .detach();
        cx.observe_global::<SkillsSettingsRequested>(|this, cx| {
            if cx.global::<SkillsSettingsRequested>().0 {
                cx.set_global(SkillsSettingsRequested(false));
                this.request_settings_section(6, cx);
            }
        })
        .detach();
        cx.observe_global::<SettingsOpen>(|this, cx| {
            if !cx.global::<SettingsOpen>().0 {
                if let Some(page) = &mut this.input_preferences_presentation {
                    page.revoke();
                }
                if let Some(page) = &mut this.appearance_presentation {
                    page.revoke();
                }
                this.settings_motion.interrupt();
                this.cancel_provider_operation(cx);
                this.cancel_mcp_oauth(cx);
                this.close_skills_session();
                if let Some(owner) = this.preparation_owner.take() {
                    this.preparation
                        .update(cx, |model, cx| model.release(&owner, false, cx));
                }
            }
        })
        .detach();
        let name_input = cx.new(|cx| {
            TextInput::new(cx, "", false)
                .with_placeholder_key("settings.field.name")
                .with_tab_stop(true)
        });
        let base_url_input = cx.new(|cx| {
            TextInput::new(cx, "", false)
                .with_placeholder_key("settings.field.base_url")
                .with_tab_stop(true)
        });
        let models_input = cx.new(|cx| {
            TextInput::new_multiline(cx, "", PROVIDER_MODELS_MIN_ROWS)
                .with_placeholder_key("settings.provider.models_placeholder")
                .with_row_bounds(PROVIDER_MODELS_MIN_ROWS, PROVIDER_MODELS_MAX_ROWS)
                .with_tab_stop(true)
        });
        let key_input = cx.new(|cx| {
            TextInput::new(cx, "", true)
                .with_placeholder_key("settings.field.api_key")
                .with_tab_stop(true)
        });
        let codex_name_input = cx.new(|cx| {
            TextInput::new(cx, "", false)
                .with_placeholder_key("settings.agents.codex.name_placeholder")
                .with_tab_stop(true)
        });
        let codex_executable_input = cx.new(|cx| {
            TextInput::new(cx, "", false)
                .with_placeholder_key("settings.agents.codex.executable_placeholder")
                .with_tab_stop(true)
        });
        let codex_args_input = cx.new(|cx| {
            TextInput::new_multiline(cx, "", 2)
                .with_placeholder_key("settings.agents.codex.arguments_placeholder")
                .with_tab_stop(true)
        });
        let pricing_model_input = cx.new(|cx| {
            TextInput::new(cx, "", false).with_placeholder_key("settings.field.model_id")
        });
        let pricing_rate_inputs = [
            cx.new(|cx| {
                TextInput::new(cx, "", false).with_placeholder_key("settings.pricing.base_input")
            }),
            cx.new(|cx| {
                TextInput::new(cx, "", false).with_placeholder_key("settings.pricing.base_output")
            }),
            cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.pricing.base_cache_read")
            }),
            cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.pricing.base_cache_write")
            }),
            cx.new(|cx| {
                TextInput::new(cx, "", false).with_placeholder_key("settings.pricing.peak_input")
            }),
            cx.new(|cx| {
                TextInput::new(cx, "", false).with_placeholder_key("settings.pricing.peak_output")
            }),
            cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.pricing.peak_cache_read")
            }),
            cx.new(|cx| {
                TextInput::new(cx, "", false)
                    .with_placeholder_key("settings.pricing.peak_cache_write")
            }),
        ];
        let profile = config.agent.codex_acp_profile.clone();
        if let Some(profile) = &profile {
            codex_name_input.update(cx, |input, cx| input.set_text(&profile.display_name, cx));
            codex_executable_input.update(cx, |input, cx| input.set_text(&profile.executable, cx));
            codex_args_input.update(cx, |input, cx| input.set_text(&profile.args.join("\n"), cx));
        }
        Self {
            catalog: super::model_catalog::CatalogUi::new(cx),
            updater: Default::default(),
            updater_message: None,
            updater_version: None,
            updater_identity: crate::nsh::ProjectionId::new(),
            updater_revision: 0,
            updater_presentation: None,
            shell_navigation: None,
            shell_heading: None,
            settings_navigation_presentation: None,
            model_navigation_presentation: None,
            appearance_presentation: None,
            default_model_presentation: None,
            input_preferences_presentation: None,
            provider_management: Default::default(),
            settings_motion: Default::default(),
            section_scrolls: std::array::from_fn(|_| gpui_kit::ScrollHandle::new()),
            model_settings: None,
            settings_help_open: None,
            settings_help_focus: cx.focus_handle().tab_stop(true),
            pricing_advanced: false,
            pending_section: None,
            pending_settings_close: false,
            section: if cx
                .try_global::<SkillsSettingsRequested>()
                .is_some_and(|request| request.0)
            {
                cx.set_global(SkillsSettingsRequested(false));
                6
            } else if cx
                .try_global::<PricingSettingsRequested>()
                .is_some_and(|request| request.0)
            {
                3
            } else {
                1
            },
            usage: Default::default(),
            section_focuses: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            mcp: super::mcp::McpSettingsState::new(cx),
            skills: Default::default(),
            config,
            config_path: None,
            available_key_refs: Vec::new(),
            name_input,
            base_url_input,
            models_input,
            key_input,
            codex_name_input,
            codex_executable_input,
            codex_args_input,
            codex_profile_saving: false,
            agents_presentation: None,
            agents_nsh_draft: None,
            preparation,
            preparation_owner,
            mode_open: false,
            palette_open: false,
            default_permission: super::permissions::DefaultPermissionState::new(cx),
            permission_reset: super::permission_reset::ResetState::new(),
            model_open: false,
            error: error
                .map(|error| SettingsText::from_known(error, &["settings.config.load_failed"])),
            pricing: PricingSettingsProjection::Loading,
            pricing_editor: None,
            pricing_model_input,
            pricing_rate_inputs,
            pricing_presentation: None,
            pricing_nsh_draft: None,
            reasoning: ReasoningSettingsProjection::Loading,
            reasoning_focuses: Vec::new(),
            reasoning_draft: None,
            next_reasoning_operation: 0,
            model_capabilities: ModelCapabilitySettingsProjection::Loading,
            model_capability_models: Vec::new(),
            model_capability_draft: None,
            capability_presentations: Default::default(),
            next_model_capability_operation: 0,
        }
    }

    /// Loads a provider's editable fields without ever loading its key.
    pub(crate) fn begin_edit_provider(&mut self, name: &str, cx: &mut Context<Self>) {
        self.cancel_provider_operation(cx);
        self.provider_management.form = true;
        self.provider_management.selected = Some(name.into());
        let Some(provider) = self
            .config
            .providers
            .iter()
            .find(|provider| provider.name == name)
            .cloned()
        else {
            return;
        };
        self.provider_management.form_base = Some(provider.clone());
        self.provider_management.form_api = provider.api;
        self.name_input
            .update(cx, |input, cx| input.set_text(&provider.name, cx));
        self.base_url_input
            .update(cx, |input, cx| input.set_text(&provider.base_url, cx));
        self.models_input.update(cx, |input, cx| {
            input.set_text(&provider.models.join("\n"), cx)
        });
        self.key_input.update(cx, TextInput::clear);
        self.reset_provider_nsh_draft(cx);
        self.error = None;
        cx.notify();
    }

    /// Validates and persists the Provider form. The candidate config is
    /// cloned so a failed config write cannot replace the in-memory authority.
    pub(crate) fn submit_provider(&mut self, cx: &mut Context<Self>) {
        if !self.provider_nsh_submission_claimed() {
            self.request_provider_nsh_save(cx);
            return;
        }
        let name = self.name_input.read(cx).text().trim().to_string();
        let base_url = self.base_url_input.read(cx).text().trim().to_string();
        let models_text = self.models_input.read(cx).text().to_string();
        let key = self.key_input.read(cx).text().to_string();
        let existing = if self.provider_management.form {
            self.provider_management.form_base.clone()
        } else {
            self.config
                .providers
                .iter()
                .find(|provider| provider.name == name)
                .cloned()
        };
        if !provider_form_is_submittable(&name, &base_url, &key, existing.is_some()) {
            return;
        }
        let models = match parse_provider_models(&models_text) {
            Ok(models) => models,
            Err(error) => {
                self.error = Some(SettingsText::Models(error));
                cx.notify();
                return;
            }
        };
        self.save_provider_background(
            existing.clone(),
            ProviderConfig {
                api: self.provider_management.form_api,
                name: name.clone(),
                base_url,
                models,
                enabled: existing.as_ref().is_none_or(|provider| provider.enabled),
                key_ref: provider_key_ref(existing.as_ref(), &name, &key),
            },
            (!key.is_empty()).then_some(key),
            cx,
        );
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
            self.pricing_nsh_draft = None;
        } else {
            self.release_pricing_draft();
        }
        cx.notify();
    }

    /// Applies a typed reasoning authority projection from the app worker.
    pub fn apply_reasoning_projection(
        &mut self,
        projection: ReasoningSettingsProjection,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            &projection,
            ReasoningSettingsProjection::Ready { error: None, .. }
        ) {
            self.reasoning_draft = None;
        }
        self.reasoning = projection;
        self.rebuild_reasoning_focuses(cx);
        cx.notify();
    }

    /// Verifies that a worker completion still belongs to the exact Settings
    /// save shown to the user. The app controller repeats the route and
    /// generation checks before calling this method.
    pub fn reasoning_save_is_current(&self, generation: u64, operation_id: u64) -> bool {
        matches!(
            &self.reasoning,
            ReasoningSettingsProjection::Saving {
                generation: current_generation,
                operation_id: current_operation,
                ..
            } if *current_generation == generation && *current_operation == operation_id
        )
    }

    pub(crate) fn emit_reasoning_save(
        &mut self,
        index: usize,
        mut profile: ReasoningProfileProjection,
        cx: &mut Context<Self>,
    ) {
        let (generation, base) = match &self.reasoning {
            ReasoningSettingsProjection::Ready {
                generation,
                profiles,
                error: _,
            } => (*generation, profiles.get(index).cloned()),
            _ => return,
        };
        let Some(base) = base else {
            return;
        };
        if profile.provider != base.provider || profile.model != base.model {
            return;
        }
        self.next_reasoning_operation = self.next_reasoning_operation.saturating_add(1);
        let operation_id = self.next_reasoning_operation;
        profile.provider.clone_from(&base.provider);
        profile.model.clone_from(&base.model);
        self.reasoning = ReasoningSettingsProjection::Saving {
            generation,
            operation_id,
            profile: profile.clone(),
        };
        self.reasoning_draft = Some(profile.clone());
        cx.emit(ReasoningProfileSaveRequested {
            generation,
            operation_id,
            base,
            profile,
        });
        cx.notify();
    }

    pub(crate) fn pricing_generation(&self) -> Option<u64> {
        match &self.pricing {
            PricingSettingsProjection::Ready { generation, .. } => Some(*generation),
            _ => None,
        }
    }

    pub(crate) fn pricing_allows_editing(&self) -> bool {
        matches!(
            self.pricing,
            PricingSettingsProjection::Ready {
                draft_reason: None,
                ..
            }
        )
    }

    pub(crate) fn begin_add_custom(&mut self, cx: &mut Context<Self>) {
        if !self.pricing_allows_editing() {
            return;
        }
        self.pricing_model_input.update(cx, TextInput::clear);
        if let Some((_, model)) = &self.model_settings {
            self.pricing_model_input
                .update(cx, |input, cx| input.set_text(model, cx));
        }
        for input in &self.pricing_rate_inputs {
            input.update(cx, TextInput::clear);
        }
        self.pricing_editor = Some(PricingEditor {
            kind: PricingEditorKind::AddCustom,
            model: None,
        });
        self.reset_pricing_draft(cx);
        cx.notify();
    }

    pub(crate) fn begin_edit_pricing(
        &mut self,
        entry: PricingEntryProjection,
        cx: &mut Context<Self>,
    ) {
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
        self.reset_pricing_draft(cx);
        cx.notify();
    }

    pub(crate) fn emit_pricing_mutation(
        &mut self,
        mutation: PricingMutation,
        cx: &mut Context<Self>,
    ) {
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

    pub(crate) fn submit_pricing_editor_values(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn activate_pricing_action(
        &mut self,
        _: &ActivatePricingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.section, 2 | 3) {
            cx.propagate();
            return;
        }
        if self.section == 2
            && let Some(target) = self
                .reasoning_focuses
                .iter()
                .find(|(_, focus)| focus.is_focused(window))
                .map(|(target, _)| target.clone())
        {
            match target {
                ReasoningFocusTarget::Reload => {
                    if !matches!(&self.reasoning, ReasoningSettingsProjection::Saving { .. }) {
                        cx.emit(ReasoningReloadRequested);
                    }
                }
                ReasoningFocusTarget::Preference { index, choice } => {
                    self.select_reasoning_preference(index, choice, cx);
                }
            }
            return;
        }
        cx.propagate();
    }

    pub(crate) fn move_pricing_focus(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section != 2 {
            cx.propagate();
            return;
        }
        if reverse {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        cx.stop_propagation();
    }

    pub(crate) fn next_pricing_action(
        &mut self,
        _: &NextPricingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_pricing_focus(false, window, cx);
    }

    pub(crate) fn previous_pricing_action(
        &mut self,
        _: &PreviousPricingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_pricing_focus(true, window, cx);
    }
}

impl EventEmitter<vega_conversation::types::ModelCatalogRequested> for SettingsView {}
