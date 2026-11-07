use super::*;

impl SettingsView {
    pub(crate) fn render_reasoning(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_model_capability_efforts(window, cx)
    }
}
