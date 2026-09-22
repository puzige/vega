//! Provider selection and explicit, cancellable background operations.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;
use vega_conversation::ProviderSettingsService;
use vega_conversation::types::{
    ProviderNetworkAction, ProviderNetworkOutcome, ProviderNetworkRequest, ProviderPatchAction,
    ProviderPatchRequest,
};
use vega_store::context_compaction::{DEFAULT_MODEL_INPUT_LIMIT, DEFAULT_MODEL_OUTPUT_RESERVE};

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
    statuses: BTreeMap<String, String>,
    message: Option<String>,
    model_editor: Option<ModelEditor>,
    next_model_context_request: u64,
    pending_model_reopen: Option<(String, Option<String>)>,
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
    message: Option<String>,
    renamed_from: Option<String>,
}
impl Drop for ProviderManagement {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}
#[derive(Clone)]
struct ProviderDrag(String);
impl Render for ProviderDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_2()
            .bg(theme(cx).colors.bg_elevated)
            .child(self.0.clone())
    }
}
#[derive(Clone)]
enum Command {
    Select(String),
    Add,
    Edit,
    Enable,
    Up,
    Down,
    Discover,
    Test(String),
    ImportPiCredential,
    Cancel,
    Import,
    Candidate(String),
    AddModel,
    EditModel(String),
    DeleteModel(String),
    SaveModel,
    ToggleModelAutomatic,
    ToggleModelUnknown,
    Reload,
}

impl SettingsView {
    pub(crate) fn provider_escape(
        &mut self,
        _: &CloseSettings,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.section == 0
            && (self.provider_management.form
                || self.provider_management.model_editor.is_some()
                || self.provider_management.candidates.is_some()
                || self.provider_management.cancel.is_some())
        {
            self.provider_command(Command::Cancel, cx);
            cx.stop_propagation();
        } else {
            self.cancel_provider_operation(cx);
            cx.propagate();
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
        state.statuses.retain(|_, value| value != "正在测试…");
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
        if self.provider_management.saving {
            return;
        }
        match command {
            Command::Select(name) => {
                self.cancel_provider_operation(cx);
                self.provider_management.selected = Some(name);
                self.provider_management.form = false;
                self.provider_management.model_editor = None;
                self.provider_management.statuses.clear();
                self.provider_management.message = None;
            }
            Command::Add => {
                self.cancel_provider_operation(cx);
                self.provider_management.form = true;
                self.provider_management.form_base = None;
                self.provider_management.form_api = Default::default();
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
            Command::ImportPiCredential => self.import_pi_credential_background(cx),
            Command::Cancel => {
                self.cancel_provider_operation(cx);
                self.provider_management.model_editor = None;
                self.provider_management.form = false;
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
                let original = if let Command::EditModel(model) = command {
                    Some(model)
                } else {
                    None
                };
                let input = cx.new(|cx| TextInput::new(cx, "模型 ID", false).with_tab_stop(true));
                if let Some(model) = &original {
                    input.update(cx, |input, cx| input.set_text(model, cx));
                }
                let input_limit =
                    cx.new(|cx| TextInput::new(cx, "输入最大 token 数", false).with_tab_stop(true));
                let output_limit =
                    cx.new(|cx| TextInput::new(cx, "输出最大 token 数", false).with_tab_stop(true));
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
            Command::DeleteModel(model) => {
                if let Some(p) = self.selected_provider() {
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
                        self.error = Some("请输入一个有效的模型 ID".into());
                    } else if let Some(mut p) = self.selected_provider() {
                        if p.models
                            .iter()
                            .any(|m| m == value.trim() && Some(m) != original.as_ref())
                        {
                            self.error = Some("模型 ID 已存在".into());
                        } else if original.as_deref() == Some(value.trim()) {
                            self.save_model_context(cx);
                        } else {
                            // A config rename and a SQLite policy mutation
                            // have different authorities. Never silently
                            // apply edited limits to the old model identity.
                            if original.is_some() && self.model_context_values_changed(cx) {
                                self.error = Some(
                                    "请先保存模型 ID，再重新打开模型设置修改上下文容量".into(),
                                );
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
            self.error = Some("正在读取模型上下文配置，请稍后重试".into());
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
                    self.error =
                        Some("请输入正整数 token 数，且输入与输出之和不超过 4294967295".into());
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
        editor.message = Some("正在保存模型上下文配置…".into());
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

    /// Only an exact currently open editor accepts this background load.
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
                editor.message = Some(error);
            }
        }
        cx.notify();
    }

    /// Save ACK is fenced by request id and exact provider/model identity.
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
                editor.message = Some("已保存；后续请求生效，进行中的请求不受影响".into());
                self.error = None;
            }
            Err(error) => {
                editor.message = Some(error);
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
            self.error = Some("配置路径不可用".into());
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
            self.key_input.clone(),
        ]
        .map(|input| input.read(cx).text().to_string());
        let worker = cx.background_executor().spawn(async move {
            let config =
                ProviderSettingsService::new(path.clone()).save_provider(base, provider, key)?;
            let refs = path
                .parent()
                .and_then(|root| keystore::available_refs(root).ok())
                .unwrap_or_default();
            Ok::<_, vega_conversation::types::ProviderSettingsError>((config, refs))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                match result {
                    Ok((config, refs)) => {
                        this.config = config;
                        this.available_key_refs = refs;
                        cx.emit(SettingsSaved);
                        if generation != this.provider_management.generation {
                            cx.notify();
                            return;
                        }
                        this.provider_management.selected = Some(name);
                        let inputs = [
                            this.name_input.clone(),
                            this.base_url_input.clone(),
                            this.models_input.clone(),
                            this.key_input.clone(),
                        ];
                        if inputs
                            .iter()
                            .zip(&draft)
                            .all(|(input, text)| input.read(cx).text() == text)
                        {
                            this.provider_management.form = false;
                            this.provider_management.form_base = None;
                            for input in inputs {
                                input.update(cx, TextInput::clear);
                            }
                        }
                        this.provider_management.statuses.clear();
                        this.provider_management.message = None;
                        this.error = None;
                    }
                    Err(error) => {
                        if generation == this.provider_management.generation {
                            this.error = Some(format!("{error}；当前输入仍未保存"));
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
                        this.config = config;
                        this.provider_management.statuses.clear();
                        this.provider_management.message = None;
                        this.error = None;
                    }
                    Err(error) => {
                        if generation == this.provider_management.generation {
                            this.error = Some(error.to_string());
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
        let worker = cx.background_executor().spawn(async move {
            ProviderSettingsService::new(path).patch(ProviderPatchRequest { provider, action })
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                match result {
                    Ok(config) => {
                        this.config = config;
                        cx.emit(SettingsSaved);
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
                            this.error = Some(error.to_string());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn provider_service(&self, path: std::path::PathBuf) -> ProviderSettingsService {
        #[cfg(test)]
        if let Some(source) = self.pi_models_path.clone() {
            return ProviderSettingsService::with_pi_models_path(path, source);
        }
        ProviderSettingsService::new(path)
    }

    fn import_pi_credential_background(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(provider)) = (self.config_path.clone(), self.selected_provider())
        else {
            return;
        };
        if self.provider_management.saving {
            return;
        }
        self.cancel_provider_operation(cx);
        self.provider_management.saving = true;
        self.provider_management.message = Some("正在从 Pi Agent 导入凭据…".into());
        let generation = self.provider_management.generation;
        let name = provider.name.clone();
        let refs_path = path.clone();
        let service = self.provider_service(path);
        let worker = cx.background_executor().spawn(async move {
            let config = service.import_pi_credential(provider)?;
            let refs = refs_path
                .parent()
                .and_then(|root| keystore::available_refs(root).ok())
                .unwrap_or_default();
            Ok::<_, vega_conversation::types::ProviderSettingsError>((config, refs))
        });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.provider_management.saving = false;
                match result {
                    Ok((config, refs)) => {
                        this.config = config;
                        this.available_key_refs = refs;
                        cx.emit(SettingsSaved);
                        if generation != this.provider_management.generation {
                            cx.notify();
                            return;
                        }
                        this.provider_management.selected = Some(name.clone());
                        this.provider_management.message =
                            Some("已从 Pi Agent 导入凭据；供应商已启用".into());
                        this.provider_management.statuses.clear();
                        this.error = None;
                    }
                    Err(error) => {
                        if generation != this.provider_management.generation {
                            cx.notify();
                            return;
                        }
                        this.provider_management.message =
                            Some(format!("从 Pi Agent 导入凭据失败：{error}"));
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
            state.statuses.insert(model.clone(), "正在测试…".into());
        }
        state.message = Some("正在连接…".into());
        let cancel = CancellationToken::new();
        state.cancel = Some(cancel.clone());
        let worker = cx.background_executor().spawn(async move {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => {
                    runtime.block_on(ProviderSettingsService::new(path).network(request, cancel))
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
                        format!("读取到 {} 个模型；请选择后添加", models.len())
                    }
                    Ok(ProviderNetworkOutcome::ModelTestSucceeded) => "连接成功".into(),
                    Err(error) => error.to_string(),
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
    fn provider_button(
        &self,
        id: String,
        label: &'static str,
        command: Command,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let key_command = command.clone();
        let selector = id.clone();
        div()
            .id(id)
            .debug_selector(move || selector.clone())
            .key_context("ProviderControl")
            .focusable()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .text_color(if enabled {
                colors.text_secondary
            } else {
                colors.text_tertiary
            })
            .focus_visible(move |s| s.bg(colors.bg_active))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(move |s| s.bg(colors.bg_hover))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.provider_command(command.clone(), cx)
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.provider_command(key_command.clone(), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
            })
            .child(label)
            .into_any_element()
    }
    pub(crate) fn render_provider_management(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.sync_provider_edit_focuses(cx);
        if self.provider_management.selected.is_none() {
            self.provider_management.selected =
                self.config.providers.first().map(|p| p.name.clone());
            self.provider_management.form = self.config.providers.is_empty();
        }
        let colors = theme(cx).colors;
        let busy = self.provider_management.saving;
        let network_busy = self.provider_management.cancel.is_some();
        let mut list = div()
            .id("provider-list")
            .h_full()
            .w(px(Layout::PROVIDER_LIST_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_y_scroll();
        for (index, p) in self.config.providers.clone().into_iter().enumerate() {
            let selected = self.provider_management.selected.as_ref() == Some(&p.name);
            let name = p.name.clone();
            let key_name = name.clone();
            list = list.child(
                div()
                    .id(("provider-select", index))
                    .track_focus(&self.provider_edit_focuses[index].1)
                    .tab_stop(true)
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .when(selected, |row| row.bg(colors.bg_active))
                    .cursor_pointer()
                    .focus_visible(move |s| s.bg(colors.bg_hover))
                    .on_drag(ProviderDrag(name.clone()), |drag, _, _, cx| {
                        cx.new(|_| drag.clone())
                    })
                    .on_drop(cx.listener(move |this, drag: &ProviderDrag, _, cx| {
                        if !this.provider_management.saving
                            && let Some(from) =
                                this.config.providers.iter().position(|p| p.name == drag.0)
                        {
                            this.provider_management.selected = Some(drag.0.clone());
                            this.move_provider(from, index, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.provider_command(Command::Select(name.clone()), cx)
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.provider_command(Command::Select(key_name.clone()), cx);
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(
                        div()
                            .text_color(if p.enabled {
                                colors.success
                            } else {
                                colors.text_tertiary
                            })
                            .child("●"),
                    )
                    .child(div().min_w_0().truncate().child(p.name)),
            );
        }
        list = list
            .child(self.provider_button(
                "provider-add".into(),
                "+ 添加供应商",
                Command::Add,
                !busy,
                cx,
            ))
            .child(self.provider_button(
                "provider-reload".into(),
                "重新读取",
                Command::Reload,
                !busy,
                cx,
            ));
        // Keep the flow intrinsically sized; the outer viewport owns scrolling.
        // A height-constrained flex column can shrink truncated text rows to zero.
        let mut detail = div()
            .debug_selector(|| "provider-detail-flow".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3();
        if self.provider_management.form {
            detail = detail
                .child(self.render_add_form(cx))
                .child(self.provider_button(
                    "provider-form-cancel".into(),
                    "取消",
                    Command::Cancel,
                    !busy,
                    cx,
                ));
        } else if let Some(p) = self.selected_provider() {
            let api_label = match p.api {
                vega_conversation::types::ProviderApi::ChatCompletions => {
                    "API 格式 · Chat Completions"
                }
                vega_conversation::types::ProviderApi::Responses => "API 格式 · Responses",
            };
            let index = self
                .config
                .providers
                .iter()
                .position(|candidate| candidate.name == p.name)
                .unwrap_or(0);
            detail = detail
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(Typography::HEADING_CARD))
                                .child(p.name.clone()),
                        )
                        .child(self.provider_button(
                            "provider-enabled".into(),
                            if p.enabled { "已启用" } else { "已禁用" },
                            Command::Enable,
                            !busy,
                            cx,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.provider_button(
                            "provider-up".into(),
                            "上移",
                            Command::Up,
                            !busy && index > 0,
                            cx,
                        ))
                        .child(self.provider_button(
                            "provider-down".into(),
                            "下移",
                            Command::Down,
                            !busy && index + 1 < self.config.providers.len(),
                            cx,
                        ))
                        .child(self.provider_button(
                            "provider-edit".into(),
                            "编辑供应商",
                            Command::Edit,
                            !busy,
                            cx,
                        ))
                        .child(self.provider_button(
                            "provider-import-pi".into(),
                            "从 Pi Agent 导入凭据",
                            Command::ImportPiCredential,
                            !busy,
                            cx,
                        )),
                )
                .child(div().text_color(colors.text_secondary).child("Base URL"))
                .child(
                    div()
                        .debug_selector(|| "provider-base-url".into())
                        .min_w_0()
                        .truncate()
                        .child(p.base_url.clone()),
                )
                .child(
                    div()
                        .debug_selector(move || format!("provider-api-format:{api_label}"))
                        .text_color(colors.text_secondary)
                        .child(api_label),
                )
                .child(div().text_color(colors.text_secondary).child(
                    if self.available_key_refs.contains(&p.key_ref) {
                        KEY_STORED_PLACEHOLDER
                    } else {
                        "需重新输入 API Key"
                    },
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(self.provider_button(
                            "provider-discover".into(),
                            "发现模型",
                            Command::Discover,
                            !busy && !network_busy && p.enabled,
                            cx,
                        ))
                        .child(self.provider_button(
                            "provider-add-model".into(),
                            "添加模型",
                            Command::AddModel,
                            !busy,
                            cx,
                        ))
                        .when(network_busy, |row| {
                            row.child(self.provider_button(
                                "provider-cancel".into(),
                                "停止",
                                Command::Cancel,
                                true,
                                cx,
                            ))
                        }),
                )
                .child(
                    div()
                        .text_color(colors.text_tertiary)
                        .child("测试会发送少量固定文本请求；不会使用聊天内容。"),
                );
            for (index, model) in p.models.iter().enumerate() {
                detail = detail.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .border_b_1()
                        .border_color(colors.border_subtle)
                        .py_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(div().flex_1().min_w_0().truncate().child(model.clone()))
                                .child(self.provider_button(
                                    format!("model-test-{index}"),
                                    "测试",
                                    Command::Test(model.clone()),
                                    !busy && !network_busy && p.enabled,
                                    cx,
                                ))
                                .child(self.provider_button(
                                    format!("model-edit-{index}"),
                                    "编辑",
                                    Command::EditModel(model.clone()),
                                    !busy,
                                    cx,
                                ))
                                .child(self.provider_button(
                                    format!("model-delete-{index}"),
                                    "删除",
                                    Command::DeleteModel(model.clone()),
                                    !busy,
                                    cx,
                                )),
                        )
                        .children(self.provider_management.statuses.get(model).map(|status| {
                            div()
                                .text_color(colors.text_secondary)
                                .child(status.clone())
                        })),
                );
            }
            if let Some(editor) = &self.provider_management.model_editor {
                let can_edit = !busy && !editor.saving;
                detail = detail
                    .child(editor.model_input.clone())
                    .when(editor.original.is_some(), |detail| {
                        let source = match editor.source {
                            PolicySource::Loading => "正在读取模型上下文配置…".to_string(),
                            PolicySource::AssumedDefault => format!(
                                "默认假设值：输入 {DEFAULT_MODEL_INPUT_LIMIT} token，输出 {DEFAULT_MODEL_OUTPUT_RESERVE} token；不是供应商验证容量。小窗口模型请按实际能力修改。"
                            ),
                            PolicySource::Saved => "已保存的模型容量；请按供应商真实能力核对。".to_string(),
                            PolicySource::Unknown => "容量未知：仍可发送，但不会按预算自动压缩。".to_string(),
                        };
                        detail
                            .child(
                                div()
                                    .debug_selector(|| "model-context-source".into())
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_secondary)
                                    .child(source),
                            )
                            .when(editor.legacy_present, |detail| {
                                detail.child(
                                    div()
                                        .debug_selector(|| "model-context-legacy-notice".into())
                                        .text_size(px(Typography::METADATA))
                                        .text_color(colors.warning)
                                        .child("旧会话级上下文设置已保留，但不再生效；请在此配置模型容量。"),
                                )
                            })
                            .children(editor.renamed_from.as_ref().map(|previous| {
                                div()
                                    .debug_selector(|| "model-context-rename-notice".into())
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.warning)
                                    .child(format!(
                                        "模型 ID 已从 {previous} 更改；旧 ID 的容量配置不会自动沿用。请检查并保存新模型容量。"
                                    ))
                            }))
                            .when(!editor.unknown, |detail| detail.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .child("输入最大限制（token）")
                                            .child(
                                                div()
                                                    .debug_selector(|| "model-context-input".into())
                                                    .child(editor.input_limit.clone()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .child("输出最大限制（token）")
                                            .child(
                                                div()
                                                    .debug_selector(|| "model-context-output".into())
                                                    .child(editor.output_limit.clone()),
                                            ),
                                    ),
                            ))
                            .child(self.provider_button(
                                "model-context-auto".into(),
                                if editor.automatic {
                                    "自动压缩：开启"
                                } else {
                                    "自动压缩：关闭"
                                },
                                Command::ToggleModelAutomatic,
                                can_edit,
                                cx,
                            ))
                            .child(self.provider_button(
                                "model-context-unknown".into(),
                                if editor.unknown {
                                    "容量未知：已选择"
                                } else {
                                    "容量未知：未选择"
                                },
                                Command::ToggleModelUnknown,
                                can_edit,
                                cx,
                            ))
                    })
                    .children(editor.message.as_ref().map(|message| {
                        div()
                            .debug_selector(|| "model-context-message".into())
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .child(message.clone())
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.provider_button(
                                "model-save".into(),
                                "保存模型设置",
                                Command::SaveModel,
                                can_edit,
                                cx,
                            ))
                            .child(self.provider_button(
                                "model-cancel".into(),
                                "取消",
                                Command::Cancel,
                                can_edit,
                                cx,
                            )),
                    );
            }
            if let Some(models) = self.provider_management.candidates.clone() {
                for (index, model) in models.into_iter().enumerate() {
                    let existing = p.models.contains(&model);
                    let checked = self.provider_management.checked.contains(&model);
                    detail = detail.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.provider_button(
                                format!("candidate-{index}"),
                                if existing {
                                    "已添加"
                                } else if checked {
                                    "☑"
                                } else {
                                    "☐"
                                },
                                Command::Candidate(model.clone()),
                                !existing && !busy,
                                cx,
                            ))
                            .child(div().min_w_0().truncate().child(model)),
                    );
                }
                detail = detail.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(self.provider_button(
                            "provider-import".into(),
                            "添加所选",
                            Command::Import,
                            !busy && !self.provider_management.checked.is_empty(),
                            cx,
                        ))
                        .child(self.provider_button(
                            "discovery-cancel".into(),
                            "取消",
                            Command::Cancel,
                            !busy,
                            cx,
                        )),
                );
            }
        }
        if let Some(message) = &self.provider_management.message {
            let selector = if message.contains("Pi Agent") {
                "provider-import-status"
            } else {
                "provider-network-message"
            };
            detail = detail.child(
                div()
                    .debug_selector(move || selector.into())
                    .text_color(colors.text_secondary)
                    .child(message.clone()),
            );
        }
        div()
            .id("provider-management")
            .size_full()
            .min_h_0()
            .on_action(cx.listener(Self::provider_escape))
            .flex()
            .items_start()
            .min_w_0()
            .gap_4()
            .on_key_down(
                cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "enter"
                        && this
                            .provider_management
                            .model_editor
                            .as_ref()
                            .is_some_and(|editor| {
                                editor
                                    .model_input
                                    .read(cx)
                                    .focus_handle(cx)
                                    .is_focused(window)
                            })
                    {
                        this.provider_command(Command::SaveModel, cx);
                        cx.stop_propagation();
                        return;
                    }
                    if event.keystroke.key == "escape"
                        && (this.provider_management.form
                            || this.provider_management.model_editor.is_some()
                            || this.provider_management.candidates.is_some()
                            || this.provider_management.cancel.is_some())
                    {
                        this.provider_command(Command::Cancel, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(list)
            .child(
                div()
                    .id("provider-detail")
                    .debug_selector(|| "provider-detail-viewport".into())
                    .h_full()
                    .min_h_0()
                    .min_w_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .child(detail),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
