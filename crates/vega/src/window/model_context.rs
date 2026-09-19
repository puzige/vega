//! Worker-owned, exact provider/model context-policy Settings operations.
use super::*;
use vega_store::Store;
use vega_store::context_compaction::{
    ModelContextPolicy, has_legacy_model_settings, load_model_policy, save_model_policy,
};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

impl VegaWindow {
    pub(crate) fn request_model_context_load(
        &mut self,
        view: Entity<SettingsView>,
        request: &ModelContextLoadRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0 || self.settings_view.as_ref() != Some(&view) {
            return;
        }
        let Some(database) = self.context_database(cx) else {
            view.update(cx, |view, cx| {
                view.apply_model_context_loaded(
                    request,
                    Err("模型上下文配置数据库不可用".into()),
                    cx,
                )
            });
            return;
        };
        let request = request.clone();
        cx.spawn(async move |this, cx| {
            let worker_request = request.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = Store::open_read_only(database).map_err(|_| ())?;
                    let policy = load_model_policy(
                        store.conn(),
                        &worker_request.provider,
                        &worker_request.model,
                    )
                    .map_err(|_| ())?;
                    let legacy_present =
                        has_legacy_model_settings(store.conn(), &worker_request.model)
                            .map_err(|_| ())?;
                    Ok::<_, ()>(ModelContextLoaded {
                        policy,
                        legacy_present,
                    })
                })
                .await
                .map_err(|_| "模型上下文配置读取失败，请重试".to_string());
            let _ = this.update(cx, |this, cx| {
                if cx.global::<SettingsOpen>().0 && this.settings_view.as_ref() == Some(&view) {
                    view.update(cx, |view, cx| {
                        view.apply_model_context_loaded(&request, result, cx)
                    });
                }
            });
        })
        .detach();
    }

    pub(crate) fn request_model_context_save(
        &mut self,
        view: Entity<SettingsView>,
        request: &ModelContextSaveRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0 || self.settings_view.as_ref() != Some(&view) {
            return;
        }
        let Some(database) = self.context_database(cx) else {
            view.update(cx, |view, cx| {
                view.apply_model_context_saved(
                    request,
                    Err("模型上下文配置数据库不可用".into()),
                    cx,
                )
            });
            return;
        };
        let request = request.clone();
        cx.spawn(async move |this, cx| {
            let worker_request = request.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = Store::open(database).map_err(|_| ())?;
                    let policy = ModelContextPolicy {
                        provider: worker_request.provider.clone(),
                        model: worker_request.model.clone(),
                        input_limit: worker_request.input_limit,
                        output_reserve: worker_request.output_limit,
                        automatic_compaction: worker_request.automatic_compaction,
                        updated_at: now_ms(),
                    };
                    save_model_policy(store.conn(), &policy).map_err(|_| ())?;
                    load_model_policy(store.conn(), &policy.provider, &policy.model)
                        .map_err(|_| ())?
                        .ok_or(())
                })
                .await
                .map_err(|_| "模型上下文配置保存失败，请重试".to_string());
            let _ = this.update(cx, |this, cx| {
                if cx.global::<SettingsOpen>().0 && this.settings_view.as_ref() == Some(&view) {
                    view.update(cx, |view, cx| {
                        view.apply_model_context_saved(&request, result, cx)
                    });
                }
            });
        })
        .detach();
    }
}
