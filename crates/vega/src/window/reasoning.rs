use super::*;

pub(super) fn reasoning_error_code(
    error: &vega_store::reasoning::ReasoningConfigError,
) -> ReasoningSettingsErrorCode {
    match error {
        vega_store::reasoning::ReasoningConfigError::Io(_) => ReasoningSettingsErrorCode::Io,
        vega_store::reasoning::ReasoningConfigError::Parse(_)
        | vega_store::reasoning::ReasoningConfigError::Serialize(_)
        | vega_store::reasoning::ReasoningConfigError::Utf8(_)
        | vega_store::reasoning::ReasoningConfigError::UnsupportedVersion(_)
        | vega_store::reasoning::ReasoningConfigError::InvalidProfile(_) => {
            ReasoningSettingsErrorCode::Invalid
        }
        vega_store::reasoning::ReasoningConfigError::ConcurrentModification => {
            ReasoningSettingsErrorCode::Conflict
        }
    }
}

fn reasoning_profile_patch(
    authority: &vega_store::reasoning::ReasoningConfig,
    base: &ReasoningProfileProjection,
    candidate: &ReasoningProfileProjection,
) -> Result<vega_store::reasoning::ReasoningProfilePatch, ReasoningSettingsErrorCode> {
    if base.provider != candidate.provider || base.model != candidate.model {
        return Err(ReasoningSettingsErrorCode::Invalid);
    }
    let mut preference_only = base.clone();
    preference_only.preference = candidate.preference.clone();
    if &preference_only != candidate
        || !candidate.supports_preference()
        || candidate.freeze().is_err()
    {
        return Err(ReasoningSettingsErrorCode::Invalid);
    }
    let stored = authority
        .profile(&base.provider, &base.model)
        .map(ReasoningProfileProjection::from_store)
        .transpose()
        .map_err(|_| ReasoningSettingsErrorCode::Invalid)?;
    let preference = match &candidate.preference {
        ReasoningChoice::Enabled => "enabled".to_string(),
        ReasoningChoice::ProviderDefault => "provider_default".to_string(),
        ReasoningChoice::Disabled => "disabled".to_string(),
        ReasoningChoice::Effort(effort) => effort.clone(),
    };
    Ok(vega_store::reasoning::ReasoningProfilePatch {
        provider: candidate.provider.clone(),
        model: candidate.model.clone(),
        preference: stored
            .as_ref()
            .is_none_or(|stored| stored.preference != candidate.preference)
            .then_some(preference),
    })
}

struct ReasoningSaveWorkerResult {
    capabilities: Option<vega_store::model_capabilities::ModelCapabilityConfig>,
    authority: Option<(
        vega_store::reasoning::ReasoningConfig,
        vega_store::reasoning::ReasoningFileSnapshot,
    )>,
    error: Option<ReasoningSettingsErrorCode>,
}

type ReasoningSaveOrigin = Entity<SettingsView>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerThinkingKind {
    Choice,
    RetryDefault,
}

pub(super) struct PendingComposerThinking {
    stream: Entity<ConversationStream>,
    request: ComposerDefaultsRequested,
    kind: ComposerThinkingKind,
}

#[derive(Debug, PartialEq, Eq)]
enum DefaultRetryError {
    Unavailable,
    Superseded(Option<Vec<vega_conversation::types::SessionReasoningPreference>>),
    RouteChanged,
}

type DefaultRetrySaveResult = Result<
    (
        ReasoningSaveWorkerResult,
        Option<Vec<vega_conversation::types::SessionReasoningPreference>>,
    ),
    DefaultRetryError,
>;

type SessionReasoningSaveResult = Result<
    (
        Vec<vega_conversation::types::SessionReasoningPreference>,
        Option<ReasoningSaveWorkerResult>,
    ),
    (),
>;

struct ReasoningDefaultsSaveJob {
    path: std::path::PathBuf,
    base: vega_store::reasoning::ReasoningConfig,
    snapshot: vega_store::reasoning::ReasoningFileSnapshot,
    patch: vega_store::reasoning::ReasoningProfilePatch,
}

struct SessionReasoningSaveJob {
    database: Option<std::path::PathBuf>,
    thread_id: String,
    draft: bool,
    preferences: Vec<vega_conversation::types::SessionReasoningPreference>,
    candidate: ReasoningProfileProjection,
    defaults: Option<ReasoningDefaultsSaveJob>,
}

fn prepare_composer_defaults_save(
    path: Option<std::path::PathBuf>,
    authority: Option<(
        vega_store::reasoning::ReasoningConfig,
        vega_store::reasoning::ReasoningFileSnapshot,
    )>,
    error: Option<ReasoningSettingsErrorCode>,
    profile: &ReasoningProfileProjection,
    candidate: &ReasoningProfileProjection,
) -> Result<ReasoningDefaultsSaveJob, ReasoningSettingsErrorCode> {
    if let Some(error) = error {
        return Err(error);
    }
    let (path, (base, snapshot)) = path.zip(authority).ok_or(ReasoningSettingsErrorCode::Io)?;
    let mut patch = reasoning_profile_patch(&base, profile, candidate)?;
    patch.preference = Some(reasoning_choice_name(&candidate.preference).to_string());
    Ok(ReasoningDefaultsSaveJob {
        path,
        base,
        snapshot,
        patch,
    })
}

fn save_session_reasoning(job: SessionReasoningSaveJob) -> SessionReasoningSaveResult {
    let preferences = if job.draft {
        let mut preferences = job.preferences;
        vega_conversation::session_reasoning::set_preference(&mut preferences, &job.candidate)
            .map_err(|_| ())?;
        preferences
    } else {
        let store = vega_store::Store::open_existing(job.database.ok_or(())?).map_err(|_| ())?;
        vega_conversation::session_reasoning::save(&store, &job.thread_id, &job.candidate)
            .map_err(|_| ())?
    };
    let defaults = job.defaults.map(|defaults| {
        save_reasoning_defaults(
            &defaults.path,
            &defaults.snapshot,
            &defaults.base,
            &defaults.patch,
        )
    });
    Ok((preferences, defaults))
}

fn save_reasoning_defaults(
    path: &std::path::Path,
    expected: &vega_store::reasoning::ReasoningFileSnapshot,
    base: &vega_store::reasoning::ReasoningConfig,
    patch: &vega_store::reasoning::ReasoningProfilePatch,
) -> ReasoningSaveWorkerResult {
    let error = vega_store::reasoning::save_profile_patch(path, expected, base, patch)
        .err()
        .map(|error| reasoning_error_code(&error));
    let authority = read_reasoning_authority(path);
    let (authority, error) = match authority {
        Ok(authority) => (Some(authority), error),
        Err(read_error) => (None, error.or(Some(reasoning_error_code(&read_error)))),
    };
    let capabilities = vega_store::model_capabilities::read_from(
        &path.with_file_name(vega_store::model_capabilities::MODEL_CAPABILITY_FILE_NAME),
    )
    .ok();
    let error = if capabilities.is_some() {
        error
    } else {
        error.or(Some(ReasoningSettingsErrorCode::Invalid))
    };
    ReasoningSaveWorkerResult {
        capabilities,
        authority,
        error,
    }
}

fn reasoning_choice_name(choice: &ReasoningChoice) -> &str {
    match choice {
        ReasoningChoice::Enabled => "enabled",
        ReasoningChoice::ProviderDefault => "provider_default",
        ReasoningChoice::Disabled => "disabled",
        ReasoningChoice::Effort(effort) => effort,
    }
}

fn composer_reasoning_candidate(
    request: &ComposerDefaultsRequested,
    profile: &ReasoningProfileProjection,
    config: &vega_store::config::AppConfig,
) -> Option<ReasoningProfileProjection> {
    let source = request.defaults.reasoning.as_ref()?;
    let provider = unique_provider_for_model(config, &request.defaults.model)?;
    if source.provider != provider.name
        || source.model != request.defaults.model
        || profile.provider != provider.name
        || profile.model != request.defaults.model
    {
        return None;
    }
    let choice = match request.defaults.thinking.as_str() {
        "provider_default" => ReasoningChoice::ProviderDefault,
        "enabled" if profile.supports_enabled => ReasoningChoice::Enabled,
        OFF_CHOICE_NAME if profile.supports_disabled && profile.disabled_wire.is_some() => {
            ReasoningChoice::Disabled
        }
        effort if profile.efforts.iter().any(|candidate| candidate == effort) => {
            ReasoningChoice::Effort(effort.to_string())
        }
        _ => return None,
    };
    let mut candidate = profile.clone();
    candidate.preference = choice;
    if !candidate.supports_preference() || candidate.freeze().is_err() {
        return None;
    }
    Some(candidate)
}

fn retry_session_preference_matches(
    preferences: Option<&[vega_conversation::types::SessionReasoningPreference]>,
    request: &ComposerDefaultsRequested,
) -> Result<(), DefaultRetryError> {
    let source = request
        .defaults
        .reasoning
        .as_ref()
        .ok_or(DefaultRetryError::Unavailable)?;
    let preferences = preferences.ok_or(DefaultRetryError::Unavailable)?;
    match preferences.iter().find(|preference| {
        preference.provider == source.provider && preference.model == request.defaults.model
    }) {
        Some(preference) if preference.preference == request.defaults.thinking => Ok(()),
        _ => Err(DefaultRetryError::Superseded(None)),
    }
}

fn save_reasoning_default_retry(
    job: ReasoningDefaultsSaveJob,
    request: &ComposerDefaultsRequested,
    database: Option<std::path::PathBuf>,
    draft_preferences: Option<Vec<vega_conversation::types::SessionReasoningPreference>>,
) -> DefaultRetrySaveResult {
    if let Some(preferences) = draft_preferences {
        retry_session_preference_matches(Some(&preferences), request)?;
        return Ok((
            save_reasoning_defaults(&job.path, &job.snapshot, &job.base, &job.patch),
            None,
        ));
    }
    let store = vega_store::Store::open_existing(database.ok_or(DefaultRetryError::Unavailable)?)
        .map_err(|_| DefaultRetryError::Unavailable)?;
    let transaction = store
        .immediate_transaction()
        .map_err(|_| DefaultRetryError::Unavailable)?;
    let preferences = vega_conversation::threads::get_thread(&store, &request.thread_id)
        .map_err(|_| DefaultRetryError::Unavailable)?
        .reasoning_preferences;
    match retry_session_preference_matches(preferences.as_deref(), request) {
        Err(DefaultRetryError::Superseded(_)) => {
            return Err(DefaultRetryError::Superseded(preferences));
        }
        result => result?,
    }
    let result = save_reasoning_defaults(&job.path, &job.snapshot, &job.base, &job.patch);
    transaction
        .commit()
        .map_err(|_| DefaultRetryError::Unavailable)?;
    Ok((result, preferences))
}

fn reasoning_result_confirms_request(
    result: &ReasoningSaveWorkerResult,
    request: &ComposerDefaultsRequested,
) -> bool {
    let Some(source) = request.defaults.reasoning.as_ref() else {
        return false;
    };
    result.authority.as_ref().is_some_and(|(authority, _)| {
        authority
            .profile(&source.provider, &request.defaults.model)
            .is_some_and(|profile| profile.preference == request.defaults.thinking)
    })
}

fn apply_draft_reasoning_result(
    draft: &mut Thread,
    thread_id: &str,
    candidate: &ReasoningProfileProjection,
) -> bool {
    if draft.id != thread_id {
        return false;
    }
    let Some(preferences) = draft.reasoning_preferences.as_mut() else {
        return false;
    };
    vega_conversation::session_reasoning::set_preference(preferences, candidate).is_ok()
}

fn confirmed_session_defaults(
    catalog: Option<&[ReasoningProfileProjection]>,
    authority: Option<&vega_store::reasoning::ReasoningConfig>,
    error: Option<ReasoningSettingsErrorCode>,
) -> Option<Vec<vega_conversation::types::SessionReasoningPreference>> {
    if error.is_some() {
        return None;
    }
    let authority = authority?;
    Some(catalog.map_or_else(|| authority.profiles.clone(), session_defaults))
}

struct ReasoningSaveJob {
    generation: u64,
    operation_id: u64,
    path: std::path::PathBuf,
    expected_snapshot: vega_store::reasoning::ReasoningFileSnapshot,
    base_config: vega_store::reasoning::ReasoningConfig,
    patch: vega_store::reasoning::ReasoningProfilePatch,
}

pub(super) fn read_reasoning_authority(
    path: &std::path::Path,
) -> Result<
    (
        vega_store::reasoning::ReasoningConfig,
        vega_store::reasoning::ReasoningFileSnapshot,
    ),
    vega_store::reasoning::ReasoningConfigError,
> {
    vega_store::reasoning::read_authority_from(path)
}

fn reconcile_reasoning_profiles(
    config: &vega_store::reasoning::ReasoningConfig,
    catalog: Vec<ReasoningProfileProjection>,
    provider_config: Option<&vega_store::config::AppConfig>,
    capabilities: &vega_store::model_capabilities::ModelCapabilityConfig,
) -> Result<Vec<ReasoningProfileProjection>, ()> {
    catalog
        .into_iter()
        .map(|catalog_profile| {
            let provider = catalog_profile.provider;
            let model = catalog_profile.model;
            let configured = config
                .profile(&provider, &model)
                .map(ReasoningProfileProjection::from_store)
                .transpose()
                .map_err(|_| ())?;
            let actual_provider = provider_config
                .and_then(|config| {
                    config.providers.iter().find(|candidate| {
                        candidate.enabled
                            && candidate.name == provider
                            && candidate.models.contains(&model)
                    })
                })
                .ok_or(())?;
            crate::reasoning_capabilities::resolve_reasoning_profile(
                &provider,
                actual_provider.api,
                &actual_provider.base_url,
                &model,
                configured,
                capabilities,
            )
            .map_err(|_| ())
        })
        .collect()
}

fn reasoning_projection_unavailable(window: &VegaWindow) -> bool {
    let reasoning_error = window
        .configured_reasoning_error
        .or(window.reasoning_error_hold);
    let authority_unavailable = window.configured_reasoning_authority.is_none()
        && (window.configured_models.is_some() || reasoning_error.is_some());
    authority_unavailable || reasoning_error == Some(ReasoningSettingsErrorCode::Invalid)
}

/// Applies the same reasoning authority decision from a free app callback
/// that cannot borrow a `Context<VegaWindow>` (model selection ack path).
pub(super) fn apply_reasoning_profile_to_stream_with_app(
    window: &VegaWindow,
    stream: &Entity<ConversationStream>,
    model: &str,
    cx: &mut App,
) {
    let preferences = stream.read(cx).thread_snapshot().reasoning_preferences;
    let available = !reasoning_projection_unavailable(window) && preferences.is_some();
    let project = |model: &str| {
        let profile = window.reasoning_profile_for_model(model)?;
        Some(vega_conversation::session_reasoning::project(
            profile,
            preferences.as_ref()?,
        ))
    };
    if !available {
        stream.update(cx, ConversationStream::mark_reasoning_unavailable);
    } else if let Some(profile) = project(model) {
        match profile {
            Ok(profile) => {
                stream.update(cx, |stream, cx| stream.apply_reasoning_profile(profile, cx))
            }
            Err(_) => stream.update(cx, ConversationStream::mark_reasoning_unavailable),
        }
    } else {
        stream.update(cx, ConversationStream::clear_reasoning_profile);
    }
    let selected_model = stream.read(cx).selector_model().to_owned();
    let profile = available
        .then(|| project(&selected_model))
        .flatten()
        .and_then(Result::ok);
    stream.update(cx, |stream, cx| {
        stream.apply_selector_reasoning_profile(profile, cx)
    });
}

impl VegaWindow {
    pub(in crate::window) fn apply_retained_draft_reasoning_result(
        draft: &mut Thread,
        thread_id: &str,
        candidate: &ReasoningProfileProjection,
    ) -> bool {
        apply_draft_reasoning_result(draft, thread_id, candidate)
    }

    /// Resolves the independent reasoning authority beside the provider
    /// config. Tests use the same owned config override, so no environment or
    /// process-global path is consulted by the controller.
    pub(super) fn reasoning_config_path(&self) -> Option<std::path::PathBuf> {
        self.composer_config_path()
            .map(|path| path.with_file_name(vega_store::reasoning::REASONING_FILE_NAME))
    }

    /// Invalidates the in-memory model catalog after Settings closes. The
    /// generation fence lets an older config worker finish harmlessly while
    /// the next render starts a read of the just-saved provider list.
    pub(crate) fn invalidate_model_catalog(&mut self) {
        self.model_catalog_generation = self.model_catalog_generation.wrapping_add(1);
        self.configured_models = None;
        self.configured_reasoning = None;
        self.configured_reasoning_error = None;
        self.configured_reasoning_authority = None;
        self.model_catalog_loading = false;
    }

    pub(crate) fn reasoning_profile_for_model(
        &self,
        model: &str,
    ) -> Option<ReasoningProfileProjection> {
        self.configured_reasoning
            .as_ref()?
            .iter()
            .find(|profile| profile.model == model)
            .cloned()
    }

    pub(crate) fn reasoning_settings_projection(&self) -> ReasoningSettingsProjection {
        let error = self
            .configured_reasoning_error
            .or(self.reasoning_error_hold);
        match &self.configured_reasoning {
            Some(profiles) => ReasoningSettingsProjection::Ready {
                generation: self.model_catalog_generation,
                profiles: profiles.clone(),
                error,
            },
            None if error.is_some() => ReasoningSettingsProjection::Ready {
                generation: self.model_catalog_generation,
                profiles: Vec::new(),
                error,
            },
            None => ReasoningSettingsProjection::Loading,
        }
    }

    /// Reloads both provider/model and reasoning projections through the
    /// existing worker path. The independent file remains authoritative; no
    /// Settings UI code performs filesystem IO.
    pub(crate) fn request_reasoning_reload(
        &mut self,
        view: Entity<SettingsView>,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.reasoning_save_pending.is_some()
            || self.model_catalog_loading
        {
            return;
        }
        self.reasoning_error_hold = None;
        self.model_catalog_refresh_pending = false;
        self.invalidate_model_catalog();
        view.update(cx, |settings, cx| {
            settings.apply_reasoning_projection(ReasoningSettingsProjection::Loading, cx)
        });
        self.start_model_catalog_load(cx);
    }

    /// Starts one exact provider/model profile patch. The Settings entity and
    /// generation are both part of the lease, while the app-level pending
    /// owner survives a route close until the worker's ack or reconciliation.
    pub(crate) fn request_reasoning_profile_save(
        &mut self,
        view: Entity<SettingsView>,
        request: &ReasoningProfileSaveRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || !view
                .read(cx)
                .reasoning_save_is_current(request.generation, request.operation_id)
        {
            return;
        }
        if self.reasoning_save_pending.is_some()
            || self.model_catalog_loading
            || self.model_catalog_refresh_pending
            || request.generation != self.model_catalog_generation
        {
            self.reasoning_error_hold = Some(ReasoningSettingsErrorCode::Busy);
            let profiles = self
                .configured_reasoning
                .clone()
                .filter(|profiles| !profiles.is_empty())
                .unwrap_or_else(|| vec![request.base.clone()]);
            view.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(
                    ReasoningSettingsProjection::Ready {
                        generation: self.model_catalog_generation,
                        profiles,
                        error: Some(ReasoningSettingsErrorCode::Busy),
                    },
                    cx,
                )
            });
            return;
        }
        let current_profiles = self
            .configured_reasoning
            .clone()
            .filter(|profiles| !profiles.is_empty())
            .unwrap_or_else(|| vec![request.base.clone()]);
        if let Some(error) = self.configured_reasoning_error {
            self.reasoning_error_hold = Some(error);
            view.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(
                    ReasoningSettingsProjection::Ready {
                        generation: request.generation,
                        profiles: current_profiles,
                        error: Some(error),
                    },
                    cx,
                )
            });
            return;
        }
        let Some((base_config, expected_snapshot)) = self.configured_reasoning_authority.clone()
        else {
            self.reasoning_error_hold = Some(ReasoningSettingsErrorCode::Io);
            self.configured_reasoning_error = Some(ReasoningSettingsErrorCode::Io);
            view.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(
                    ReasoningSettingsProjection::Ready {
                        generation: request.generation,
                        profiles: current_profiles,
                        error: Some(ReasoningSettingsErrorCode::Io),
                    },
                    cx,
                )
            });
            return;
        };
        let patch = match reasoning_profile_patch(&base_config, &request.base, &request.profile) {
            Ok(patch) => patch,
            Err(error) => {
                self.reasoning_error_hold = Some(error);
                self.configured_reasoning_error = Some(error);
                view.update(cx, |settings, cx| {
                    settings.apply_reasoning_projection(
                        ReasoningSettingsProjection::Ready {
                            generation: request.generation,
                            profiles: current_profiles,
                            error: Some(error),
                        },
                        cx,
                    )
                });
                return;
            }
        };
        let Some(path) = self.reasoning_config_path() else {
            self.reasoning_error_hold = Some(ReasoningSettingsErrorCode::Io);
            self.configured_reasoning_error = Some(ReasoningSettingsErrorCode::Io);
            view.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(
                    ReasoningSettingsProjection::Ready {
                        generation: request.generation,
                        profiles: current_profiles,
                        error: Some(ReasoningSettingsErrorCode::Io),
                    },
                    cx,
                )
            });
            return;
        };
        self.reasoning_error_hold = None;
        self.start_reasoning_save(
            view,
            ReasoningSaveJob {
                generation: request.generation,
                operation_id: request.operation_id,
                path,
                expected_snapshot,
                base_config,
                patch,
            },
            cx,
        );
    }

    pub(crate) fn persist_composer_thinking(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerDefaultsRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx)
            || request.defaults.model != stream.read(cx).selector_model()
        {
            stream.update(cx, |stream, cx| {
                stream.acknowledge_reasoning_intent(request, cx)
            });
            return;
        }
        self.begin_reasoning_default_save(request, cx);
        self.enqueue_composer_thinking(stream, request, ComposerThinkingKind::Choice, cx);
    }

    pub(crate) fn retry_composer_thinking(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerDefaultsRequested,
        cx: &mut Context<Self>,
    ) {
        if self.reasoning_save_pending.is_none() && !self.model_catalog_loading {
            self.invalidate_model_catalog();
            self.start_model_catalog_load(cx);
            self.enqueue_composer_thinking(stream, request, ComposerThinkingKind::RetryDefault, cx);
            return;
        }
        self.model_catalog_refresh_pending = true;
        self.enqueue_composer_thinking(stream, request, ComposerThinkingKind::RetryDefault, cx);
    }

    fn enqueue_composer_thinking(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerDefaultsRequested,
        kind: ComposerThinkingKind,
        cx: &mut Context<Self>,
    ) {
        let mut index = 0;
        while index < self.pending_composer_thinking.len() {
            let queued = &self.pending_composer_thinking[index];
            if queued.request.thread_id == request.thread_id
                && queued.request.defaults.model == request.defaults.model
            {
                let queued = self.pending_composer_thinking.remove(index);
                queued.stream.update(cx, |stream, cx| {
                    stream.acknowledge_reasoning_intent(&queued.request, cx)
                });
                if let Some((id, current)) = self.stream_view.clone()
                    && id == queued.request.thread_id
                    && current != queued.stream
                {
                    current.update(cx, |stream, cx| {
                        stream.acknowledge_reasoning_intent(&queued.request, cx)
                    });
                }
                self.finish_reasoning_default_save(&queued.request, None, cx);
            } else {
                index += 1;
            }
        }
        self.pending_composer_thinking
            .push(PendingComposerThinking {
                stream,
                request: request.clone(),
                kind,
            });
        self.start_pending_composer_thinking(cx);
    }

    fn reject_composer_thinking(
        &mut self,
        stream: &Entity<ConversationStream>,
        request: &ComposerDefaultsRequested,
        report_error: bool,
        cx: &mut Context<Self>,
    ) {
        self.finish_reasoning_default_save(request, None, cx);
        if let Some((id, current)) = self.stream_view.clone()
            && id == request.thread_id
            && current != *stream
        {
            current.update(cx, |stream, cx| {
                stream.acknowledge_reasoning_intent(request, cx)
            });
            let model = current.read(cx).displayed_model().to_owned();
            self.apply_reasoning_profile_to_stream(&current, &model, cx);
        }
        if stream.read(cx).thread_snapshot().id == request.thread_id {
            stream.update(cx, |stream, cx| {
                stream.acknowledge_reasoning_intent(request, cx);
                if report_error {
                    stream.apply_controller_error(cx);
                }
            });
            let displayed_model = stream.read(cx).displayed_model().to_owned();
            self.apply_reasoning_profile_to_stream(stream, &displayed_model, cx);
        }
        if report_error
            && let Some((id, current)) = self.stream_view.clone()
            && id == request.thread_id
            && current != *stream
        {
            current.update(cx, ConversationStream::apply_controller_error);
        }
    }

    fn save_composer_thinking(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerDefaultsRequested,
        kind: ComposerThinkingKind,
        cx: &mut Context<Self>,
    ) {
        let key = (request.thread_id.clone(), request.defaults.model.clone());
        if self.reasoning_default_latest.get(&key) != Some(request) {
            self.reject_composer_thinking(&stream, request, false, cx);
            return;
        }
        if stream.read(cx).thread_snapshot().id != request.thread_id
            || (kind == ComposerThinkingKind::RetryDefault
                && (!self.owns_stream_request(&stream, &request.thread_id, cx)
                    || cx.global::<SettingsOpen>().0))
        {
            self.reject_composer_thinking(&stream, request, true, cx);
            return;
        }
        if self.is_draft_route(&request.thread_id)
            && (self.trusted_actions.is_busy()
                || self
                    .agent_controller
                    .ordinary_preparing
                    .contains_key(&request.thread_id)
                || self
                    .automation_controller
                    .draft_creation
                    .as_ref()
                    .is_some_and(|(id, _)| id == &request.thread_id))
        {
            self.reject_composer_thinking(&stream, request, true, cx);
            return;
        }
        let Some(profile) = self.reasoning_profile_for_model(&request.defaults.model) else {
            self.reject_composer_thinking(
                &stream,
                request,
                kind == ComposerThinkingKind::Choice,
                cx,
            );
            return;
        };
        let Some(candidate) = self
            .loaded_replay_config
            .as_ref()
            .and_then(|config| composer_reasoning_candidate(request, &profile, config))
        else {
            self.reject_composer_thinking(
                &stream,
                request,
                kind == ComposerThinkingKind::Choice,
                cx,
            );
            return;
        };
        if kind == ComposerThinkingKind::RetryDefault {
            self.start_reasoning_default_retry(stream, request, &profile, &candidate, cx);
            return;
        }
        let defaults_save = match prepare_composer_defaults_save(
            self.reasoning_config_path(),
            self.configured_reasoning_authority.clone(),
            self.configured_reasoning_error
                .or(self.reasoning_error_hold),
            &profile,
            &candidate,
        ) {
            Ok(job) => job,
            Err(_) => {
                self.reject_composer_thinking(&stream, request, true, cx);
                return;
            }
        };
        let draft = self.is_draft_route(&request.thread_id);
        let preferences = if draft {
            self.draft_for_route(&request.thread_id)
                .and_then(|draft| draft.reasoning_preferences)
        } else {
            stream.read(cx).thread_snapshot().reasoning_preferences
        };
        let Some(preferences) = preferences else {
            self.reject_composer_thinking(&stream, request, true, cx);
            return;
        };
        self.composer_reasoning_operation = self.composer_reasoning_operation.wrapping_add(1);
        let owner = (
            self.model_catalog_generation,
            self.composer_reasoning_operation,
        );
        self.reasoning_save_pending = Some(owner);
        let database = self.file_backed_store_path(cx);
        let thread_id = request.thread_id.clone();
        let draft_candidate = draft.then(|| candidate.clone());
        let job = SessionReasoningSaveJob {
            database,
            thread_id: thread_id.clone(),
            draft,
            preferences,
            candidate,
            defaults: Some(defaults_save),
        };
        let request = request.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-session-reasoning-save".into())
            .spawn(move || {
                let _ = sender.send(save_session_reasoning(job));
            });
        if worker.is_err() {
            self.finish_session_reasoning_save(
                stream,
                request,
                owner,
                draft_candidate,
                Err(()),
                cx,
            );
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(super::session::MODEL_SELECTION_POLL)
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => Err(()),
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_session_reasoning_save(
                        stream,
                        request,
                        owner,
                        draft_candidate,
                        result,
                        cx,
                    );
                });
                break;
            }
        })
        .detach();
    }

    fn start_reasoning_default_retry(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerDefaultsRequested,
        profile: &ReasoningProfileProjection,
        candidate: &ReasoningProfileProjection,
        cx: &mut Context<Self>,
    ) {
        let key = (request.thread_id.clone(), request.defaults.model.clone());
        if !self
            .reasoning_default_failures
            .get(&key)
            .is_some_and(|failure| failure.request == *request && failure.retry_pending)
        {
            self.finish_reasoning_default_save(request, None, cx);
            if self.reasoning_default_failures.contains_key(&key) {
                stream.update(cx, ConversationStream::apply_controller_error);
            }
            return;
        }
        if cx.global::<SettingsOpen>().0
            || !self.owns_stream_request(&stream, &request.thread_id, cx)
        {
            self.reject_composer_thinking(&stream, request, true, cx);
            return;
        }
        let draft = self.is_draft_route(&request.thread_id);
        let preferences = if draft {
            self.draft_for_route(&request.thread_id)
                .and_then(|draft| draft.reasoning_preferences)
        } else {
            stream.read(cx).thread_snapshot().reasoning_preferences
        };
        if draft {
            match retry_session_preference_matches(preferences.as_deref(), request) {
                Ok(()) => {}
                Err(DefaultRetryError::Superseded(_)) => {
                    self.discard_reasoning_default_failure(request, cx);
                    self.finish_reasoning_default_save(request, None, cx);
                    return;
                }
                Err(DefaultRetryError::Unavailable | DefaultRetryError::RouteChanged) => {
                    self.reject_composer_thinking(&stream, request, false, cx);
                    return;
                }
            }
        }
        let job = match prepare_composer_defaults_save(
            self.reasoning_config_path(),
            self.configured_reasoning_authority.clone(),
            self.configured_reasoning_error
                .or(self.reasoning_error_hold),
            profile,
            candidate,
        ) {
            Ok(job) => job,
            Err(_) => {
                self.reject_composer_thinking(&stream, request, false, cx);
                return;
            }
        };
        let draft_preferences = if draft { preferences } else { None };
        let database = self.file_backed_store_path(cx);
        self.composer_reasoning_operation = self.composer_reasoning_operation.wrapping_add(1);
        let owner = (
            self.model_catalog_generation,
            self.composer_reasoning_operation,
        );
        self.reasoning_save_pending = Some(owner);
        let request = request.clone();
        let worker_request = request.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-reasoning-default-retry".into())
            .spawn(move || {
                let _ = sender.send(save_reasoning_default_retry(
                    job,
                    &worker_request,
                    database,
                    draft_preferences,
                ));
            });
        if worker.is_err() {
            self.finish_reasoning_default_retry(
                request,
                owner,
                Err(DefaultRetryError::Unavailable),
                cx,
            );
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(super::session::MODEL_SELECTION_POLL)
                    .await;
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => Err(DefaultRetryError::Unavailable),
                };
                let _ = this.update(cx, |this, cx| {
                    let result = if draft && !this.is_draft_route(&request.thread_id) {
                        Err(DefaultRetryError::RouteChanged)
                    } else {
                        result
                    };
                    this.finish_reasoning_default_retry(request, owner, result, cx);
                });
                break;
            }
        })
        .detach();
    }

    fn apply_reloaded_reasoning_preferences(
        &mut self,
        thread_id: &str,
        preferences: Vec<vega_conversation::types::SessionReasoningPreference>,
        cx: &mut Context<Self>,
    ) {
        if let Some(current) = cx.global::<OpenedThread>().0.as_ref()
            && current.id == thread_id
        {
            let mut current = current.clone();
            current.reasoning_preferences = Some(preferences.clone());
            cx.set_global(OpenedThread(Some(current)));
        }
        if let Some((id, stream)) = self.stream_view.clone()
            && id == thread_id
        {
            stream.update(cx, |stream, cx| {
                stream.apply_session_reasoning_preferences(preferences, cx)
            });
            let model = stream.read(cx).displayed_model().to_owned();
            self.apply_reasoning_profile_to_stream(&stream, &model, cx);
        }
    }

    fn mark_session_reasoning_unavailable(&mut self, thread_id: &str, cx: &mut Context<Self>) {
        if let Some(current) = cx.global::<OpenedThread>().0.as_ref()
            && current.id == thread_id
        {
            let mut current = current.clone();
            current.reasoning_preferences = None;
            cx.set_global(OpenedThread(Some(current)));
        }
        if let Some((id, stream)) = self.stream_view.clone()
            && id == thread_id
        {
            stream.update(cx, ConversationStream::mark_session_reasoning_unavailable);
        }
    }

    fn finish_reasoning_default_retry(
        &mut self,
        request: ComposerDefaultsRequested,
        owner: (u64, u64),
        result: DefaultRetrySaveResult,
        cx: &mut Context<Self>,
    ) {
        if self.reasoning_save_pending != Some(owner) {
            self.finish_reasoning_default_save(&request, None, cx);
            return;
        }
        self.reasoning_save_pending = None;
        let defaults_error = match result {
            Ok((result, preferences)) => {
                if let Some(preferences) = preferences {
                    self.apply_reloaded_reasoning_preferences(&request.thread_id, preferences, cx);
                }
                let confirmed = reasoning_result_confirms_request(&result, &request);
                let error = self.apply_reasoning_authority_result(result, cx);
                let failed = error.is_some() || !confirmed;
                self.finish_reasoning_default_save(&request, Some(failed), cx);
                if confirmed && error.is_none() {
                    self.clear_confirmed_reasoning_default_failures(cx);
                }
                failed
            }
            Err(DefaultRetryError::Superseded(preferences)) => {
                self.discard_reasoning_default_failure(&request, cx);
                self.finish_reasoning_default_save(&request, None, cx);
                if let Some(preferences) = preferences {
                    self.apply_reloaded_reasoning_preferences(&request.thread_id, preferences, cx);
                } else {
                    self.mark_session_reasoning_unavailable(&request.thread_id, cx);
                }
                false
            }
            Err(DefaultRetryError::Unavailable) => {
                self.finish_reasoning_default_save(&request, None, cx);
                self.mark_session_reasoning_unavailable(&request.thread_id, cx);
                true
            }
            Err(DefaultRetryError::RouteChanged) => {
                self.finish_reasoning_default_save(&request, None, cx);
                true
            }
        };
        if defaults_error || self.model_catalog_loading || self.model_catalog_refresh_pending {
            if defaults_error && self.reasoning_error_hold != Some(ReasoningSettingsErrorCode::Busy)
            {
                self.reasoning_error_hold = None;
            }
            self.model_catalog_refresh_pending = false;
            self.invalidate_model_catalog();
            self.start_model_catalog_load(cx);
        }
        if let Some(settings) = self.settings_view.clone() {
            let projection = self.reasoning_settings_projection();
            settings.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(projection, cx)
            });
        }
        self.start_pending_composer_thinking(cx);
        cx.notify();
    }

    fn finish_session_reasoning_save(
        &mut self,
        stream: Entity<ConversationStream>,
        request: ComposerDefaultsRequested,
        owner: (u64, u64),
        draft_candidate: Option<ReasoningProfileProjection>,
        result: SessionReasoningSaveResult,
        cx: &mut Context<Self>,
    ) {
        if self.reasoning_save_pending != Some(owner) {
            self.finish_reasoning_default_save(&request, None, cx);
            return;
        }
        self.reasoning_save_pending = None;
        let thread_id = request.thread_id.clone();
        let (mut preferences, defaults_error) = match result {
            Ok((preferences, Some(defaults))) => {
                let confirmed = reasoning_result_confirms_request(&defaults, &request);
                let error = self.apply_reasoning_authority_result(defaults, cx);
                let error = error.or((!confirmed).then_some(ReasoningSettingsErrorCode::Invalid));
                (Some(preferences), error)
            }
            Ok((preferences, None)) => (Some(preferences), Some(ReasoningSettingsErrorCode::Io)),
            Err(()) => (None, None),
        };
        if preferences.is_some()
            && let Some(candidate) = &draft_candidate
        {
            preferences = self.draft_for_route(&thread_id).and_then(|mut draft| {
                if !Self::apply_retained_draft_reasoning_result(&mut draft, &thread_id, candidate) {
                    return None;
                }
                let preferences = draft.reasoning_preferences.clone();
                self.mirror_draft(&draft);
                preferences
            });
        }
        self.finish_reasoning_default_save(
            &request,
            preferences.as_ref().map(|_| defaults_error.is_some()),
            cx,
        );
        if preferences.is_some() && defaults_error.is_none() {
            self.clear_confirmed_reasoning_default_failures(cx);
        }
        stream.update(cx, |stream, cx| {
            stream.acknowledge_reasoning_intent(&request, cx)
        });
        if cx
            .global::<OpenedThread>()
            .0
            .as_ref()
            .is_some_and(|thread| thread.id == thread_id)
        {
            self.conversation_loading.clear();
        }
        if let Some(preferences) = &preferences
            && let Some(current) = cx.global::<OpenedThread>().0.as_ref()
            && current.id == thread_id
        {
            let mut current = current.clone();
            current.reasoning_preferences = Some(preferences.clone());
            cx.set_global(OpenedThread(Some(current)));
        }
        if stream.read(cx).thread_snapshot().id == thread_id {
            if let Some(preferences) = &preferences {
                stream.update(cx, |stream, cx| {
                    stream.apply_session_reasoning_preferences(preferences.clone(), cx)
                });
            } else {
                stream.update(cx, ConversationStream::apply_controller_error);
            }
            let model = stream.read(cx).displayed_model().to_owned();
            self.apply_reasoning_profile_to_stream(&stream, &model, cx);
        }
        if let Some((id, current)) = self.stream_view.clone()
            && id == thread_id
            && current != stream
        {
            current.update(cx, |stream, cx| {
                stream.acknowledge_reasoning_intent(&request, cx)
            });
            if let Some(preferences) = preferences {
                current.update(cx, |stream, cx| {
                    stream.apply_session_reasoning_preferences(preferences, cx)
                });
            } else {
                current.update(cx, ConversationStream::apply_controller_error);
            }
            let model = current.read(cx).displayed_model().to_owned();
            self.apply_reasoning_profile_to_stream(&current, &model, cx);
        }
        if defaults_error.is_some()
            || self.model_catalog_loading
            || self.model_catalog_refresh_pending
        {
            if defaults_error.is_some()
                && self.reasoning_error_hold != Some(ReasoningSettingsErrorCode::Busy)
            {
                self.reasoning_error_hold = None;
            }
            self.model_catalog_refresh_pending = false;
            self.invalidate_model_catalog();
            self.start_model_catalog_load(cx);
        }
        if let Some(settings) = self.settings_view.clone() {
            let projection = self.reasoning_settings_projection();
            settings.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(projection, cx)
            });
        }
        self.start_pending_composer_thinking(cx);
        cx.notify();
    }

    /// Shared by both origins so they cannot drift on the durable contract:
    /// one pending owner, one compare-before-rename, one readback.
    fn start_reasoning_save(
        &mut self,
        origin: ReasoningSaveOrigin,
        job: ReasoningSaveJob,
        cx: &mut Context<Self>,
    ) {
        self.reasoning_save_pending = Some((job.generation, job.operation_id));
        let generation = job.generation;
        let operation_id = job.operation_id;
        let ReasoningSaveJob {
            path,
            expected_snapshot,
            base_config,
            patch,
            ..
        } = job;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-reasoning-save".into())
            .spawn(move || {
                let mut result = match vega_store::reasoning::save_profile_patch(
                    &path,
                    &expected_snapshot,
                    &base_config,
                    &patch,
                ) {
                    Ok(_) => match read_reasoning_authority(&path) {
                        Ok(authority) => ReasoningSaveWorkerResult {
                            capabilities: None,
                            authority: Some(authority),
                            error: None,
                        },
                        Err(error) => ReasoningSaveWorkerResult {
                            capabilities: None,
                            authority: None,
                            error: Some(reasoning_error_code(&error)),
                        },
                    },
                    Err(error) => ReasoningSaveWorkerResult {
                        capabilities: None,
                        // A failed save can still have renamed its candidate;
                        // reread the actual file and make that disk state the
                        // next authority. If reread fails, retain only the
                        // UI draft and surface the uncertain state.
                        authority: read_reasoning_authority(&path).ok(),
                        error: Some(reasoning_error_code(&error)),
                    },
                };
                result.capabilities = vega_store::model_capabilities::read_from(
                    &path
                        .with_file_name(vega_store::model_capabilities::MODEL_CAPABILITY_FILE_NAME),
                )
                .ok();
                if result.capabilities.is_none() {
                    result.error = result.error.or(Some(ReasoningSettingsErrorCode::Invalid));
                }
                let _ = sender.send(result);
            });
        if worker.is_err() {
            self.finish_reasoning_profile_save(
                origin,
                generation,
                operation_id,
                ReasoningSaveWorkerResult {
                    capabilities: None,
                    authority: None,
                    error: Some(ReasoningSettingsErrorCode::Io),
                },
                cx,
            );
            return;
        }

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(super::session::MODEL_SELECTION_POLL)
                    .await;
                match receiver.try_recv() {
                    Ok(result) => {
                        let _ = this.update(cx, |this, cx| {
                            this.finish_reasoning_profile_save(
                                origin,
                                generation,
                                operation_id,
                                result,
                                cx,
                            )
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update(cx, |this, cx| {
                            this.finish_reasoning_profile_save(
                                origin,
                                generation,
                                operation_id,
                                ReasoningSaveWorkerResult {
                                    capabilities: None,
                                    authority: None,
                                    error: Some(ReasoningSettingsErrorCode::Io),
                                },
                                cx,
                            )
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    pub(super) fn clear_confirmed_reasoning_default_failures(&mut self, cx: &mut Context<Self>) {
        if self.configured_reasoning_authority.is_none() {
            return;
        }
        let confirmed = self.configured_reasoning.as_ref().map(|profiles| {
            profiles
                .iter()
                .map(|profile| {
                    (
                        profile.provider.clone(),
                        profile.model.clone(),
                        reasoning_choice_name(&profile.preference).to_string(),
                    )
                })
                .collect::<Vec<_>>()
        });
        for (provider, model, choice) in confirmed.into_iter().flatten() {
            self.clear_reasoning_default_failures_matching(&provider, &model, &choice, cx);
        }
    }

    fn apply_reasoning_authority_result(
        &mut self,
        result: ReasoningSaveWorkerResult,
        cx: &mut Context<Self>,
    ) -> Option<ReasoningSettingsErrorCode> {
        let ReasoningSaveWorkerResult {
            authority,
            capabilities,
            mut error,
        } = result;
        let previous_snapshot = self
            .configured_reasoning_authority
            .as_ref()
            .map(|(_, snapshot)| snapshot.clone());
        let mut replay_identity_changed = false;
        if let Some((config, snapshot)) = authority {
            replay_identity_changed = previous_snapshot.as_ref() != Some(&snapshot);
            if let Some(catalog) = self.configured_reasoning.take() {
                let fallback_catalog = catalog.clone();
                let capabilities = capabilities.ok_or(());
                match capabilities.and_then(|capabilities| {
                    reconcile_reasoning_profiles(
                        &config,
                        catalog,
                        self.loaded_replay_config.as_ref(),
                        &capabilities,
                    )
                }) {
                    Ok(profiles) => self.configured_reasoning = Some(profiles),
                    Err(()) => {
                        self.configured_reasoning = Some(fallback_catalog);
                        error = Some(ReasoningSettingsErrorCode::Invalid);
                    }
                }
            }
            self.configured_reasoning_authority = Some((config, snapshot));
        } else {
            // Do not claim the old authority remains on disk after an
            // uncertain post-rename error. Keep the visible profiles only as
            // a draft context and require an explicit reload to reconcile.
            self.configured_reasoning_authority = None;
        }
        if replay_identity_changed
            && let Some(sessions) = cx.try_global::<crate::app_agent::AppReplaySessions>()
        {
            sessions.0.invalidate_all();
        }
        self.reasoning_error_hold =
            if self.reasoning_error_hold == Some(ReasoningSettingsErrorCode::Busy) {
                self.reasoning_error_hold
            } else {
                error
            };
        self.configured_reasoning_error = error;
        if error.is_none() {
            self.clear_confirmed_reasoning_default_failures(cx);
        }
        let mut defaults = cx.global::<NewTaskDefaults>().clone();
        defaults.reasoning_preferences = confirmed_session_defaults(
            self.configured_reasoning.as_deref(),
            self.configured_reasoning_authority
                .as_ref()
                .map(|(config, _)| config),
            error,
        );
        cx.set_global(defaults);
        error
    }

    fn finish_reasoning_profile_save(
        &mut self,
        origin: ReasoningSaveOrigin,
        generation: u64,
        operation_id: u64,
        result: ReasoningSaveWorkerResult,
        cx: &mut Context<Self>,
    ) {
        if self.reasoning_save_pending != Some((generation, operation_id)) {
            return;
        }
        self.reasoning_save_pending = None;
        self.apply_reasoning_authority_result(result, cx);
        // Invalidate any catalog worker that read before this save. Its
        // generation is stale even if it completes after the save ack. A
        // provider Settings save observed while this operation was pending is
        // coalesced into the same fresh read, rather than racing a second
        // worker against the reasoning acknowledgement.
        let refresh_catalog = self.model_catalog_loading || self.model_catalog_refresh_pending;
        self.model_catalog_refresh_pending = false;
        if refresh_catalog {
            // Invalidate once to fence any catalog worker that observed the
            // prior generation, then expose Loading under that new generation.
            // Settings keeps an exact failed draft across Loading; the held
            // typed error is restored when the fresh authority read returns.
            self.invalidate_model_catalog();
            self.start_model_catalog_load(cx);
        } else {
            // No catalog worker is active. Advance the authority generation
            // before publishing Ready so the next Settings edit carries the
            // acknowledged generation instead of a stale one.
            self.model_catalog_generation = self.model_catalog_generation.wrapping_add(1);
        }
        // The Settings editor owns a visible draft that must learn its save
        // finished. The composer owns no draft: its slider is re-projected
        // from the new authority below.
        let view = &origin;
        if cx.global::<SettingsOpen>().0
            && self.settings_view.as_ref() == Some(view)
            && view
                .read(cx)
                .reasoning_save_is_current(generation, operation_id)
        {
            let projection = if self.model_catalog_loading {
                ReasoningSettingsProjection::Loading
            } else {
                self.reasoning_settings_projection()
            };
            let view = view.clone();
            view.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(projection, cx)
            });
        }
        if let Some((_, stream)) = &self.stream_view {
            let model = stream.read(cx).displayed_model().to_owned();
            self.apply_reasoning_profile_to_stream(stream, &model, cx);
        }
        self.start_pending_composer_thinking(cx);
        cx.notify();
    }

    /// Starts the newest tier intent that arrived while the worker was busy.
    ///
    /// A drag across the track emits one intent per tier; only the last one
    /// matters, so they coalesce into a single slot instead of a queue that
    /// would replay every intermediate tier after the ack.
    pub(super) fn project_pending_reasoning_intents(
        &self,
        stream: &Entity<ConversationStream>,
        thread_id: &str,
        cx: &mut Context<Self>,
    ) {
        for pending in &self.pending_composer_thinking {
            if pending.kind == ComposerThinkingKind::Choice
                && pending.request.thread_id == thread_id
                && pending.stream != *stream
            {
                stream.update(cx, |stream, cx| {
                    stream.restore_reasoning_intent(&pending.request, cx)
                });
            }
        }
    }

    pub(super) fn start_pending_composer_thinking(&mut self, cx: &mut Context<Self>) {
        if self.reasoning_save_pending.is_some()
            || self.model_catalog_loading
            || self.pending_composer_thinking.is_empty()
        {
            return;
        }
        if self.model_catalog_refresh_pending {
            self.model_catalog_refresh_pending = false;
            self.invalidate_model_catalog();
        }
        if self.configured_models.is_none() {
            self.start_model_catalog_load(cx);
            return;
        }
        while self.reasoning_save_pending.is_none()
            && !self.model_catalog_loading
            && !self.model_catalog_refresh_pending
            && !self.pending_composer_thinking.is_empty()
        {
            let pending = self.pending_composer_thinking.remove(0);
            self.save_composer_thinking(pending.stream, &pending.request, pending.kind, cx);
        }
    }

    pub(crate) fn apply_reasoning_profile_to_stream(
        &self,
        stream: &Entity<ConversationStream>,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        apply_reasoning_profile_to_stream_with_app(self, stream, model, cx);
    }
}

pub(super) fn session_defaults(
    profiles: &[ReasoningProfileProjection],
) -> Vec<vega_conversation::types::SessionReasoningPreference> {
    profiles
        .iter()
        .map(
            |profile| vega_conversation::types::SessionReasoningPreference {
                provider: profile.provider.clone(),
                model: profile.model.clone(),
                preference: match &profile.preference {
                    ReasoningChoice::Enabled => "enabled".to_string(),
                    ReasoningChoice::Disabled => "disabled".to_string(),
                    ReasoningChoice::ProviderDefault => "provider_default".to_string(),
                    ReasoningChoice::Effort(effort) => effort.clone(),
                },
            },
        )
        .collect()
}
