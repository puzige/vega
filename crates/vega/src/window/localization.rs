use super::*;

impl VegaWindow {
    pub(super) fn sync_language(&mut self, cx: &mut Context<Self>) {
        let language = vega_i18n::language();
        if self.language == language {
            return;
        }
        self.language = language;
        self.publish_updater(cx);
        self.refresh_image_language(cx);
        let mut streams = std::collections::HashMap::new();
        for stream in self
            .stream_view
            .iter()
            .map(|(_, stream)| stream)
            .chain(self.agent_controller.active.values().map(|run| &run.stream))
            .chain(self.agent_controller.queue_streams.values())
            .chain(self.automation_controller.streams.values())
        {
            streams.insert(stream.entity_id(), stream.clone());
        }
        for stream in streams.into_values() {
            stream.update(cx, |stream, cx| {
                let pending = stream.pending_queue().clone();
                let error = pending
                    .error
                    .as_deref()
                    .map(crate::app_localization::localized_error);
                stream.apply_pending_queue_localized(pending, error, cx);
                let automation = stream.automation_projection().clone();
                crate::app_localization::apply_automation(stream, automation, cx);
            });
        }
    }
}
