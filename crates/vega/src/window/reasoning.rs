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
    base: &ReasoningProfileProjection,
    candidate: &ReasoningProfileProjection,
) -> Result<vega_store::reasoning::ReasoningProfilePatch, ReasoningSettingsErrorCode> {
    if base.provider != candidate.provider || base.model != candidate.model {
        return Err(ReasoningSettingsErrorCode::Invalid);
    }
    let protocol = |protocol| match protocol {
        ReasoningProtocol::OpenAiChatCompletions => "openai_chat_completions",
        ReasoningProtocol::ZhipuChatCompletions => "zhipu_chat_completions",
        ReasoningProtocol::Unknown => "unknown",
    };
    let support = |support| match support {
        ReasoningSupport::Required => "required",
        ReasoningSupport::Optional => "optional",
        ReasoningSupport::Unsupported => "unsupported",
        ReasoningSupport::Unknown => "unknown",
    };
    let disabled_wire = candidate.disabled_wire.map(|wire| match wire {
        ReasoningDisabledWire::ThinkingTypeDisabled => "thinking_type_disabled".to_string(),
        ReasoningDisabledWire::ReasoningEffortNone => "reasoning_effort_none".to_string(),
    });
    let preference = match &candidate.preference {
        ReasoningChoice::ProviderDefault => "provider_default".to_string(),
        ReasoningChoice::Disabled => "disabled".to_string(),
        ReasoningChoice::Effort(effort) => effort.clone(),
    };
    Ok(vega_store::reasoning::ReasoningProfilePatch {
        provider: candidate.provider.clone(),
        model: candidate.model.clone(),
        protocol: (base.protocol != candidate.protocol)
            .then(|| protocol(candidate.protocol).into()),
        support: (base.support != candidate.support).then(|| support(candidate.support).into()),
        efforts: (base.efforts != candidate.efforts).then(|| candidate.efforts.clone()),
        supports_disabled: (base.supports_disabled != candidate.supports_disabled)
            .then_some(candidate.supports_disabled),
        disabled_wire: (base.disabled_wire != candidate.disabled_wire).then_some(disabled_wire),
        preserve_reasoning_content: (base.preserve_reasoning_content
            != candidate.preserve_reasoning_content)
            .then_some(candidate.preserve_reasoning_content),
        preference: (base.preference != candidate.preference).then_some(preference),
    })
}

/// Result of one independent reasoning.toml save. An error may still carry a
/// valid authority: for example, directory fsync can report an error after
/// rename has already made the candidate visible. The app reconciles against
/// this reread instead of assuming every error left the old bytes in place.
struct ReasoningSaveWorkerResult {
    authority: Option<(
        vega_store::reasoning::ReasoningConfig,
        vega_store::reasoning::ReasoningFileSnapshot,
    )>,
    error: Option<ReasoningSettingsErrorCode>,
}

/// Where one reasoning.toml save originated (R57 P3).
///
/// Both surfaces share one single-flight owner, one authority, and one
/// reconciliation path; only the completion projection differs. The Settings
/// editor owns a visible draft that must be told its save finished, while the
/// composer owns a stream that must be re-projected from the new authority.
enum ReasoningSaveOrigin {
    Settings(Entity<SettingsView>),
    /// The composer's tier slider (R57 P3).
    Composer,
}

/// One fully-validated reasoning.toml save ready to run on a worker.
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

fn project_reasoning_profiles(
    config: &vega_store::reasoning::ReasoningConfig,
) -> Result<Vec<ReasoningProfileProjection>, ()> {
    config
        .profiles
        .iter()
        .map(|profile| ReasoningProfileProjection::from_store(profile).map_err(|_| ()))
        .collect()
}

fn reconcile_reasoning_profiles(
    config: &vega_store::reasoning::ReasoningConfig,
    catalog: Vec<ReasoningProfileProjection>,
) -> Result<Vec<ReasoningProfileProjection>, ()> {
    if catalog.is_empty() {
        return project_reasoning_profiles(config);
    }
    catalog
        .into_iter()
        .map(|catalog_profile| {
            let provider = catalog_profile.provider;
            let model = catalog_profile.model;
            match config.profile(&provider, &model) {
                Some(profile) => ReasoningProfileProjection::from_store(profile).map_err(|_| ()),
                None => Ok(ReasoningProfileProjection::unknown(provider, model)),
            }
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
    if reasoning_projection_unavailable(window) {
        stream.update(cx, ConversationStream::mark_reasoning_unavailable);
    } else if let Some(profile) = window.reasoning_profile_for_model(model) {
        stream.update(cx, |stream, cx| stream.apply_reasoning_profile(profile, cx));
    } else {
        stream.update(cx, ConversationStream::clear_reasoning_profile);
    }
}

impl VegaWindow {
    /// Resolves the independent reasoning authority beside the provider
    /// config. Tests use the same owned config override, so no environment or
    /// process-global path is consulted by the controller.
    fn reasoning_config_path(&self) -> Option<std::path::PathBuf> {
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

    /// Returns the exact capability projection for one model in the current
    /// uniquely-resolved catalog. Missing profiles are represented as
    /// provider-default/unknown by the worker and therefore never become an
    /// invented disabled choice.
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
            || self.reasoning_save_pending.is_some()
            || request.generation != self.model_catalog_generation
        {
            return;
        }
        let current_profiles = self
            .configured_reasoning
            .clone()
            .filter(|profiles| !profiles.is_empty())
            .unwrap_or_else(|| vec![request.base.clone()]);
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
        if !view
            .read(cx)
            .reasoning_save_is_current(request.generation, request.operation_id)
        {
            return;
        }
        let patch = match reasoning_profile_patch(&request.base, &request.profile) {
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
        self.start_reasoning_save(
            ReasoningSaveOrigin::Settings(view),
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

    /// R57 P3: persists one tier chosen in the composer's thinking slider.
    ///
    /// This is the existing `ComposerDefaultsRequested` path, not a new
    /// persistence mechanism: the slider's `ThinkingTierSelected` becomes the
    /// same `ReasoningProfilePatch { preference }` a Settings edit produces,
    /// written through the same authority, single-flight owner, and
    /// reconciliation. Only the profile's `preference` field is patched, so
    /// picking a tier cannot change the declared `efforts` the slider's own
    /// dot count comes from.
    ///
    /// The intent is validated against the app's own authority, never against
    /// the stream's copy: a tier the profile does not declare is refused with
    /// the exact stream re-projected from that authority, so the slider cannot
    /// display a value the next run would not send.
    ///
    /// R58: the request's `thinking` is a persisted *choice name*. The Off
    /// position sends [`OFF_CHOICE_NAME`] (`"disabled"`), which becomes
    /// `ReasoningChoice::Disabled` — the same value the wire encoder already
    /// routes through `disabled_wire`. It is never written into `efforts` and
    /// never becomes `Effort("off")`.
    pub(crate) fn persist_composer_thinking(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &ComposerDefaultsRequested,
        cx: &mut Context<Self>,
    ) {
        if !self.owns_stream_request(&stream, &request.thread_id, cx) {
            return;
        }
        // Every intent that reaches this handler supersedes any coalesced one:
        // only the user's newest position matters. Dropping the slot here keeps
        // a superseded tier from being replayed after a later save.
        self.pending_composer_thinking = None;
        // Settings owns the visible reasoning editor, and its generation is
        // the one the reasoning worker validates against. While it is open the
        // composer's slider is not on screen, so an intent from it cannot be
        // applied without racing that editor.
        if cx.global::<SettingsOpen>().0 {
            return;
        }
        // One worker at a time. A drag across the track emits one intent per
        // tier, so the newest one is kept and replayed after the ack instead of
        // being dropped or replayed as a queue of stale positions.
        if self.reasoning_save_pending.is_some() {
            self.pending_composer_thinking =
                Some((request.thread_id.clone(), request.defaults.thinking.clone()));
            return;
        }
        let choice = request.defaults.thinking.clone();
        let model = stream.read(cx).displayed_model().to_owned();
        let Some(profile) = self.reasoning_profile_for_model(&model) else {
            // A missing profile is provider-default with no controls. The
            // stream still re-projects so a stale slider cannot survive.
            self.apply_reasoning_profile_to_stream(&stream, &model, cx);
            return;
        };
        let Some((base_config, expected_snapshot)) = self.configured_reasoning_authority.clone()
        else {
            self.apply_reasoning_profile_to_stream(&stream, &model, cx);
            return;
        };
        let Some(path) = self.reasoning_config_path() else {
            self.apply_reasoning_profile_to_stream(&stream, &model, cx);
            return;
        };
        // R58: the Off position persists `"disabled"`, which is the store's
        // own name for `ReasoningChoice::Disabled` — a *separate* state, never
        // an effort. It is refused unless the profile declares the disabled
        // operation, mirroring the store validator; otherwise the store would
        // reject the write and surface it as a generic I/O error.
        let next = if choice == OFF_CHOICE_NAME {
            if !(profile.supports_disabled && profile.disabled_wire.is_some()) {
                self.apply_reasoning_profile_to_stream(&stream, &model, cx);
                return;
            }
            ReasoningChoice::Disabled
        } else {
            // Refuse anything the profile does not declare. An unknown tier
            // would otherwise fail the store validator inside the worker and
            // surface as a generic I/O error, when it is really an invalid
            // intent.
            if !profile.efforts.iter().any(|effort| effort == &choice) {
                self.apply_reasoning_profile_to_stream(&stream, &model, cx);
                return;
            }
            ReasoningChoice::Effort(choice)
        };
        // An unchanged preference is already the durable authority: re-project
        // instead of writing the same bytes back.
        if profile.preference == next {
            self.apply_reasoning_profile_to_stream(&stream, &model, cx);
            return;
        }
        let mut candidate = profile.clone();
        candidate.preference = next;
        let patch = match reasoning_profile_patch(&profile, &candidate) {
            Ok(patch) => patch,
            Err(_) => {
                self.apply_reasoning_profile_to_stream(&stream, &model, cx);
                return;
            }
        };
        // The composer has no Settings generation. Its own monotonic counter
        // keeps the owner id unique against a Settings operation in flight, so
        // a late ack can never be mistaken for the other surface's save.
        self.composer_reasoning_operation = self.composer_reasoning_operation.wrapping_add(1);
        let operation_id = self.composer_reasoning_operation;
        self.start_reasoning_save(
            ReasoningSaveOrigin::Composer,
            ReasoningSaveJob {
                generation: self.model_catalog_generation,
                operation_id,
                path,
                expected_snapshot,
                base_config,
                patch,
            },
            cx,
        );
    }

    /// Runs one validated reasoning.toml save on a worker thread.
    ///
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
        #[cfg(test)]
        let worker_gate = self.reasoning_save_worker_gate.take();
        let worker = std::thread::Builder::new()
            .name("vega-reasoning-save".into())
            .spawn(move || {
                #[cfg(test)]
                if let Some(gate) = worker_gate {
                    gate.wait();
                }
                let result = match vega_store::reasoning::save_profile_patch(
                    &path,
                    &expected_snapshot,
                    &base_config,
                    &patch,
                ) {
                    Ok(_) => match read_reasoning_authority(&path) {
                        Ok(authority) => ReasoningSaveWorkerResult {
                            authority: Some(authority),
                            error: None,
                        },
                        Err(error) => ReasoningSaveWorkerResult {
                            authority: None,
                            error: Some(reasoning_error_code(&error)),
                        },
                    },
                    Err(error) => ReasoningSaveWorkerResult {
                        // A failed save can still have renamed its candidate;
                        // reread the actual file and make that disk state the
                        // next authority. If reread fails, retain only the
                        // UI draft and surface the uncertain state.
                        authority: read_reasoning_authority(&path).ok(),
                        error: Some(reasoning_error_code(&error)),
                    },
                };
                let _ = sender.send(result);
            });
        if worker.is_err() {
            self.finish_reasoning_profile_save(
                origin,
                generation,
                operation_id,
                ReasoningSaveWorkerResult {
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

        let ReasoningSaveWorkerResult {
            authority,
            mut error,
        } = result;
        if let Some((config, snapshot)) = authority {
            let catalog = self.configured_reasoning.take().unwrap_or_default();
            let fallback_catalog = catalog.clone();
            match reconcile_reasoning_profiles(&config, catalog) {
                Ok(profiles) => self.configured_reasoning = Some(profiles),
                Err(()) => {
                    // A decoded authority should normally project losslessly
                    // because the store validator and this conversion share
                    // the same grammar. Keep the catalog rows visible if a
                    // future conversion drift is introduced, but make the
                    // authority unusable instead of treating them as a legal
                    // provider-default profile.
                    self.configured_reasoning = Some(fallback_catalog);
                    error = Some(ReasoningSettingsErrorCode::Invalid);
                }
            }
            self.configured_reasoning_authority = Some((config, snapshot));
        } else {
            // Do not claim the old authority remains on disk after an
            // uncertain post-rename error. Keep the visible profiles only as
            // a draft context and require an explicit reload to reconcile.
            self.configured_reasoning_authority = None;
        }
        self.reasoning_error_hold = error;
        self.configured_reasoning_error = error;
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
        if let ReasoningSaveOrigin::Settings(view) = &origin
            && cx.global::<SettingsOpen>().0
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
        if matches!(origin, ReasoningSaveOrigin::Composer) {
            self.start_pending_composer_thinking(cx);
        }
        cx.notify();
    }

    /// Starts the newest tier intent that arrived while the worker was busy.
    ///
    /// A drag across the track emits one intent per tier; only the last one
    /// matters, so they coalesce into a single slot instead of a queue that
    /// would replay every intermediate tier after the ack.
    fn start_pending_composer_thinking(&mut self, cx: &mut Context<Self>) {
        let Some((thread_id, tier)) = self.pending_composer_thinking.take() else {
            return;
        };
        let Some(stream) = self
            .stream_view
            .as_ref()
            .filter(|(id, _)| *id == thread_id)
            .map(|(_, stream)| stream.clone())
        else {
            return;
        };
        // The replayed intent still goes through the same validating entry
        // point, so a coalesced tier that the profile no longer declares is
        // refused exactly like a fresh one.
        let defaults = ComposerDefaults {
            model: stream.read(cx).displayed_model().to_owned(),
            thinking: tier,
            ..ComposerDefaults::default()
        };
        let request = ComposerDefaultsRequested {
            thread_id,
            defaults,
        };
        self.persist_composer_thinking(stream, &request, cx);
    }

    pub(crate) fn apply_reasoning_profile_to_stream(
        &self,
        stream: &Entity<ConversationStream>,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        if reasoning_projection_unavailable(self) {
            stream.update(cx, ConversationStream::mark_reasoning_unavailable);
        } else if let Some(profile) = self.reasoning_profile_for_model(model) {
            stream.update(cx, |stream, cx| stream.apply_reasoning_profile(profile, cx));
        } else {
            stream.update(cx, ConversationStream::clear_reasoning_profile);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::reconcile_reasoning_profiles;
    use vega_conversation::types::{ReasoningChoice, ReasoningProfileProjection, ReasoningSupport};

    fn glm_profile() -> vega_store::reasoning::ReasoningProfile {
        vega_store::reasoning::ReasoningProfile {
            provider: "zhipu".into(),
            model: "glm-5.3".into(),
            protocol: "zhipu_chat_completions".into(),
            support: "required".into(),
            efforts: vec!["low".into(), "high".into(), "max".into()],
            supports_disabled: false,
            disabled_wire: None,
            preserve_reasoning_content: true,
            preference: "max".into(),
        }
    }

    #[gpui_kit::test]
    async fn reconcile_external_profile_deletion_keeps_catalog_as_provider_default(
        _cx: &mut gpui_kit::TestAppContext,
    ) {
        let catalog = vec![ReasoningProfileProjection::from_store(&glm_profile()).unwrap()];
        let reconciled = reconcile_reasoning_profiles(
            &vega_store::reasoning::ReasoningConfig::default(),
            catalog,
        );
        let reconciled =
            reconciled.expect("missing profile is a valid provider-default projection");
        assert_eq!(reconciled.len(), 1);
        assert_eq!(reconciled[0].provider, "zhipu");
        assert_eq!(reconciled[0].model, "glm-5.3");
        assert_eq!(reconciled[0].support, ReasoningSupport::Unknown);
        assert_eq!(reconciled[0].preference, ReasoningChoice::ProviderDefault);
        assert!(!reconciled[0].preserve_reasoning_content);
    }

    #[gpui_kit::test]
    async fn reconcile_invalid_declared_profile_cannot_become_provider_default(
        _cx: &mut gpui_kit::TestAppContext,
    ) {
        let mut invalid = glm_profile();
        invalid.protocol = "openai_chat_completions".into();
        invalid.support = "optional".into();
        invalid.efforts = vec!["not-a-declared-effort".into()];
        invalid.preference = "provider_default".into();
        let config = vega_store::reasoning::ReasoningConfig {
            version: vega_store::reasoning::REASONING_CONFIG_VERSION,
            profiles: vec![invalid],
        };
        assert!(reconcile_reasoning_profiles(&config, Vec::new()).is_err());
    }
}
