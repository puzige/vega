use super::*;

impl VegaWindow {
    pub(crate) fn model_capability_settings_projection(&self) -> ModelCapabilitySettingsProjection {
        self.model_capability_projection.clone()
    }

    pub(crate) fn request_model_capability_reload(
        &mut self,
        view: Entity<SettingsView>,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.model_capability_pending.is_some()
        {
            return;
        }
        let Some(path) = self.composer_config_path() else {
            return;
        };
        self.model_capability_generation = self.model_capability_generation.wrapping_add(1);
        let generation = self.model_capability_generation;
        self.model_capability_projection = ModelCapabilitySettingsProjection::Loading;
        view.update(cx, |view, cx| {
            view.apply_model_capability_projection(ModelCapabilitySettingsProjection::Loading, cx)
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-model-capabilities-load".into())
            .spawn(move || {
                let _ = sender.send(ModelCapabilitySettingsService::new(path).load());
            });
        if worker.is_err() {
            self.finish_model_capability_load(
                view,
                generation,
                Err(ModelCapabilitySettingsErrorCode::Io),
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
                    Ok(value) => value,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err(ModelCapabilitySettingsErrorCode::Io)
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_model_capability_load(view, generation, result, cx)
                });
                break;
            }
        })
        .detach();
    }

    fn finish_model_capability_load(
        &mut self,
        view: Entity<SettingsView>,
        generation: u64,
        result: Result<ModelCapabilityAuthority, ModelCapabilitySettingsErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if self.model_capability_generation != generation || self.model_capability_pending.is_some()
        {
            return;
        }
        let error = match result {
            Ok(authority) => {
                self.model_capability_authority = Some(authority);
                None
            }
            Err(error) => {
                self.model_capability_authority = None;
                Some(error)
            }
        };
        self.model_capability_projection = ModelCapabilitySettingsProjection::Ready {
            generation,
            models: self
                .model_capability_authority
                .as_ref()
                .map(|authority| authority.models.clone())
                .unwrap_or_default(),
            error,
        };
        if cx.global::<SettingsOpen>().0 && self.settings_view.as_ref() == Some(&view) {
            let projection = self.model_capability_settings_projection();
            view.update(cx, |view, cx| {
                view.apply_model_capability_projection(projection, cx)
            });
        }
        self.invalidate_model_catalog();
        self.start_model_catalog_load(cx);
        cx.notify();
    }

    pub(crate) fn request_model_capability_save(
        &mut self,
        view: Entity<SettingsView>,
        request: &ModelCapabilitySaveRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0
            || self.settings_view.as_ref() != Some(&view)
            || self.model_capability_pending.is_some()
            || request.generation != self.model_capability_generation
            || !view
                .read(cx)
                .model_capability_save_is_current(request.generation, request.operation_id)
        {
            return;
        }
        let Some(base) = self
            .model_capability_authority
            .as_ref()
            .filter(|authority| authority.models.iter().any(|model| model == &request.base))
            .cloned()
        else {
            return;
        };
        let identity = match &request.patch {
            ModelCapabilityPatch::Upsert(value) => &value.identity,
            ModelCapabilityPatch::Reset { identity } => identity,
        };
        if identity != &request.base.resolved.identity {
            return;
        }
        let Some(path) = self.composer_config_path() else {
            return;
        };
        let patch = request.patch.clone();
        let generation = request.generation;
        let operation_id = request.operation_id;
        self.model_capability_pending = Some((generation, operation_id));
        self.model_capability_projection = ModelCapabilitySettingsProjection::Saving {
            generation,
            operation_id,
            model: request.base.clone(),
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vega-model-capabilities-save".into())
            .spawn(move || {
                let service = ModelCapabilitySettingsService::new(path);
                let result = service.save(&base, &patch);
                let authority = match &result {
                    Ok(authority) => Some(authority.clone()),
                    Err(_) => service.load().ok(),
                };
                let _ = sender.send((authority, result.err()));
            });
        if worker.is_err() {
            self.finish_model_capability_save(
                view,
                generation,
                operation_id,
                None,
                Some(ModelCapabilitySettingsErrorCode::Io),
                cx,
            );
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(super::session::MODEL_SELECTION_POLL)
                    .await;
                let (authority, error) = match receiver.try_recv() {
                    Ok(value) => value,
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        (None, Some(ModelCapabilitySettingsErrorCode::Io))
                    }
                };
                let _ = this.update(cx, |this, cx| {
                    this.finish_model_capability_save(
                        view,
                        generation,
                        operation_id,
                        authority,
                        error,
                        cx,
                    )
                });
                break;
            }
        })
        .detach();
    }

    fn finish_model_capability_save(
        &mut self,
        view: Entity<SettingsView>,
        generation: u64,
        operation_id: u64,
        authority: Option<ModelCapabilityAuthority>,
        error: Option<ModelCapabilitySettingsErrorCode>,
        cx: &mut Context<Self>,
    ) {
        if self.model_capability_pending != Some((generation, operation_id)) {
            return;
        }
        self.model_capability_pending = None;
        self.model_capability_authority = authority;
        self.model_capability_generation = self.model_capability_generation.wrapping_add(1);
        self.model_capability_projection = ModelCapabilitySettingsProjection::Ready {
            generation: self.model_capability_generation,
            models: self
                .model_capability_authority
                .as_ref()
                .map(|authority| authority.models.clone())
                .unwrap_or_default(),
            error,
        };
        if cx.global::<SettingsOpen>().0
            && self.settings_view.as_ref() == Some(&view)
            && view
                .read(cx)
                .model_capability_save_is_current(generation, operation_id)
        {
            let projection = self.model_capability_settings_projection();
            view.update(cx, |view, cx| {
                view.apply_model_capability_projection(projection, cx)
            });
        }
        self.invalidate_model_catalog();
        self.start_model_catalog_load(cx);
        cx.notify();
    }
}
