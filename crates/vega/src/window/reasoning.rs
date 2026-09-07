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
        let Some((base_config, expected_snapshot)) = self.configured_reasoning_authority.clone()
        else {
            self.reasoning_error_hold = Some(ReasoningSettingsErrorCode::Io);
            self.configured_reasoning_error = Some(ReasoningSettingsErrorCode::Io);
            let profiles = self
                .configured_reasoning
                .clone()
                .filter(|profiles| !profiles.is_empty())
                .unwrap_or_else(|| vec![request.base.clone()]);
            view.update(cx, |settings, cx| {
                settings.apply_reasoning_projection(
                    ReasoningSettingsProjection::Ready {
                        generation: request.generation,
                        profiles,
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
        let current_profiles = self
            .configured_reasoning
            .clone()
            .filter(|profiles| !profiles.is_empty())
            .unwrap_or_else(|| vec![request.base.clone()]);
        let patch = match reasoning_profile_patch(&request.base, &request.profile) {
            Ok(patch) => patch,
            Err(error) => {
                self.reasoning_error_hold = Some(error);
                self.configured_reasoning_error = Some(error);
                view.update(cx, |settings, cx| {
                    settings.apply_reasoning_projection(
                        ReasoningSettingsProjection::Ready {
                            generation: request.generation,
                            profiles: current_profiles.clone(),
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
                        profiles: current_profiles.clone(),
                        error: Some(ReasoningSettingsErrorCode::Io),
                    },
                    cx,
                )
            });
            return;
        };

        self.reasoning_save_pending = Some((request.generation, request.operation_id));
        let generation = request.generation;
        let operation_id = request.operation_id;
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
                view,
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
                                view.clone(),
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
                                view.clone(),
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
        view: Entity<SettingsView>,
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
            if cx.global::<SettingsOpen>().0
                && self.settings_view.as_ref() == Some(&view)
                && view
                    .read(cx)
                    .reasoning_save_is_current(generation, operation_id)
            {
                let projection = if self.model_catalog_loading {
                    ReasoningSettingsProjection::Loading
                } else {
                    self.reasoning_settings_projection()
                };
                view.update(cx, |settings, cx| {
                    settings.apply_reasoning_projection(projection, cx)
                });
            }
        } else {
            // No catalog worker is active. Advance the authority generation
            // before publishing Ready so the next Settings edit carries the
            // acknowledged generation instead of a stale one.
            self.model_catalog_generation = self.model_catalog_generation.wrapping_add(1);
            let projection = self.reasoning_settings_projection();
            if cx.global::<SettingsOpen>().0
                && self.settings_view.as_ref() == Some(&view)
                && view
                    .read(cx)
                    .reasoning_save_is_current(generation, operation_id)
            {
                view.update(cx, |settings, cx| {
                    settings.apply_reasoning_projection(projection, cx)
                });
            }
        }
        if let Some((_, stream)) = &self.stream_view {
            let model = stream.read(cx).displayed_model().to_owned();
            self.apply_reasoning_profile_to_stream(stream, &model, cx);
        }
        cx.notify();
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

    #[gpui::test]
    async fn reconcile_external_profile_deletion_keeps_catalog_as_provider_default(
        _cx: &mut gpui::TestAppContext,
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

    #[gpui::test]
    async fn reconcile_invalid_declared_profile_cannot_become_provider_default(
        _cx: &mut gpui::TestAppContext,
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
