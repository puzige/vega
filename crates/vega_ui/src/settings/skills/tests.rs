use super::*;
use gpui_kit::{
    Bounds, Context, Entity, Render, TestAppContext, VisualTestContext, Window, WindowBounds,
    WindowHandle, WindowOptions, size,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vega_store::Store;

#[derive(Debug, PartialEq, Eq)]
struct ImportAuthority {
    settings: vega_store::skills::SkillSettings,
    sources: Vec<vega_store::skills::SkillSourceRecord>,
    approvals: Vec<vega_store::skills::SkillApprovalRecord>,
}

struct Harness(Entity<SettingsView>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}
