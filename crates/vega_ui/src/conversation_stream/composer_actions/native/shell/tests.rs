use super::*;
use nsh_core::element::{Component, NoEvent, Root, Scope};
use nsh_core::{AccessibilityRole, ApproxTextEngine, Core, MountItem};
use std::cell::RefCell;

thread_local! {
    static INITIAL: RefCell<Option<Style>> = const { RefCell::new(None) };
    static UPDATES: RefCell<Option<tokio::sync::mpsc::Sender<Style>>> = const { RefCell::new(None) };
}

struct ShellFixture;

impl Component for ShellFixture {
    type Props = ();
    type Event = NoEvent;

    fn view(_: (), _: Scope<NoEvent>) -> impl ns::IntoElement {
        let style = ns::signal(INITIAL.with(|slot| slot.borrow_mut().take().expect("style")));
        let (updates, mut incoming) = tokio::sync::mpsc::channel(1);
        UPDATES.with(|slot| *slot.borrow_mut() = Some(updates));
        ns::spawn(async move {
            while let Some(next) = incoming.recv().await {
                style.set(next);
            }
        });
        let (sender, _) = tokio::sync::mpsc::channel(1);
        render(style, sender)
    }
}

#[test]
fn shell_is_passive_native_decoration_and_repaints_real_theme_changes() {
    let colors = vega_theme::Theme::light().colors;
    let style = Style {
        background: colors.bg_elevated,
        border: colors.border_subtle,
    };
    INITIAL.with(|slot| *slot.borrow_mut() = Some(style.clone()));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(500.), ns::px(120.)));
    core.mount_root(&Root::of::<ShellFixture>());
    let first = core.commit().expect("shell mount");
    let nodes = first
        .items
        .iter()
        .filter(|item| matches!(item, MountItem::ViewProps { .. }))
        .count();
    assert!(nodes > 0);
    assert!(!first.items.iter().any(|item| matches!(item, MountItem::ViewProps { props, .. } if props.accessibility.role == AccessibilityRole::Button)));
    assert!(first.items.iter().any(|item| matches!(item,
        MountItem::ViewProps { props, .. }
            if props.accessibility.hidden
                && props.accessibility.accessible == Some(false)
                && props.pointer_events == nsh_core::PointerEvents::None
                && !props.focus_stop(false))));
    assert!(!first.items.iter().any(|item| matches!(item,
        MountItem::Events { events, .. } if !events.is_empty())));
    assert!(first.items.iter().any(|item| matches!(item,
        MountItem::ViewStyle { style: painted, .. }
            if painted.background_color == crate::nsh::color(style.background).into())));
    let mut changed = style;
    changed.background = vega_theme::Theme::dark().colors.bg_elevated;
    changed.border = vega_theme::Theme::dark().colors.border_subtle;
    let expected_background: nsh_core::Color = crate::nsh::color(changed.background).into();
    UPDATES.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("updates")
            .try_send(changed)
            .expect("theme update")
    });
    core.run_component_tasks();
    let updated = core.commit().expect("theme repaint");
    assert!(
        updated
            .items
            .iter()
            .any(|item| matches!(item, MountItem::ViewStyle { style, .. } if style.background_color == expected_background))
    );
}
