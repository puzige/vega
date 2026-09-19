use super::*;

#[cfg(test)]
type ProviderKeyWriter = std::sync::Arc<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;
#[cfg(test)]
type ProviderConfigSaver = std::sync::Arc<dyn Fn(&AppConfig) -> Result<(), String> + Send + Sync>;

/// the config — each time settings is opened).
pub struct SettingsView {
    pub(crate) provider_management: super::provider_management::ProviderManagement,
    pub(crate) section: usize,
    pub(crate) usage: super::usage::UsageState,
    pub(crate) usage_focuses: [FocusHandle; 6],
    pub(crate) section_focuses: [FocusHandle; 5],
    pub(crate) config: AppConfig,
    pub(crate) config_path: Option<std::path::PathBuf>,
    pub(crate) available_key_refs: Vec<String>,
    pub(crate) name_input: Entity<TextInput>,
    pub(crate) base_url_input: Entity<TextInput>,
    pub(crate) models_input: Entity<TextInput>,
    pub(crate) key_input: Entity<TextInput>,
    pub(crate) provider_save_focus: FocusHandle,
    pub(crate) provider_edit_focuses: Vec<(String, FocusHandle)>,
    pub(crate) mode_open: bool,
    pub(crate) model_open: bool,
    /// Inline error message (ui-spec §4.6: no modals); empty until an IO or
    /// local credential failure occurs.
    pub(crate) error: Option<String>,
    #[cfg(test)]
    pub(crate) key_writer: Option<ProviderKeyWriter>,
    #[cfg(test)]
    pub(crate) config_saver: Option<ProviderConfigSaver>,
    /// Owned Pi source override for mounted Settings tests. Production leaves
    /// this unset so the service resolves `$HOME/.pi/agent/models.json` only
    /// after the explicit import action.
    #[cfg(test)]
    pub(crate) pi_models_path: Option<std::path::PathBuf>,
    pub(crate) pricing: PricingSettingsProjection,
    pub(crate) pricing_editor: Option<PricingEditor>,
    pub(crate) pricing_model_input: Entity<TextInput>,
    pub(crate) pricing_rate_inputs: [Entity<TextInput>; 8],
    pub(crate) pricing_focuses: Vec<(PricingFocusTarget, FocusHandle)>,
    /// Worker-owned independent reasoning capability/preferences projection.
    pub(crate) reasoning: ReasoningSettingsProjection,
    /// Focus handles for the explicit reasoning capability editor. These are
    /// rebuilt with the projection so Tab/Enter/Space use the same action
    /// route as the existing Settings controls.
    pub(crate) reasoning_focuses: Vec<(ReasoningFocusTarget, FocusHandle)>,
    /// Last failed edit retained for an explicit retry/reload decision.
    pub(crate) reasoning_draft: Option<ReasoningProfileProjection>,
    pub(crate) next_reasoning_operation: u64,
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.section_focuses[self.section].clone()
    }
}

impl EventEmitter<PricingMutationRequested> for SettingsView {}
impl EventEmitter<PricingReloadRequested> for SettingsView {}
impl EventEmitter<PricingRetryRequested> for SettingsView {}
impl EventEmitter<PricingDiscardRequested> for SettingsView {}
impl EventEmitter<SettingsSaved> for SettingsView {}
impl EventEmitter<ModelContextLoadRequested> for SettingsView {}
impl EventEmitter<ModelContextSaveRequested> for SettingsView {}
impl EventEmitter<ReasoningProfileSaveRequested> for SettingsView {}
impl EventEmitter<ReasoningReloadRequested> for SettingsView {}

pub(crate) const PRICING_INPUT_BYTES_LIMIT: usize = 1024 * 1024;
pub(crate) const PROVIDER_MODELS_MIN_ROWS: usize = 2;
pub(crate) const PROVIDER_MODELS_MAX_ROWS: usize = 4;
/// Two `p_2` edges plus two 1px borders around the body-text viewport.
pub(crate) const PROVIDER_MODELS_FRAME_INSET: f32 = 18.0;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ProviderFocusTarget {
    Edit(String),
    Name,
    BaseUrl,
    Models,
    Key,
    Save,
}

impl SettingsView {
    /// Loads the current config and creates empty form inputs.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::from_path(
            vega_store::paths::config_dir().map(|root| root.join("config.toml")),
            cx,
        )
    }

    /// Construct Settings using the same explicit config path as the app worker.
    pub fn from_path(path: Option<std::path::PathBuf>, cx: &mut Context<Self>) -> Self {
        let (config, error) = match path.as_deref().map(config::load_from) {
            Some(Ok(config)) => (config, None),
            _ => (AppConfig::default(), Some("配置加载失败".into())),
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
        cx.observe_global::<SettingsOpen>(|this, cx| {
            if !cx.global::<SettingsOpen>().0 {
                this.cancel_provider_operation(cx);
            }
        })
        .detach();
        let name_input = cx.new(|cx| TextInput::new(cx, "名称", false).with_tab_stop(true));
        let base_url_input = cx.new(|cx| TextInput::new(cx, "Base URL", false).with_tab_stop(true));
        let models_input = cx.new(|cx| {
            TextInput::new_multiline(cx, "模型 ID（每行一个）", PROVIDER_MODELS_MIN_ROWS)
                .with_row_bounds(PROVIDER_MODELS_MIN_ROWS, PROVIDER_MODELS_MAX_ROWS)
                .with_tab_stop(true)
        });
        let key_input = cx.new(|cx| TextInput::new(cx, "API Key", true).with_tab_stop(true));
        let pricing_model_input = cx.new(|cx| TextInput::new(cx, "模型 ID", false));
        let pricing_rate_inputs = [
            cx.new(|cx| TextInput::new(cx, "Base Input", false)),
            cx.new(|cx| TextInput::new(cx, "Base Output", false)),
            cx.new(|cx| TextInput::new(cx, "Base Cache Read", false)),
            cx.new(|cx| TextInput::new(cx, "Base Cache Write", false)),
            cx.new(|cx| TextInput::new(cx, "Peak Input", false)),
            cx.new(|cx| TextInput::new(cx, "Peak Output", false)),
            cx.new(|cx| TextInput::new(cx, "Peak Cache Read", false)),
            cx.new(|cx| TextInput::new(cx, "Peak Cache Write", false)),
        ];
        Self {
            provider_management: Default::default(),
            section: if cx
                .try_global::<PricingSettingsRequested>()
                .is_some_and(|request| request.0)
            {
                3
            } else {
                1
            },
            usage: Default::default(),
            usage_focuses: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            section_focuses: std::array::from_fn(|_| cx.focus_handle().tab_stop(true)),
            config,
            config_path: None,
            available_key_refs: Vec::new(),
            name_input,
            base_url_input,
            models_input,
            key_input,
            provider_save_focus: cx.focus_handle().tab_stop(true),
            provider_edit_focuses: Vec::new(),
            mode_open: false,
            model_open: false,
            error,
            #[cfg(test)]
            key_writer: None,
            #[cfg(test)]
            config_saver: None,
            #[cfg(test)]
            pi_models_path: None,
            pricing: PricingSettingsProjection::Loading,
            pricing_editor: None,
            pricing_model_input,
            pricing_rate_inputs,
            pricing_focuses: Vec::new(),
            reasoning: ReasoningSettingsProjection::Loading,
            reasoning_focuses: Vec::new(),
            reasoning_draft: None,
            next_reasoning_operation: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(cx: &mut Context<Self>) -> Self {
        Self::from_config(AppConfig::default(), None, cx)
    }

    /// Keeps edit-row focus handles stable while the provider list changes.
    pub(crate) fn sync_provider_edit_focuses(&mut self, cx: &mut Context<Self>) {
        let previous = std::mem::take(&mut self.provider_edit_focuses);
        self.provider_edit_focuses = self
            .config
            .providers
            .iter()
            .map(|provider| {
                let focus = previous
                    .iter()
                    .find(|(name, _)| name == &provider.name)
                    .map(|(_, focus)| focus.clone())
                    .unwrap_or_else(|| cx.focus_handle().tab_stop(true));
                (provider.name.clone(), focus)
            })
            .collect();
    }

    /// Returns the visible Provider controls in their DOM order.
    pub(crate) fn provider_focuses(&self, cx: &App) -> Vec<(ProviderFocusTarget, FocusHandle)> {
        let mut focuses = self
            .provider_edit_focuses
            .iter()
            .map(|(name, focus)| (ProviderFocusTarget::Edit(name.clone()), focus.clone()))
            .collect::<Vec<_>>();
        focuses.extend([
            (
                ProviderFocusTarget::Name,
                self.name_input.read(cx).focus_handle(cx),
            ),
            (
                ProviderFocusTarget::BaseUrl,
                self.base_url_input.read(cx).focus_handle(cx),
            ),
            (
                ProviderFocusTarget::Models,
                self.models_input.read(cx).focus_handle(cx),
            ),
            (
                ProviderFocusTarget::Key,
                self.key_input.read(cx).focus_handle(cx),
            ),
            (ProviderFocusTarget::Save, self.provider_save_focus.clone()),
        ]);
        focuses
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
        self.name_input
            .update(cx, |input, cx| input.set_text(&provider.name, cx));
        self.base_url_input
            .update(cx, |input, cx| input.set_text(&provider.base_url, cx));
        self.models_input.update(cx, |input, cx| {
            input.set_text(&provider.models.join("\n"), cx)
        });
        self.key_input.update(cx, TextInput::clear);
        self.error = None;
        cx.notify();
    }

    /// Handles Enter/Space on Provider actions and Cmd+Enter from the form.
    pub(crate) fn activate_provider_action(
        &mut self,
        _: &ActivateProviderAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section != 0 {
            cx.propagate();
            return;
        }
        let target = self
            .provider_focuses(cx)
            .into_iter()
            .find(|(_, focus)| focus.is_focused(window))
            .map(|(target, _)| target);
        let Some(target) = target else {
            cx.propagate();
            return;
        };
        match target {
            ProviderFocusTarget::Edit(name) => {
                self.begin_edit_provider(&name, cx);
                self.name_input.read(cx).focus_handle(cx).focus(window, cx);
            }
            ProviderFocusTarget::Name
            | ProviderFocusTarget::BaseUrl
            | ProviderFocusTarget::Models
            | ProviderFocusTarget::Key
            | ProviderFocusTarget::Save => self.submit_provider(cx),
        }
    }

    pub(crate) fn move_provider_focus(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section != 0 {
            cx.propagate();
            return;
        }
        let focuses = self.provider_focuses(cx);
        let Some(current) = focuses
            .iter()
            .position(|(_, focus)| focus.is_focused(window))
        else {
            cx.propagate();
            return;
        };
        let next = if reverse {
            current.checked_sub(1)
        } else {
            current.checked_add(1).filter(|next| *next < focuses.len())
        };
        let Some(next) = next else {
            cx.propagate();
            return;
        };
        focuses[next].1.focus(window, cx);
    }

    pub(crate) fn next_provider_action(
        &mut self,
        _: &NextProviderAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_provider_focus(false, window, cx);
    }

    pub(crate) fn previous_provider_action(
        &mut self,
        _: &PreviousProviderAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_provider_focus(true, window, cx);
    }

    #[cfg(test)]
    fn write_provider_key(&self, name: &str, key: &str) -> Result<(), String> {
        #[cfg(test)]
        if let Some(writer) = &self.key_writer {
            return writer(name, key);
        }
        keystore::set_key(
            self.config_path
                .as_deref()
                .and_then(std::path::Path::parent)
                .ok_or("配置路径不可用")?,
            name,
            key,
        )
        .map_err(|error| error.to_string())
    }

    #[cfg(test)]
    pub(crate) fn persist_config(&self, config: &AppConfig) -> Result<(), String> {
        #[cfg(test)]
        if let Some(saver) = &self.config_saver {
            return saver(config);
        }
        config
            .save_to(self.config_path.as_deref().ok_or("配置路径不可用")?)
            .map_err(|error| error.to_string())
    }

    /// Validates and persists the Provider form. The candidate config is
    /// cloned so a failed config write cannot replace the in-memory authority.
    pub(crate) fn submit_provider(&mut self, cx: &mut Context<Self>) {
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
                self.error = Some(provider_models_error_label(&error));
                cx.notify();
                return;
            }
        };
        #[cfg(test)]
        let use_worker = self.config_saver.is_none() && self.key_writer.is_none();
        #[cfg(not(test))]
        let use_worker = true;
        if use_worker {
            self.save_provider_background(
                existing.clone(),
                ProviderConfig {
                    name: name.clone(),
                    base_url,
                    models,
                    enabled: existing.as_ref().is_none_or(|provider| provider.enabled),
                    key_ref: provider_key_ref(existing.as_ref(), &name, &key),
                },
                (!key.is_empty()).then_some(key),
                cx,
            );
            #[cfg(test)]
            return;
        }
        #[cfg(test)]
        {
            let key_ref = if key.is_empty() {
                provider_key_ref(existing.as_ref(), &name, &key)
            } else {
                if let Err(error) = self.write_provider_key(&name, &key) {
                    self.error = Some(format!("本地凭据写入失败：{error}"));
                    cx.notify();
                    return;
                }
                provider_key_ref(existing.as_ref(), &name, &key)
            };
            let mut candidate = self.config.clone();
            upsert_provider(
                &mut candidate.providers,
                ProviderConfig {
                    name: name.clone(),
                    enabled: existing.as_ref().is_none_or(|provider| provider.enabled),
                    base_url,
                    models,
                    key_ref,
                },
            );
            if let Err(error) = self.persist_config(&candidate) {
                self.error = Some(format!("配置保存失败：{error}；当前输入仍未保存"));
                cx.notify();
                return;
            }
            self.config = candidate;
            self.provider_management.form = false;
            self.provider_management.selected = Some(name);
            self.refresh_key_refs();
            self.sync_provider_edit_focuses(cx);
            self.name_input.update(cx, TextInput::clear);
            self.base_url_input.update(cx, TextInput::clear);
            self.models_input.update(cx, TextInput::clear);
            self.key_input.update(cx, TextInput::clear);
            self.error = None;
            cx.emit(SettingsSaved);
            cx.notify();
        }
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
        }
        self.rebuild_pricing_focuses(cx);
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

    /// Read-only projection used by app-level acceptance tests and route
    /// adapters; edits still go through the typed actions below.
    pub fn reasoning_projection(&self) -> &ReasoningSettingsProjection {
        &self.reasoning
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

    pub(crate) fn rebuild_pricing_focuses(&mut self, cx: &mut Context<Self>) {
        let mut targets = Vec::new();
        match &self.pricing {
            PricingSettingsProjection::Invalid(_) => targets.push(PricingFocusTarget::Reload),
            PricingSettingsProjection::Ready {
                entries,
                draft_reason,
                ..
            } => {
                targets.push(PricingFocusTarget::Reload);
                if draft_reason.is_some() {
                    targets.push(PricingFocusTarget::Retry);
                    targets.push(PricingFocusTarget::Discard);
                } else {
                    targets.push(PricingFocusTarget::Add);
                    for index in 0..entries.len() {
                        targets.push(PricingFocusTarget::Edit(index));
                        targets.push(PricingFocusTarget::Secondary(index));
                    }
                    if self.pricing_editor.is_some() {
                        targets.push(PricingFocusTarget::Save);
                        targets.push(PricingFocusTarget::Cancel);
                    }
                }
            }
            PricingSettingsProjection::Loading
            | PricingSettingsProjection::Saving { .. }
            | PricingSettingsProjection::Reloading => {}
        }
        self.pricing_focuses = targets
            .into_iter()
            .map(|target| (target, cx.focus_handle().tab_stop(true)))
            .collect();
    }

    pub(crate) fn pricing_focus(&self, target: &PricingFocusTarget) -> Option<FocusHandle> {
        self.pricing_focuses
            .iter()
            .find(|(candidate, _)| candidate == target)
            .map(|(_, focus)| focus.clone())
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
        for input in &self.pricing_rate_inputs {
            input.update(cx, TextInput::clear);
        }
        self.pricing_editor = Some(PricingEditor {
            kind: PricingEditorKind::AddCustom,
            model: None,
        });
        self.rebuild_pricing_focuses(cx);
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
        self.rebuild_pricing_focuses(cx);
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

    pub(crate) fn submit_pricing_editor(&mut self, cx: &mut Context<Self>) {
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
                .map(|(target, _)| *target)
        {
            match target {
                ReasoningFocusTarget::Reload => {
                    if !matches!(&self.reasoning, ReasoningSettingsProjection::Saving { .. }) {
                        cx.emit(ReasoningReloadRequested);
                    }
                }
                ReasoningFocusTarget::Template { index, template } => {
                    self.apply_reasoning_template(index, template, cx);
                }
                ReasoningFocusTarget::Protocol(index) => {
                    self.cycle_reasoning_protocol(index, cx);
                }
                ReasoningFocusTarget::Support(index) => {
                    self.cycle_reasoning_support(index, cx);
                }
                ReasoningFocusTarget::Effort { index, effort } => {
                    self.toggle_reasoning_effort(index, effort, cx);
                }
                ReasoningFocusTarget::Preference(index) => {
                    self.cycle_reasoning_preference(index, cx);
                }
                ReasoningFocusTarget::Disabled(index) => {
                    self.toggle_reasoning_disabled(index, cx);
                }
                ReasoningFocusTarget::Replay(index) => {
                    self.toggle_reasoning_replay(index, cx);
                }
            }
            return;
        }
        let target = self
            .pricing_focuses
            .iter()
            .find(|(_, focus)| focus.is_focused(window))
            .map(|(target, _)| target.clone());
        if self.section != 3 {
            cx.propagate();
            return;
        }
        let Some(target) = target else {
            cx.propagate();
            return;
        };
        match target {
            PricingFocusTarget::Reload => cx.emit(PricingReloadRequested),
            PricingFocusTarget::Add => self.begin_add_custom(cx),
            PricingFocusTarget::Edit(index) => {
                if let PricingSettingsProjection::Ready {
                    entries,
                    draft_reason: None,
                    ..
                } = &self.pricing
                    && let Some(entry) = entries.get(index).cloned()
                {
                    self.begin_edit_pricing(entry, cx);
                }
            }
            PricingFocusTarget::Secondary(index) => {
                if let PricingSettingsProjection::Ready {
                    entries,
                    draft_reason: None,
                    ..
                } = &self.pricing
                    && let Some(entry) = entries.get(index)
                {
                    let mutation = if entry.kind == PricingEntryKind::CustomStatic {
                        PricingMutation::DeleteCustom {
                            model: entry.model.clone(),
                        }
                    } else {
                        PricingMutation::ResetBuiltin {
                            model: entry.model.clone(),
                        }
                    };
                    self.emit_pricing_mutation(mutation, cx);
                }
            }
            PricingFocusTarget::Retry => {
                if let Some(generation) = self.pricing_generation() {
                    cx.emit(PricingRetryRequested { generation });
                }
            }
            PricingFocusTarget::Discard => {
                if let Some(generation) = self.pricing_generation() {
                    cx.emit(PricingDiscardRequested { generation });
                }
            }
            PricingFocusTarget::Save => self.submit_pricing_editor(cx),
            PricingFocusTarget::Cancel => {
                self.pricing_editor = None;
                self.rebuild_pricing_focuses(cx);
                cx.notify();
            }
        }
    }

    pub(crate) fn move_pricing_focus(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focuses: Vec<FocusHandle> = match self.section {
            2 => self
                .reasoning_focuses
                .iter()
                .map(|(_, focus)| focus.clone())
                .collect(),
            3 => self
                .pricing_focuses
                .iter()
                .map(|(_, focus)| focus.clone())
                .collect(),
            _ => {
                cx.propagate();
                return;
            }
        };
        let Some(current) = focuses.iter().position(|focus| focus.is_focused(window)) else {
            cx.propagate();
            return;
        };
        let next = if reverse {
            current.checked_sub(1)
        } else {
            current.checked_add(1).filter(|next| *next < focuses.len())
        };
        if let Some(next) = next {
            focuses[next].focus(window, cx);
        } else {
            cx.propagate();
        }
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
