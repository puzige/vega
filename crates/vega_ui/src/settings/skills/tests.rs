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

fn import_authority(store: &Store) -> ImportAuthority {
    ImportAuthority {
        settings: vega_store::skills::read_settings(store.conn()).unwrap(),
        sources: vega_store::skills::list_sources(store.conn()).unwrap(),
        approvals: vega_store::skills::list_approved_skills(store.conn()).unwrap(),
    }
}

struct Harness(Entity<SettingsView>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}
