use super::*;

/// the config — each time settings is opened).
pub struct SettingsView {
    pub(crate) config: AppConfig,
    pub(crate) name_input: Entity<TextInput>,
    pub(crate) base_url_input: Entity<TextInput>,
    pub(crate) key_input: Entity<TextInput>,
    pub(crate) mode_open: bool,
    pub(crate) model_open: bool,
    /// Inline error message (ui-spec §4.6: no modals); empty until an IO or
    /// Keychain failure occurs.
    pub(crate) error: Option<String>,
    pub(crate) pricing: PricingSettingsProjection,
    pub(crate) pricing_editor: Option<PricingEditor>,
    pub(crate) pricing_model_input: Entity<TextInput>,
    pub(crate) pricing_rate_inputs: [Entity<TextInput>; 8],
    pub(crate) pricing_focuses: Vec<(PricingFocusTarget, FocusHandle)>,
}

impl EventEmitter<PricingMutationRequested> for SettingsView {}
impl EventEmitter<PricingReloadRequested> for SettingsView {}
impl EventEmitter<PricingRetryRequested> for SettingsView {}
impl EventEmitter<PricingDiscardRequested> for SettingsView {}

pub(crate) const PRICING_INPUT_BYTES_LIMIT: usize = 1024 * 1024;

impl SettingsView {
    /// Loads the current config and creates empty form inputs.
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 构造时载入现有配置：这是"重启后配置恢复"的落点。失败时以内联
        // 错误提示，并以默认配置继续渲染（ui-spec §4.6：不弹模态）。
        let (config, error) = match config::load() {
            Ok(config) => (config, None),
            Err(err) => (AppConfig::default(), Some(format!("配置加载失败：{err}"))),
        };
        Self::from_config(config, error, cx)
    }

    pub(crate) fn from_config(
        config: AppConfig,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input = cx.new(|cx| TextInput::new(cx, "名称", false));
        let base_url_input = cx.new(|cx| TextInput::new(cx, "Base URL", false));
        let key_input = cx.new(|cx| TextInput::new(cx, "API Key", true));
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
            config,
            name_input,
            base_url_input,
            key_input,
            mode_open: false,
            model_open: false,
            error,
            pricing: PricingSettingsProjection::Loading,
            pricing_editor: None,
            pricing_model_input,
            pricing_rate_inputs,
            pricing_focuses: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(cx: &mut Context<Self>) -> Self {
        Self::from_config(AppConfig::default(), None, cx)
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
        let target = self
            .pricing_focuses
            .iter()
            .find(|(_, focus)| focus.is_focused(window))
            .map(|(target, _)| target.clone());
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
        let Some(current) = self
            .pricing_focuses
            .iter()
            .position(|(_, focus)| focus.is_focused(window))
        else {
            cx.propagate();
            return;
        };
        let next = if reverse {
            current.checked_sub(1)
        } else {
            current
                .checked_add(1)
                .filter(|next| *next < self.pricing_focuses.len())
        };
        let Some(next) = next else {
            cx.propagate();
            return;
        };
        self.pricing_focuses[next].1.focus(window, cx);
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
