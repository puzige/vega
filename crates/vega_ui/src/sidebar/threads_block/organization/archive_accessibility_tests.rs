use super::*;
use gpui_kit::{
    Element,
    accesskit::{Action, Node, Role},
};

#[gpui_kit::test]
fn archive_control_exports_button_semantics_for_both_states(cx: &mut gpui_kit::TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("archive-accessibility.db")).unwrap();
    store.migrate().unwrap();
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(None));
        cx.set_global(OpenedThread(None));
    });
    let sidebar = cx.new(Sidebar::new);
    let sessions = sidebar.read_with(cx, |sidebar, _| sidebar.sessions_block.clone());
    for (archived, expected_label) in [(false, "显示已归档"), (true, "隐藏已归档")] {
        sessions.update(cx, |block, cx| {
            block.organization.as_mut().unwrap().archive = archived;
            let control = block.archive_control(true, cx);
            assert_eq!(
                Element::id(&control),
                Some(ElementId::Name("organization-archive".into())),
                "archive={archived}"
            );
            let role = Element::a11y_role(&control);
            assert_eq!(role, Some(Role::Button), "archive={archived}");
            let mut node = Node::new(role.unwrap());
            Element::write_a11y_info(&control, &mut node);
            assert_eq!(node.label(), Some(expected_label), "archive={archived}");
            assert!(node.supports_action(Action::Click), "archive={archived}");
            assert!(node.supports_action(Action::Focus), "archive={archived}");
        });
    }
}
