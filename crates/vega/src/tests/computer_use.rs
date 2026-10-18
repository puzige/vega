use super::*;
use gpui_kit::{TestAppContext, VisualTestContext};

fn fixture(
    cx: &mut TestAppContext,
) -> (
    TempDir,
    Entity<VegaWindow>,
    Entity<ConversationStream>,
    Thread,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    let project = vega_store::projects::create(
        store.conn(),
        dir.path().to_str().unwrap(),
        "computer-use",
        None,
    )
    .unwrap();
    let thread =
        vega_conversation::threads::create_thread(&store, &project.id, "mock", "confirm").unwrap();
    cx.update(|cx| install_diff_window_globals(store, thread.clone(), cx));
    let stream = cx.new(|cx| ConversationStream::new(thread.clone(), cx));
    let config_path = dir.path().join("config.toml");
    vega_store::config::AppConfig::default()
        .save_to(&config_path)
        .unwrap();
    let root = cx.new(VegaWindow::new);
    root.update(cx, |root, _| {
        root.model_selection_config_override = Some(config_path)
    });
    (dir, root, stream, thread)
}
