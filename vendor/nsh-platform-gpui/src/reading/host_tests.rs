#![allow(missing_docs)]

use super::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};

fn area() -> (Rc<Surface>, NshView) {
    let surface = Surface::new(nsh_engine::Environment::default());
    surface.mounter.borrow_mut().apply(
        &surface,
        &nsh_core::MountItem::Create {
            id: 2,
            kind: nsh_core::ViewKind::View,
        },
    );
    surface.mounter.borrow_mut().apply(
        &surface,
        &nsh_core::MountItem::Insert {
            parent: nsh_core::ROOT_ID,
            child: 2,
            before: None,
        },
    );
    let area = surface.view(2).expect("mounted area");
    area.state_mut().props.selectable_view = true;
    (surface, area)
}

pub fn single_text_install_preserves_direction_and_rejects_retired_snapshots() {
    crate::text::clear_selection();
    crate::selectable_view::clear_all();
    let surface = Surface::new(nsh_engine::Environment::default());
    let mount = |item| surface.mounter.borrow_mut().apply(&surface, &item);
    mount(nsh_core::MountItem::Create {
        id: 2,
        kind: nsh_core::ViewKind::Text,
    });
    mount(nsh_core::MountItem::Insert {
        parent: nsh_core::ROOT_ID,
        child: 2,
        before: None,
    });
    let content = Arc::new(AttributedText {
        text: "a😀é אב".into(),
        selectable: Some(true),
        ..Default::default()
    });
    let install_content = |content| {
        mount(nsh_core::MountItem::Text {
            id: 2,
            text: content,
            layout: nsh_core::TextLayout(Arc::new(crate::text::GpuiLayout::default())),
            content: crate::geom::rect(0.0, 0.0, 120.0, 20.0),
        });
    };
    install_content(content.clone());
    let view = surface.view(2).expect("mounted text");
    let empty = snapshot(&surface, &view).expect("empty snapshot");
    assert!(!empty.cross_text);
    assert!(empty.ranges.is_empty());
    let reverse = install(&surface, &empty, (0, 4), (0, 1)).expect("reverse install");
    assert_eq!(reverse.ranges, vec![(0, 1..4)]);
    assert_eq!(reverse.endpoints, Some(((0, 4), (0, 1))));
    assert!(Arc::ptr_eq(&reverse.texts[0].content, &content));
    assert!(!is_current(&surface, &empty));
    for (anchor, focus) in [((0, 2), (0, 4)), ((0, 0), (0, 8)), ((1, 0), (0, 4))] {
        assert!(install(&surface, &reverse, anchor, focus).is_none());
        assert!(is_current(&surface, &reverse));
    }
    let other = Surface::new(nsh_engine::Environment::default());
    assert!(install(&other, &reverse, (0, 1), (0, 4)).is_none());
    let forward = install(&surface, &reverse, (0, 1), (0, 4)).expect("direction flip");
    assert_eq!(forward.ranges, reverse.ranges);
    assert_eq!(forward.endpoints, Some(((0, 1), (0, 4))));
    assert!(!is_current(&surface, &reverse));
    let collapsed = install(&surface, &forward, (0, 3), (0, 3)).expect("collapse");
    assert!(collapsed.ranges.is_empty());
    assert!(collapsed.endpoints.is_none());
    assert!(!is_current(&surface, &forward));
    let restored = install(&surface, &collapsed, (0, 4), (0, 1)).expect("restore");
    crate::text::clear_selection();
    assert!(!is_current(&surface, &restored));
    let cleared = snapshot(&surface, &view).expect("cleared snapshot");
    assert!(cleared.ranges.is_empty());
    assert!(cleared.endpoints.is_none());
    let restored = install(&surface, &cleared, (0, 4), (0, 1)).expect("reselect");
    assert!(!is_current(&surface, &reverse));
    install_content(Arc::new((*content).clone()));
    assert!(!is_current(&surface, &restored));
    let replacement = snapshot(&surface, &view).expect("replacement snapshot");
    install_content(Arc::new(AttributedText {
        text: "different".into(),
        selectable: Some(true),
        ..Default::default()
    }));
    assert!(!is_current(&surface, &replacement));
    assert!(snapshot(&surface, &view).expect("new content").ranges.is_empty());
    view.unparent();
    assert!(snapshot(&surface, &view).is_none());
    assert!(install(&surface, &replacement, (0, 0), (0, 1)).is_none());
    mount(nsh_core::MountItem::Remove { id: 2 });
    mount(nsh_core::MountItem::Create {
        id: 2,
        kind: nsh_core::ViewKind::Text,
    });
    mount(nsh_core::MountItem::Insert {
        parent: nsh_core::ROOT_ID,
        child: 2,
        before: None,
    });
    install_content(content);
    assert!(snapshot(&surface, &view).is_none());
    let current = snapshot(&surface, &surface.view(2).expect("new mount")).expect("new snapshot");
    surface.stop();
    assert!(!is_current(&surface, &current));
    crate::text::clear_selection();
}

pub fn takeover_is_fail_closed_after_detach_and_scoped_to_surface() {
    let (surface, area) = area();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let sink = requests.clone();
    *surface.reading_handler.borrow_mut() =
        Some(Rc::new(move |request| sink.borrow_mut().push(request)));
    for intent in [
        ReadingIntent::Copy,
        ReadingIntent::ContextMenu,
        ReadingIntent::PrimarySelection,
    ] {
        assert!(intercept(&surface, &area, intent));
    }
    assert_eq!(requests.borrow().len(), 3);
    let captured = requests.borrow()[0].snapshot.clone();
    assert!(is_current(&surface, &captured));
    let (other, _) = self::area();
    assert!(!is_current(&other, &captured));
    assert!(!intercept(&other, &area, ReadingIntent::Copy));
    area.unparent();
    assert!(!is_current(&surface, &captured));
    assert!(intercept(&surface, &area, ReadingIntent::Copy));
    assert_eq!(requests.borrow().len(), 3);
}

pub fn pending_clipboard_write_is_revoked_by_takeover_aba_and_stop() {
    let (surface, _) = area();
    let pending = ClipboardLease::capture(&surface).expect("ordinary copy permitted");
    assert!(pending.current(&surface));
    set_handler(&surface, Some(Rc::new(|_| {})));
    assert!(!pending.current(&surface));
    assert!(ClipboardLease::capture(&surface).is_none());
    set_handler(&surface, None);
    assert!(!pending.current(&surface));
    let fresh = ClipboardLease::capture(&surface).expect("new copy permitted");
    assert!(fresh.current(&surface));
    surface.stop();
    assert!(!fresh.current(&surface));
}

pub fn snapshots_reject_selection_aba_and_distinct_content_objects() {
    let (surface, area) = area();
    let content = Arc::new(AttributedText {
        text: "a😀é אב".into(),
        ..Default::default()
    });
    let text = ReadingText {
        node: 3,
        content: content.clone(),
        view: area.clone(),
    };
    let first = ReadingSnapshot::new(
        &surface,
        area.clone(),
        vec![text.clone()],
        vec![(0, 1..3)],
        true,
    );
    let equivalent =
        ReadingSnapshot::new(&surface, area.clone(), vec![text], vec![(0, 1..3)], true);
    assert!(first.matches(&equivalent));
    let replaced = ReadingText {
        node: 3,
        content: Arc::new((*content).clone()),
        view: area.clone(),
    };
    let replaced = ReadingSnapshot::new(
        &surface,
        area.clone(),
        vec![replaced],
        vec![(0, 1..3)],
        true,
    );
    assert!(!first.matches(&replaced));
    changed();
    let current = ReadingSnapshot::new(
        &surface,
        area.clone(),
        vec![ReadingText {
            node: 3,
            content,
            view: area,
        }],
        vec![(0, 1..3)],
        true,
    );
    assert!(!first.matches(&current));
}
