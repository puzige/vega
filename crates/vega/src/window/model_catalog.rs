use super::*;
use vega_conversation::types::{CatalogAction, ModelCatalogProjection, ModelCatalogRequested};

impl VegaWindow {
    pub(crate) fn request_model_catalog(
        &mut self,
        view: Entity<SettingsView>,
        request: &ModelCatalogRequested,
        cx: &mut Context<Self>,
    ) {
        if !cx.global::<SettingsOpen>().0 || self.settings_view.as_ref() != Some(&view) {
            return;
        }
        let Some(root) = self
            .composer_config_path()
            .and_then(|p| p.parent().map(|p| p.join("model-catalog")))
        else {
            view.update(cx, |view, cx| {
                view.apply_model_catalog(
                    request,
                    ModelCatalogProjection {
                        error: Some("模型目录缓存路径不可用；仍可手动配置".into()),
                        ..Default::default()
                    },
                    cx,
                )
            });
            return;
        };
        let request = request.clone();
        cx.spawn(async move |this, cx| {
            let worker_root = root.clone();
            let worker_request = request.clone();
            let mut cached = cx
                .background_executor()
                .spawn(async move {
                    crate::model_catalog::load(
                        &worker_root,
                        &worker_request,
                        crate::model_catalog::now_ms(),
                    )
                })
                .await;
            let refresh = request.action == CatalogAction::Refresh
                || (request.action == CatalogAction::Load
                    && (cached.stale || cached.fetched_at_ms.is_none()));
            cached.refreshing = refresh;
            let active = this
                .update(cx, |this, cx| {
                    if !cx.global::<SettingsOpen>().0 || this.settings_view.as_ref() != Some(&view)
                    {
                        return false;
                    }
                    view.update(cx, |view, cx| {
                        view.apply_model_catalog(&request, cached, cx)
                    })
                })
                .unwrap_or(false);
            if !refresh || !active {
                return;
            }
            let owner = request.owner.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::model_catalog::refresh(&root, &owner, crate::model_catalog::now_ms())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if cx.global::<SettingsOpen>().0 && this.settings_view.as_ref() == Some(&view) {
                    view.update(cx, |view, cx| {
                        view.apply_model_catalog(&request, result, cx);
                    });
                }
            });
        })
        .detach();
    }
}
