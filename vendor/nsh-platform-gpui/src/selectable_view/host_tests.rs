#![allow(missing_docs)]

use super::*;

fn mounted_area() -> (Rc<Surface>, NshView, Vec<Arc<AttributedText>>) {
    let surface = Surface::new(nsh_engine::Environment::default());
    let mount = |item| surface.mounter.borrow_mut().apply(&surface, &item);
    mount(nsh_core::MountItem::Create {
        id: 2,
        kind: nsh_core::ViewKind::View,
    });
    mount(nsh_core::MountItem::Insert {
        parent: nsh_core::ROOT_ID,
        child: 2,
        before: None,
    });
    let area = surface.view(2).expect("mounted area");
    area.state_mut().props.selectable_view = true;
    let mut contents = Vec::new();
    for (id, text) in [(3, "a😀é"), (4, "אבג combined e\u{301}")] {
        mount(nsh_core::MountItem::Create {
            id,
            kind: nsh_core::ViewKind::Text,
        });
        mount(nsh_core::MountItem::Insert {
            parent: 2,
            child: id,
            before: None,
        });
        let content = Arc::new(AttributedText {
            text: text.into(),
            ..Default::default()
        });
        mount(nsh_core::MountItem::Text {
            id,
            text: content.clone(),
            layout: nsh_core::TextLayout(Arc::new(crate::text::GpuiLayout::default())),
            content: crate::geom::rect(0.0, 0.0, 120.0, 20.0),
        });
        contents.push(content);
    }
    (surface, area, contents)
}

pub fn selection_listener_tracks_mount_clear_and_reentrant_retirement() {
    clear_all();
    let (surface, area, _) = mounted_area();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let weak = Rc::downgrade(&surface);
    assert!(crate::reading::set_listener(
        &surface,
        area.id(),
        Some(Rc::new(move |current| {
            let surface = weak.upgrade().expect("surface");
            if let Some(current) = &current {
                assert!(crate::reading::is_current(&surface, current));
            }
            output.borrow_mut().push(current);
            crate::reading::publish(&surface);
        }))
    ));
    assert_eq!(seen.borrow().len(), 1);
    assert!(seen.borrow()[0].is_none());
    select_all(surface.key(), &area);
    crate::reading::publish(&surface);
    assert_eq!(seen.borrow().len(), 2);
    let selected = seen.borrow()[1].clone().expect("selection");
    assert_eq!(selected.ranges, vec![(0, 0..4), (1, 0..15)]);
    crate::reading::publish(&surface);
    assert_eq!(seen.borrow().len(), 2);
    clear_all();
    assert!(!crate::reading::is_current(&surface, &selected));
    crate::reading::publish(&surface);
    assert!(seen.borrow().last().expect("clear").is_none());
    select_all(surface.key(), &area);
    crate::reading::publish(&surface);
    area.unparent();
    crate::reading::publish(&surface);
    assert!(seen.borrow().last().expect("detach").is_none());
    let before = seen.borrow().len();
    surface.stop();
    assert_eq!(seen.borrow().len(), before + 1);
    assert!(surface.reading_listener.borrow().is_none());
    let (surface, area, _) = mounted_area();
    let rebinds = Rc::new(RefCell::new(Vec::new()));
    let results = rebinds.clone();
    let weak = Rc::downgrade(&surface);
    let initial = Rc::new(std::cell::Cell::new(true));
    assert!(crate::reading::set_listener(
        &surface,
        area.id(),
        Some(Rc::new(move |_| {
            if !initial.replace(false) {
                let surface = weak.upgrade().expect("surface");
                results.borrow_mut().push(crate::reading::set_listener(
                    &surface,
                    2,
                    Some(Rc::new(|_| {})),
                ));
            }
        }))
    ));
    assert!(crate::reading::set_listener(&surface, area.id(), None));
    assert_eq!(*rebinds.borrow(), [false]);
    assert!(surface.reading_listener.borrow().is_none());
    clear_all();
}

pub fn exact_endpoints_preserve_direction_and_reject_stale_or_split_surrogates() {
    clear_all();
    let (surface, area, _) = mounted_area();
    let empty = crate::reading::snapshot(&surface, &area).expect("empty snapshot");
    let selected =
        crate::reading::install(&surface, &empty, (1, 3), (0, 1)).expect("reverse selection");
    assert_eq!(selected.endpoints, Some(((1, 3), (0, 1))));
    assert_eq!(selected.ranges, vec![(0, 1..4), (1, 0..3)]);
    assert!(crate::reading::is_current(&surface, &selected));
    assert!(crate::reading::install(&surface, &empty, (0, 0), (1, 3)).is_none());
    assert!(crate::reading::install(&surface, &selected, (0, 2), (1, 3)).is_none());
    assert!(crate::reading::is_current(&surface, &selected));
    assert!(crate::reading::install(&surface, &selected, (3, 0), (1, 3)).is_none());
    let other = Surface::new(nsh_engine::Environment::default());
    assert!(crate::reading::install(&other, &selected, (0, 0), (1, 3)).is_none());
    let forward =
        crate::reading::install(&surface, &selected, (0, 1), (1, 3)).expect("forward selection");
    assert_eq!(forward.ranges, selected.ranges);
    assert_eq!(forward.endpoints, Some(((0, 1), (1, 3))));
    assert!(!crate::reading::is_current(&surface, &selected));
    assert_eq!(area.state().props.selectable_view, true);
    assert_eq!(
        surface
            .view(3)
            .expect("first")
            .state()
            .text
            .as_ref()
            .expect("text")
            .area_range,
        Some((1, 4))
    );
    assert_eq!(
        surface
            .view(4)
            .expect("second")
            .state()
            .text
            .as_ref()
            .expect("text")
            .area_range,
        Some((0, 3))
    );
    clear_all();
}

pub fn mounted_cross_text_copy_uses_exact_content_and_rejects_replacement() {
    let (surface, area, contents) = mounted_area();
    let mount = |item| surface.mounter.borrow_mut().apply(&surface, &item);
    select_all(surface.key(), &area);
    let captured = crate::reading::snapshot(&surface, &area).expect("current snapshot");
    assert_eq!(captured.texts.len(), 2);
    assert_eq!(captured.ranges, vec![(0, 0..4), (1, 0..15)]);
    for (actual, expected) in captured.texts.iter().zip(&contents) {
        assert!(Arc::ptr_eq(&actual.content, expected));
    }
    let requests = Rc::new(RefCell::new(Vec::new()));
    let sink = requests.clone();
    *surface.reading_handler.borrow_mut() = Some(Rc::new(move |r| sink.borrow_mut().push(r)));
    let event = KeyEvent {
        vk: 0x43,
        down: true,
        repeat: false,
        modifiers: crate::input::Modifiers {
            meta: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Default::default()
        },
    };
    assert!(key(&surface, &event));
    assert_eq!(requests.borrow().len(), 1);
    assert_eq!(requests.borrow()[0].snapshot.ranges, captured.ranges);
    let replacement = Arc::new(AttributedText {
        text: "replacement".into(),
        ..Default::default()
    });
    mount(nsh_core::MountItem::Text {
        id: 3,
        text: replacement,
        layout: nsh_core::TextLayout(Arc::new(crate::text::GpuiLayout::default())),
        content: crate::geom::rect(0.0, 0.0, 120.0, 20.0),
    });
    batch_applied(&surface);
    assert!(!crate::reading::is_current(&surface, &captured));
    mount(nsh_core::MountItem::Remove { id: 2 });
    assert!(!crate::reading::is_current(&surface, &captured));
    clear_all();
}
