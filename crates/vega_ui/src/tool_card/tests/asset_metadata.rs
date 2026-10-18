use super::*;
use gpui_kit::{
    Bounds, Context, Render, TestAppContext, VisualTestContext, Window, WindowBounds, WindowHandle,
    WindowOptions, size,
};

struct AssetHarness(Entity<ToolCard>);

impl Render for AssetHarness {}
