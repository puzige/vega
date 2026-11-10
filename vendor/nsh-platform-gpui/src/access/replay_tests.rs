use super::*;
use gpui::{AppContext as _, Context, Entity, Render, StyleRefinement, TestAppContext};
use std::{cell::Cell, collections::HashSet};

struct Semantics {
    renders: Rc<Cell<usize>>,
    focus: FocusHandle,
    flat: bool,
    shared_owner: bool,
}

fn entries() -> FxHashMap<accesskit::NodeId, Entry> {
    let mut parent = Node::new(Role::Group);
    parent.set_label("group");
    parent.set_children(vec![node_id(7, 2), node_id(7, 3)]);
    parent.set_bounds(accesskit::Rect::new(20., 40., 420., 240.));
    let mut button = Node::new(Role::Button);
    button.set_label("save");
    button.add_action(Action::Click);
    button.add_action(Action::Focus);
    button.set_bounds(accesskit::Rect::new(40., 60., 160., 120.));
    let mut field = Node::new(Role::TextInput);
    field.set_label("name");
    field.set_value("draft");
    field.add_action(Action::SetValue);
    field.add_action(Action::Focus);
    field.set_bounds(accesskit::Rect::new(40., 140., 300., 200.));
    [(1, parent), (2, button), (3, field)]
        .into_iter()
        .map(|(id, node)| {
            let actions = [Action::Click, Action::SetValue, Action::Focus]
                .into_iter()
                .filter(|action| node.supports_action(*action))
                .collect();
            (
                node_id(7, id as NodeId),
                Entry {
                    id: node_id(7, id as NodeId),
                    view: id as NodeId,
                    node,
                    actions,
                },
            )
        })
        .collect()
}

impl Render for Semantics {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        let mut entries = entries();
        if self.shared_owner {
            for entry in entries.values_mut() {
                entry.view = 1;
            }
        }
        let child = if self.flat {
            div()
                .id("flat")
                .role(Role::Group)
                .a11y_synthetic_children(move |builder: &mut A11ySubtreeBuilder| {
                    for id in [2, 3, 1] {
                        let entry = entries.remove(&node_id(7, id)).unwrap();
                        builder.push_child(entry.id, entry.node);
                    }
                    builder.parent_node().set_children(vec![node_id(7, 1)]);
                })
                .into_any_element()
        } else {
            let focus = self.focus.clone();
            let focused = if self.shared_owner { 1 } else { 3 };
            gpui::canvas(
                move |_, window, cx| {
                    let mut child = semantic_element(
                        node_id(7, 1),
                        &mut entries,
                        7,
                        Some(focused),
                        &focus,
                        2.,
                        (10., 20.),
                    )
                    .unwrap();
                    child.prepaint_as_root(
                        point(px(10.), px(20.)),
                        size(AvailableSpace::MinContent, AvailableSpace::MinContent),
                        window,
                        cx,
                    );
                    child
                },
                |_, mut child, window, cx| child.paint(window, cx),
            )
            .size_full()
            .into_any_element()
        };
        div().relative().size_full().child(child)
    }
}

struct Host {
    child: Entity<Semantics>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            self.child
                .clone()
                .cached(StyleRefinement::default().size_full()),
        )
    }
}

fn exercise(flat: bool, shared_owner: bool) {
    let mut cx = TestAppContext::single();
    let renders = Rc::new(Cell::new(0));
    let window = cx.add_window({
        let renders = renders.clone();
        move |window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Host {
                child: cx.new(|_| Semantics {
                    renders,
                    focus,
                    flat,
                    shared_owner,
                }),
            }
        }
    });
    let mut initial = None;
    for frame in 0..4 {
        window.update(&mut cx, |_, _, cx| cx.notify()).unwrap();
        let tree = cx
            .update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear(cx);
                serde_json::from_str::<serde_json::Value>(
                    &window.debug_a11y_tree_json().expect("active AX tree"),
                )
                .unwrap()
            })
            .unwrap();
        let nodes = tree["nodes"].as_object().unwrap();
        let mut seen = HashSet::new();
        for node in nodes.values() {
            if let Some(children) = node["children"].as_array() {
                for child in children {
                    assert!(
                        seen.insert(child.as_str().unwrap()),
                        "child has multiple parents"
                    );
                }
            }
        }
        if flat {
            continue;
        }
        let find = |label| {
            nodes
                .iter()
                .find(|(_, node)| node["aria"]["label"] == label)
                .unwrap()
        };
        let (group_id, group) = find("group");
        let (button_id, button) = find("save");
        let (field_id, field) = find("name");
        assert_eq!(group["children"], serde_json::json!([button_id, field_id]));
        assert_eq!(button["aria"]["role"], "Button");
        assert_eq!(field["aria"]["role"], "TextInput");
        assert_eq!(field["aria"]["value"], "draft");
        assert!(
            button["aria"]["on_action"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("Click"))
        );
        assert!(
            field["aria"]["on_action"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("SetValue"))
        );
        assert_eq!(
            tree["gpui_focus"].as_str(),
            Some(if shared_owner { group_id } else { field_id }.as_str())
        );
        let semantics = (
            group.clone(),
            button.clone(),
            field.clone(),
            group_id.clone(),
        );
        if let Some(initial) = &initial {
            assert_eq!(&semantics, initial);
        } else {
            initial = Some(semantics);
        }
        if frame > 0 {
            assert_eq!(
                renders.get(),
                1,
                "cached child must replay instead of rerender"
            );
        }
    }
    if !flat && !shared_owner {
        let focus = window
            .update(&mut cx, |host, _, cx| host.child.read(cx).focus.clone())
            .unwrap();
        let mut visual = gpui::VisualTestContext::from_window(window.into(), &cx);
        visual.update(|window, cx| window.blur(cx));
        visual.simulate_mouse_down(
            point(px(15.), px(55.)),
            gpui::MouseButton::Left,
            gpui::Modifiers::default(),
        );
        assert!(
            !visual.update(|window, _| focus.is_focused(window)),
            "old unshifted hitbox must not focus"
        );
        visual.simulate_mouse_down(
            point(px(145.), px(95.)),
            gpui::MouseButton::Left,
            gpui::Modifiers::default(),
        );
        assert!(
            visual.update(|window, _| focus.is_focused(window)),
            "nested hitbox at actual root origin must focus"
        );
    }
}

#[test]
#[should_panic(expected = "child has multiple parents")]
fn flat_synthetic_hierarchy_reproduces_replay_failure() {
    exercise(true, false);
}

#[test]
fn semantic_hierarchy_survives_cached_replay_with_focus_and_actions() {
    exercise(false, false);
}

#[test]
fn shared_action_owner_has_only_one_focused_semantic_node() {
    exercise(false, true);
}
