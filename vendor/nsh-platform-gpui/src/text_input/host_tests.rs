#![allow(missing_docs)]

use super::*;
use crate::editing::InputPastePayload;
use crate::keys::VK_SPACE;

impl crate::AppView {
    pub fn integration_edit_snapshot(&self, node: u32) -> Option<crate::editing::InputEdit> {
        let surface = self.surface();
        let view = surface.view(node)?;
        let input = input_of(&view)?;
        native_snapshot(surface, input.as_ref())
    }

    pub fn integration_edit_focus(&self, node: u32) -> bool {
        let surface = self.surface();
        let Some(view) = surface.view(node).filter(|view| input_of(view).is_some()) else {
            return false;
        };
        set_focus(surface, Some(&view));
        focused(surface).as_ref() == Some(&view)
    }

    pub fn integration_edit_type(&self, node: u32, text: &str) -> bool {
        let surface = self.surface();
        if focused(surface).is_none_or(|view| view.id() != node) {
            return false;
        }
        ime::replace_text_in_range(surface, None, text);
        true
    }

    pub fn integration_edit_mark(&self, node: u32, text: &str) -> bool {
        let surface = self.surface();
        if focused(surface).is_none_or(|view| view.id() != node) {
            return false;
        }
        ime::replace_and_mark_text_in_range(surface, None, text, None);
        true
    }

    pub fn integration_edit_unmark(&self) {
        ime::unmark_text(self.surface());
    }

    pub fn integration_edit_set_value(&self, node: u32, text: &str) -> bool {
        let surface = self.surface();
        surface
            .view(node)
            .is_some_and(|view| set_accessible_value(surface, &view, text))
    }

    pub fn integration_edit_menu(&self, node: u32, redo: bool) -> bool {
        let surface = self.surface();
        let Some(input) = surface.view(node).as_ref().and_then(input_of) else {
            return false;
        };
        let role = if redo {
            nsh_core::desktop::MenuRole::Redo
        } else {
            nsh_core::desktop::MenuRole::Undo
        };
        let action = menu_items(surface, &input, false)
            .into_iter()
            .find_map(|item| match item {
                crate::chrome::LocalItem::Item {
                    role: Some(current),
                    enabled: true,
                    action,
                    ..
                } if current == role => Some(action),
                _ => None,
            });
        if let Some(action) = action {
            action(surface);
            true
        } else {
            false
        }
    }

    pub fn integration_edit_key(&self, key: crate::editing::InputKey) -> bool {
        crate::input::key(
            self.surface(),
            KeyEvent {
                vk: key.virtual_key,
                down: key.down,
                repeat: key.repeat,
                modifiers: Modifiers {
                    shift: key.shift,
                    ctrl: key.control,
                    alt: key.alt,
                    meta: key.meta,
                },
            },
        )
    }

    pub fn integration_edit_paste(&self, node: u32, item: Option<gpui::ClipboardItem>) -> bool {
        let surface = self.surface();
        let Some(view) = surface.view(node) else {
            return false;
        };
        let Some(input) = input_of(&view) else {
            return false;
        };
        let Some(hooks) = input.hooks.borrow().clone() else {
            return false;
        };
        session(surface, &input, hooks).apply(
            surface,
            &view,
            &input,
            InputPastePayload::native(item),
        );
        true
    }

    pub fn integration_edit_press_ready(&self, label: &str) -> bool {
        let surface = self.surface();
        let mut nodes = vec![surface.root.clone()];
        while let Some(view) = nodes.pop() {
            let matches = view
                .state()
                .text
                .as_ref()
                .is_some_and(|text| text.text.text == label);
            if matches {
                let mut parent = Some(view);
                while let Some(view) = parent {
                    let state = view.state();
                    if !view.attached() || state.props.accessibility.state.disabled {
                        return false;
                    }
                    if state.events.contains(EventKind::Press) {
                        return true;
                    }
                    drop(state);
                    parent = view.parent_view();
                }
                return false;
            }
            nodes.extend(view.child_views());
        }
        false
    }

    pub fn integration_edit_press(&self, label: &str) -> bool {
        let surface = self.surface();
        let mut nodes = vec![surface.root.clone()];
        while let Some(view) = nodes.pop() {
            let matches = view
                .state()
                .text
                .as_ref()
                .is_some_and(|text| text.text.text == label);
            if matches {
                let mut parent = Some(view);
                while let Some(view) = parent {
                    if !view.attached() || view.state().props.accessibility.state.disabled {
                        return false;
                    }
                    if crate::access::activate(surface, &view) {
                        return true;
                    }
                    parent = view.parent_view();
                }
                return false;
            }
            nodes.extend(view.child_views());
        }
        false
    }
}

fn inert_hooks() -> crate::editing::InputHooks {
    crate::editing::InputHooks {
        key: Rc::new(|_, _| false),
        edit: Rc::new(|_| {}),
        focus: Rc::new(|_, _| {}),
        paste_begin: Rc::new(|_| false),
        paste: Rc::new(|_| crate::editing::PasteDecision::Reject),
        paste_complete: Rc::new(|_| {}),
    }
}

fn field() -> (Rc<Surface>, NshView, Rc<InputState>) {
    let surface = Surface::new(nsh_engine::Environment::default());
    surface.mounter.borrow_mut().apply(
        &surface,
        &nsh_core::MountItem::Create {
            id: 2,
            kind: nsh_core::ViewKind::Input,
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
    let view = surface.view(2).expect("mounted input");
    let input = input_of(&view).expect("native input");
    input.focused.set(true);
    FOCUS.with(|focus| {
        focus.borrow_mut().insert(
            surface.key(),
            Focus {
                view: Some(view.clone()),
                ..Default::default()
            },
        );
    });
    (surface, view, input)
}

fn session(
    surface: &Surface,
    input: &InputState,
    hooks: crate::editing::InputHooks,
) -> PasteSession {
    let before = native_snapshot(surface, input).expect("snapshot");
    assert!((hooks.paste_begin)(before.clone()));
    PasteSession {
        before,
        registration: input.hook_registration.borrow().clone(),
        hooks,
        completed: false,
    }
}

pub fn paste_is_one_native_transaction_and_attachment_only_is_not_lost() {
    let (surface, view, input) = field();
    input.model.borrow_mut().set_text("a😀z");
    input.model.borrow_mut().select(5, 1);
    let edits = Rc::new(Cell::new(0));
    let edit_count = edits.clone();
    let completed = Rc::new(RefCell::new(Vec::new()));
    let sink = completed.clone();
    let hooks = crate::editing::InputHooks {
        edit: Rc::new(move |_| edit_count.set(edit_count.get() + 1)),
        paste_begin: Rc::new(|before| !before.composing),
        paste: Rc::new(|event| {
            assert_eq!(event.before.text, "a😀z");
            assert_eq!((event.before.anchor_utf16, event.before.head_utf16), (3, 1));
            crate::editing::PasteDecision::Insert(event.payload.text().replace("\r\n", "\n"))
        }),
        paste_complete: Rc::new(move |result| sink.borrow_mut().push(result)),
        ..inert_hooks()
    };
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(hooks.clone())
    ));
    assert_eq!(edits.get(), 1);
    let mut payload = nsh_core::PasteEvent::new("中\r\n");
    payload.html = "<p>original</p>".into();
    session(&surface, &input, hooks.clone()).apply(
        &surface,
        &view,
        &input,
        InputPastePayload::Synthetic(payload.clone()),
    );
    {
        let results = completed.borrow();
        let result = results[0].as_ref().expect("completed paste");
        assert_eq!(result.edit.text, "a中\nz");
        assert_eq!((result.edit.anchor_utf16, result.edit.head_utf16), (3, 3));
        assert_eq!(result.edit.native_edit_serial, 2);
        assert_eq!(
            result.edit.mutation,
            Some(crate::editing::InputMutation {
                range_utf16: (1, 3),
                inserted: "中\n".into(),
            })
        );
        assert!(
            matches!(result.payload.as_ref(), InputPastePayload::Synthetic(value) if value == &payload)
        );
    }
    assert_eq!(edits.get(), 1);
    let sink = completed.clone();
    let attachment_hooks = crate::editing::InputHooks {
        paste_begin: Rc::new(|_| true),
        paste: Rc::new(|_| crate::editing::PasteDecision::Insert(String::new())),
        paste_complete: Rc::new(move |result| sink.borrow_mut().push(result)),
        ..inert_hooks()
    };
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(attachment_hooks.clone())
    ));
    let serial = input.native_serial.get();
    let mut attachment = nsh_core::PasteEvent::default();
    attachment.uris.push("file:///tmp/original.png".into());
    session(&surface, &input, attachment_hooks).apply(
        &surface,
        &view,
        &input,
        InputPastePayload::Synthetic(attachment.clone()),
    );
    let results = completed.borrow();
    let result = results[1].as_ref().expect("attachment transaction");
    assert_eq!(result.edit.text, "a中\nz");
    assert_eq!(result.edit.native_edit_serial, serial + 1);
    assert_eq!(result.edit.mutation, None);
    assert!(
        matches!(result.payload.as_ref(), InputPastePayload::Synthetic(value) if value == &attachment)
    );
}

pub fn paste_completion_blur_preserves_engine_accounting_and_rejects_stale_value() {
    let (surface, view, input) = field();
    input.model.borrow_mut().set_text("draft");
    let weak = Rc::downgrade(&surface);
    let completed = Rc::new(Cell::new(0));
    let count = completed.clone();
    let hooks = crate::editing::InputHooks {
        paste_begin: Rc::new(|_| true),
        paste: Rc::new(|event| {
            crate::editing::PasteDecision::Insert(event.payload.text().to_owned())
        }),
        paste_complete: Rc::new(move |result| {
            let result = result.expect("successful paste");
            let surface = weak.upgrade().expect("surface");
            let view = surface.view(result.edit.identity.node).expect("view");
            let input = input_of(&view).expect("input");
            assert_eq!(input.count.get(), 1);
            assert!(
                surface
                    .host_events
                    .borrow()
                    .iter()
                    .any(|(node, kind, payload)| {
                        *node == view.id()
                            && *kind == EventKind::ChangeText
                            && *payload
                                == EventPayload::Text {
                                    text: "draft!".into(),
                                    count: 1,
                                }
                    })
            );
            window_focus(&surface, false);
            value(&view, "obsolete", 0);
            assert_eq!(input.model.borrow().text(), "draft!");
            count.set(count.get() + 1);
        }),
        ..inert_hooks()
    };
    input.model.borrow_mut().select(5, 5);
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(hooks.clone())
    ));
    session(&surface, &input, hooks).apply(
        &surface,
        &view,
        &input,
        InputPastePayload::Synthetic(nsh_core::PasteEvent::new("!")),
    );
    assert_eq!(completed.get(), 1);
    assert_eq!(input.count.get(), 1);
    assert_eq!(input.model.borrow().text(), "draft!");
    let events = surface.host_events.borrow();
    assert_eq!(
        events
            .iter()
            .filter(|(_, kind, _)| *kind == EventKind::ChangeText)
            .count(),
        1
    );
}

pub fn hooks_only_image_clipboard_enables_native_paste_menu() {
    let (surface, view, input) = field();
    assert!(!view.state().events.contains(EventKind::Paste));
    let item = gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
        gpui::ImageFormat::Png,
        vec![1, 2, 3],
    ));
    let paste_enabled = |input: &InputState| {
        menu_items(
            &surface,
            input,
            clipboard_can_paste(&view, input, Some(&item)),
        )
        .into_iter()
        .find_map(|item| match item {
            crate::chrome::LocalItem::Item {
                role: Some(nsh_core::desktop::MenuRole::Paste),
                enabled,
                ..
            } => Some(enabled),
            _ => None,
        })
        .expect("paste menu item")
    };
    assert!(!paste_enabled(&input));
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(inert_hooks())
    ));
    assert!(paste_enabled(&input));
    input.config.borrow_mut().secure = true;
    assert!(!paste_enabled(&input));
    input.config.borrow_mut().secure = false;
    input.config.borrow_mut().editable = false;
    assert!(!paste_enabled(&input));
}

pub fn stale_paste_and_callback_rebinding_cancel_without_native_edit() {
    for rebind in [false, true] {
        let (surface, view, input) = field();
        input.model.borrow_mut().set_text("draft");
        let cancelled = Rc::new(Cell::new(0));
        let count = cancelled.clone();
        let weak = Rc::downgrade(&surface);
        let hooks = crate::editing::InputHooks {
            paste_begin: Rc::new(|_| true),
            paste: Rc::new(move |event| {
                if rebind {
                    let surface = weak.upgrade().expect("surface");
                    assert!(set_hooks(
                        &surface,
                        &event.before.identity,
                        Some(inert_hooks())
                    ));
                }
                crate::editing::PasteDecision::Insert("wrong".into())
            }),
            paste_complete: Rc::new(move |result| {
                assert!(result.is_none());
                count.set(count.get() + 1);
            }),
            ..inert_hooks()
        };
        assert!(set_hooks(
            &surface,
            &identity(&surface, &input),
            Some(hooks.clone())
        ));
        let pending = session(&surface, &input, hooks);
        if !rebind {
            input.model.borrow_mut().select(0, 0);
            report_native_edit(&surface, &input);
        }
        pending.apply(
            &surface,
            &view,
            &input,
            InputPastePayload::Synthetic(nsh_core::PasteEvent::new("wrong")),
        );
        assert_eq!(cancelled.get(), 1);
        assert_eq!(input.model.borrow().text(), "draft");
    }
}

pub fn visual_history_context_uses_soft_wrap_and_current_viewport() {
    let (surface, _, input) = field();
    input.model.borrow_mut().set_text("abcdef");
    input.config.borrow_mut().multiline = true;
    input.size.set((30.0, 10.0));
    let line = |start, end, y| layout::Line {
        start,
        end,
        x: 0.0,
        y,
        height: 10.0,
        baseline: 8.0,
        text_height: 10.0,
        width: 30.0,
        stops: (start..=end)
            .map(|n| (n, (n - start) as f32 * 10.0))
            .collect(),
        glyphs: 0..0,
    };
    input.layout.replace(Some(Rc::new(Layout {
        lines: vec![line(0, 3, 0.0), line(3, 6, 10.0)],
        glyphs: vec![],
        size: 10.0,
        width: 30.0,
        height: 20.0,
    })));
    input.model.borrow_mut().select(3, 3);
    input.native_serial.set(7);
    let context = key_context(&input).expect("layout context");
    assert!(!context.first_visual_row);
    assert!(context.viewport_top);
    assert_eq!(context.native_edit_serial, 7);
    input.model.borrow_mut().select(1, 1);
    input.scroll.set(10.0);
    let context = key_context(&input).expect("scrolled context");
    assert!(context.first_visual_row);
    assert!(!context.viewport_top);
    input.scroll.set(0.0);
    input.model.borrow_mut().select(0, 1);
    assert!(!key_context(&input).expect("selection").selection_collapsed);
    input.model.borrow_mut().insert("new", None);
    after_edit(&surface, &surface.view(2).expect("view"), &input, true);
    assert!(input.layout.borrow().is_none());
}

pub fn marked_keys_never_reach_shortcuts_or_waiting_keys() {
    use nsh_core::keymap::{KeyBinding, Keymap, Keystroke, When};

    let (surface, view, input) = field();
    let bindings = [
        ("down", true),
        ("up", true),
        ("enter", false),
        ("escape", false),
        ("tab", false),
        ("shift+tab", false),
        ("mod+k", false),
    ];
    let table = std::sync::Arc::new(Keymap::new(
        bindings
            .iter()
            .enumerate()
            .map(|(id, (key, repeat))| {
                KeyBinding::new(
                    id as u32 + 1,
                    Keystroke::parse_with(key, cfg!(target_os = "macos")).expect("keystroke"),
                    When::Always,
                    *repeat,
                )
            })
            .collect(),
    ));
    surface
        .mounter
        .borrow_mut()
        .apply(&surface, &nsh_core::MountItem::Keymap(table.clone()));
    ime::replace_and_mark_text_in_range(&surface, None, "候选", None);
    assert!(native_composing(&surface));
    let captured = identity(&surface, &input);
    let marked = input.model.borrow().display().into_owned();
    surface.host_events.borrow_mut().clear();
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    surface.shortcuts.borrow_mut().push(Box::new(move |_| {
        counter.set(counter.get() + 1);
        true
    }));
    arm(&surface);
    FOCUS.with(|focus| {
        let mut focus = focus.borrow_mut();
        let pending = &mut focus.get_mut(&surface.key()).expect("focus").pending;
        pending.push_back(Held::Text("kept".into()));
        pending.push_back(Held::Key(
            KeyEvent {
                vk: VK_RETURN,
                down: true,
                repeat: false,
                modifiers: Default::default(),
            },
            None,
        ));
        pending.push_back(Held::Key(
            KeyEvent {
                vk: VK_RETURN,
                down: true,
                repeat: false,
                modifiers: Default::default(),
            },
            Some(captured.clone()),
        ));
    });
    for vk in [
        VK_RETURN, VK_ESCAPE, VK_UP, VK_DOWN, VK_TAB, VK_LEFT, VK_RIGHT,
    ] {
        for meta in [false, true] {
            for shift in [false, true] {
                for repeat in [false, true] {
                    let event = KeyEvent {
                        vk,
                        down: true,
                        repeat,
                        modifiers: Modifiers {
                            meta,
                            shift,
                            ..Default::default()
                        },
                    };
                    assert!(!crate::input::key(&surface, event));
                    assert!(!crate::input::key_now(&surface, event));
                    assert!(surface.host_events.borrow().is_empty());
                    assert_eq!(focused(&surface).as_ref(), Some(&view));
                    assert!(captured.same_focus(&identity(&surface, &input)));
                    assert_eq!(input.model.borrow().display().as_ref(), marked);
                }
            }
        }
    }
    assert_eq!(calls.get(), 0);
    FOCUS.with(|focus| {
        let focus = focus.borrow();
        let pending = &focus[&surface.key()].pending;
        assert_eq!(pending.len(), 1);
        assert!(matches!(pending.front(), Some(Held::Text(text)) if text == "kept"));
    });
    let shortcut = surface.shortcuts.borrow_mut().pop().expect("shortcut");
    for binding in &table.bindings()[..6] {
        for repeat in [false, true] {
            let event = KeyEvent {
                vk: binding.keystroke.key.virtual_key().expect("virtual key"),
                down: true,
                repeat,
                modifiers: Modifiers {
                    shift: binding.keystroke.modifiers.shift,
                    ..Default::default()
                },
            };
            assert!(!crate::input::key(&surface, event));
            assert!(!crate::input::key_now(&surface, event));
            assert!(surface.host_events.borrow().is_empty());
            assert_eq!(focused(&surface).as_ref(), Some(&view));
            assert!(captured.same_focus(&identity(&surface, &input)));
            assert_eq!(input.model.borrow().display().as_ref(), marked);
            FOCUS.with(|focus| {
                let focus = focus.borrow();
                let pending = &focus[&surface.key()].pending;
                assert_eq!(pending.len(), 1);
                assert!(matches!(pending.front(), Some(Held::Text(text)) if text == "kept"));
            });
        }
    }
    assert!(matches!(
        crate::input::composition_key(&surface, None),
        Some(crate::input::CompositionKeyRoute::Native)
    ));
    for route in [
        crate::input::key,
        crate::input::key_now,
        crate::input::component_key,
    ] {
        for vk in [
            VK_RETURN, VK_ESCAPE, VK_UP, VK_DOWN, VK_TAB, VK_LEFT, VK_RIGHT, VK_HOME, VK_END,
            VK_PRIOR, VK_NEXT, VK_BACK, VK_DELETE, VK_SPACE, 0x41, 0x43, 0x56, 0x58, 0x59, 0x5A,
            0x4B,
        ] {
            for modifiers in [
                Modifiers::default(),
                Modifiers {
                    shift: true,
                    ..Default::default()
                },
                Modifiers {
                    ctrl: true,
                    alt: true,
                    ..Default::default()
                },
                Modifiers {
                    alt: true,
                    ..Default::default()
                },
                command(),
                Modifiers {
                    shift: true,
                    ..command()
                },
            ] {
                if vk == 0x4B && !modifiers.alt && (modifiers.ctrl || modifiers.meta) {
                    continue;
                }
                for repeat in [false, true] {
                    assert!(!route(
                        &surface,
                        KeyEvent {
                            vk,
                            down: true,
                            repeat,
                            modifiers,
                        }
                    ));
                    assert!(surface.host_events.borrow().is_empty());
                    assert!(native_composing(&surface));
                    assert_eq!(focused(&surface).as_ref(), Some(&view));
                    assert!(captured.same_focus(&identity(&surface, &input)));
                    assert_eq!(input.model.borrow().display().as_ref(), marked);
                }
            }
        }
        let binding = &table.bindings()[6];
        let event = KeyEvent {
            vk: binding.keystroke.key.virtual_key().expect("virtual key"),
            down: true,
            repeat: false,
            modifiers: command(),
        };
        for _ in 0..2 {
            assert!(route(&surface, event));
            assert_eq!(
                *surface.host_events.borrow(),
                vec![(
                    nsh_core::ROOT_ID,
                    EventKind::KeyCommand,
                    EventPayload::Value(nsh_core::PayloadValue::Number(binding.id as f64)),
                )]
            );
            assert!(route(&surface, event));
            assert!(route(
                &surface,
                KeyEvent {
                    repeat: true,
                    ..event
                }
            ));
            assert_eq!(surface.host_events.borrow().len(), 1);
            assert!(!route(
                &surface,
                KeyEvent {
                    down: false,
                    ..event
                }
            ));
            assert_eq!(surface.host_events.borrow().len(), 1);
            surface.host_events.borrow_mut().clear();
            assert!(native_composing(&surface));
            assert_eq!(focused(&surface).as_ref(), Some(&view));
            assert!(captured.same_focus(&identity(&surface, &input)));
            assert_eq!(input.model.borrow().display().as_ref(), marked);
            FOCUS.with(|focus| {
                let focus = focus.borrow();
                let pending = &focus[&surface.key()].pending;
                assert_eq!(pending.len(), 1);
                assert!(matches!(pending.front(), Some(Held::Text(text)) if text == "kept"));
            });
        }
        assert!(!route(&surface, KeyEvent { vk: 0x4C, ..event }));
        assert!(!route(
            &surface,
            KeyEvent {
                vk: 0x4C,
                down: false,
                ..event
            }
        ));
        assert!(surface.host_events.borrow().is_empty());
        assert!(native_composing(&surface));
        assert_eq!(focused(&surface).as_ref(), Some(&view));
        assert!(captured.same_focus(&identity(&surface, &input)));
        assert_eq!(input.model.borrow().display().as_ref(), marked);
        FOCUS.with(|focus| {
            let focus = focus.borrow();
            let pending = &focus[&surface.key()].pending;
            assert_eq!(pending.len(), 1);
            assert!(matches!(pending.front(), Some(Held::Text(text)) if text == "kept"));
        });
    }
    let global_calls = Rc::new(Cell::new(0));
    let global_counter = global_calls.clone();
    surface.shortcuts.borrow_mut().push(Box::new(move |_| {
        global_counter.set(global_counter.get() + 1);
        true
    }));
    for (index, route) in [crate::input::key, crate::input::key_now]
        .into_iter()
        .enumerate()
    {
        assert!(route(
            &surface,
            KeyEvent {
                vk: 0x4B,
                down: true,
                repeat: false,
                modifiers: command(),
            }
        ));
        assert_eq!(global_calls.get(), index + 1);
        assert!(surface.host_events.borrow().is_empty());
        assert!(native_composing(&surface));
        assert_eq!(input.model.borrow().display().as_ref(), marked);
        assert_eq!(focused(&surface).as_ref(), Some(&view));
        assert!(captured.same_focus(&identity(&surface, &input)));
        FOCUS.with(|focus| {
            let focus = focus.borrow();
            let pending = &focus[&surface.key()].pending;
            assert_eq!(pending.len(), 1);
            assert!(matches!(pending.front(), Some(Held::Text(text)) if text == "kept"));
        });
    }
    drop(
        surface
            .shortcuts
            .borrow_mut()
            .pop()
            .expect("global shortcut"),
    );
    assert_eq!(calls.get(), 0);
    flush_pending(&surface);
    assert!(surface.host_events.borrow().is_empty());
    assert_eq!(input.model.borrow().display().as_ref(), marked);
    ime::unmark_text(&surface);
    assert!(!native_composing(&surface));
    surface.host_events.borrow_mut().clear();
    flush_pending(&surface);
    assert_eq!(input.model.borrow().text(), "候选kept");
    assert!(
        !surface
            .host_events
            .borrow()
            .iter()
            .any(|(_, kind, _)| *kind == EventKind::KeyCommand)
    );
    FOCUS.with(|focus| assert!(focus.borrow()[&surface.key()].pending.is_empty()));
    assert_eq!(calls.get(), 0);
    for route in [crate::input::key, crate::input::key_now] {
        for binding in table.bindings() {
            let modifiers = binding.keystroke.modifiers;
            let event = KeyEvent {
                vk: binding.keystroke.key.virtual_key().expect("virtual key"),
                down: true,
                repeat: false,
                modifiers: Modifiers {
                    shift: modifiers.shift,
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    meta: modifiers.cmd,
                },
            };
            surface.host_events.borrow_mut().clear();
            assert!(route(&surface, event));
            assert_eq!(
                *surface.host_events.borrow(),
                vec![(
                    nsh_core::ROOT_ID,
                    EventKind::KeyCommand,
                    EventPayload::Value(nsh_core::PayloadValue::Number(binding.id as f64)),
                )]
            );
            assert!(route(
                &surface,
                KeyEvent {
                    repeat: true,
                    ..event
                }
            ));
            assert_eq!(
                surface.host_events.borrow().len(),
                if binding.repeat { 2 } else { 1 }
            );
            assert!(!route(
                &surface,
                KeyEvent {
                    down: false,
                    ..event
                }
            ));
            disarm(&surface);
            assert_eq!(focused(&surface).as_ref(), Some(&view));
            assert!(captured.same_focus(&identity(&surface, &input)));
            assert_eq!(input.model.borrow().text(), "候选kept");
        }
    }
    surface.shortcuts.borrow_mut().push(shortcut);
    surface.host_events.borrow_mut().clear();
    assert!(crate::input::key(
        &surface,
        KeyEvent {
            vk: 0x4B,
            down: true,
            repeat: false,
            modifiers: Modifiers {
                meta: true,
                ..Default::default()
            },
        }
    ));
    assert_eq!(calls.get(), 1);
    assert!(surface.host_events.borrow().is_empty());
    assert_eq!(focused(&surface).as_ref(), Some(&view));
}

pub fn edit_transaction_preserves_reversed_utf16_and_secure_input_is_not_observed() {
    let (surface, _, input) = field();
    input.model.borrow_mut().set_text("a😀z");
    input.model.borrow_mut().select(5, 1);
    let edits = Rc::new(RefCell::new(Vec::new()));
    let sink = edits.clone();
    let captured = identity(&surface, &input);
    assert!(set_hooks(
        &surface,
        &captured,
        Some(crate::editing::InputHooks {
            key: Rc::new(|_, _| false),
            focus: Rc::new(|_, _| {}),
            edit: Rc::new(move |edit| sink.borrow_mut().push(edit)),
            ..inert_hooks()
        })
    ));
    {
        let edits = edits.borrow();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].text, "a😀z");
        assert_eq!((edits[0].anchor_utf16, edits[0].head_utf16), (3, 1));
        assert_eq!(edits[0].native_edit_serial, 1);
    }
    ime::replace_and_mark_text_in_range(&surface, Some(1..1), "候😀", Some(1..1));
    {
        let edits = edits.borrow();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[1].text, "a候😀😀z");
        assert_eq!((edits[1].anchor_utf16, edits[1].head_utf16), (2, 2));
        assert!(edits[1].composing);
        assert_eq!(edits[1].native_edit_serial, 2);
        assert_eq!(
            edits[1].mutation,
            Some(crate::editing::InputMutation {
                range_utf16: (1, 1),
                inserted: "候😀".into(),
            })
        );
    }
    input.config.borrow_mut().secure = true;
    input.model.borrow_mut().set_text("secret");
    report_native_edit(&surface, &input);
    assert_eq!(edits.borrow().len(), 2);
    assert!(input_identity(&surface, 2).is_none());
    assert!(!set_hooks(&surface, &captured, None));
}

pub fn ownerless_waiting_key_never_reaches_new_field_policy() {
    let (surface, _, input) = field();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(crate::editing::InputHooks {
            key: Rc::new(move |_, _| {
                count.set(count.get() + 1);
                true
            }),
            edit: Rc::new(|_| {}),
            focus: Rc::new(|_, _| {}),
            ..inert_hooks()
        })
    ));
    arm(&surface);
    FOCUS.with(|focus| {
        let mut focus = focus.borrow_mut();
        let pending = &mut focus.get_mut(&surface.key()).expect("focus").pending;
        pending.push_back(Held::Text("preserved".into()));
        pending.push_back(Held::Key(
            KeyEvent {
                vk: VK_RETURN,
                down: true,
                repeat: false,
                modifiers: Modifiers {
                    meta: true,
                    ..Default::default()
                },
            },
            None,
        ));
    });
    flush_pending(&surface);
    assert_eq!(calls.get(), 0);
    assert_eq!(input.model.borrow().text(), "preserved");
}

pub fn retirement_and_replacement_callbacks_cannot_rebind() {
    let (surface, _, input) = field();
    let attempts = Rc::new(RefCell::new(Vec::new()));
    let sink = attempts.clone();
    let weak = Rc::downgrade(&surface);
    let hooks = crate::editing::InputHooks {
        key: Rc::new(|_, _| false),
        edit: Rc::new(|_| {}),
        focus: Rc::new(move |revoked, focused| {
            if !focused {
                let surface = weak.upgrade().expect("surface");
                let current = input_identity(&surface, revoked.node).expect("attached input");
                let replacement = crate::editing::InputHooks {
                    key: Rc::new(|_, _| true),
                    edit: Rc::new(|_| {}),
                    focus: Rc::new(|_, _| {}),
                    ..inert_hooks()
                };
                sink.borrow_mut()
                    .push(set_hooks(&surface, &current, Some(replacement)));
            }
        }),
        ..inert_hooks()
    };
    let captured = identity(&surface, &input);
    assert!(set_hooks(&surface, &captured, Some(hooks.clone())));
    assert!(set_hooks(&surface, &captured, Some(hooks)));
    assert_eq!(*attempts.borrow(), vec![false]);
    window_focus(&surface, false);
    assert_eq!(*attempts.borrow(), vec![false, false]);
    assert!(input.hooks.borrow().is_none());
    assert!(!captured.same_focus(&identity(&surface, &input)));
    window_focus(&surface, true);
    assert!(!guarded_key(
        &surface,
        &KeyEvent {
            vk: VK_RETURN,
            down: true,
            repeat: false,
            modifiers: Modifiers {
                meta: true,
                ..Default::default()
            },
        }
    ));
}

pub fn host_focus_gain_notifies_current_registration_once_and_loss_retires_it() {
    let (surface, _, input) = field();
    FOCUS.with(|focus| focus.borrow_mut().get_mut(&surface.key()).unwrap().inactive = true);
    input.active.set(false);
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    let hooks = crate::editing::InputHooks {
        focus: Rc::new(move |identity, focused| {
            sink.borrow_mut().push((identity, focused));
        }),
        ..inert_hooks()
    };
    let captured = identity(&surface, &input);
    assert!(set_hooks(&surface, &captured, Some(hooks.clone())));
    assert_eq!(events.borrow().len(), 1);
    assert!(!events.borrow()[0].1);
    assert!(window_focus(&surface, true));
    assert!(input.active.get());
    assert_eq!(events.borrow().len(), 2);
    assert!(events.borrow()[1].0.same_focus(&captured));
    assert!(events.borrow()[1].1);
    assert!(!window_focus(&surface, true));
    assert_eq!(events.borrow().len(), 2);
    assert!(window_focus(&surface, false));
    assert!(!input.active.get());
    assert_eq!(events.borrow().len(), 3);
    assert!(!events.borrow()[2].1);
    assert!(input.hooks.borrow().is_none());
    assert!(window_focus(&surface, true));
    assert_eq!(events.borrow().len(), 3);
    assert!(!set_hooks(&surface, &captured, Some(hooks)));
}

pub fn focus_loss_and_recycled_node_reject_old_policy() {
    let (surface, view, input) = field();
    let captured = identity(&surface, &input);
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let hooks = crate::editing::InputHooks {
        key: Rc::new(move |key, _| {
            count.set(count.get() + 1);
            key.meta
        }),
        edit: Rc::new(|_| {}),
        focus: Rc::new(|_, _| {}),
        ..inert_hooks()
    };
    assert!(set_hooks(&surface, &captured, Some(hooks.clone())));
    assert!(crate::input::key(
        &surface,
        KeyEvent {
            vk: VK_RETURN,
            down: true,
            repeat: false,
            modifiers: Modifiers {
                meta: true,
                ..Default::default()
            },
        }
    ));
    assert_eq!(calls.get(), 1);
    window_focus(&surface, false);
    assert!(!set_hooks(&surface, &captured, Some(hooks.clone())));
    view.unparent();
    surface.mounter.borrow_mut().apply(
        &surface,
        &nsh_core::MountItem::Create {
            id: 2,
            kind: nsh_core::ViewKind::Input,
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
    assert!(!set_hooks(&surface, &captured, Some(hooks)));
    assert_eq!(calls.get(), 1);
}

fn observe(surface: &Surface, input: &InputState) -> Rc<RefCell<Vec<crate::editing::InputEdit>>> {
    let edits = Rc::new(RefCell::new(Vec::new()));
    let sink = edits.clone();
    assert!(set_hooks(
        surface,
        &identity(surface, input),
        Some(crate::editing::InputHooks {
            edit: Rc::new(move |edit| sink.borrow_mut().push(edit)),
            ..inert_hooks()
        })
    ));
    edits
}

fn expect_edit(
    edits: &Rc<RefCell<Vec<crate::editing::InputEdit>>>,
    serial: u64,
    text: &str,
    selection: (u32, u32),
    composing: bool,
    mutation: Option<((u32, u32), &str)>,
) {
    let edits = edits.borrow();
    let edit = edits.last().expect("observed edit");
    assert_eq!(edits.len(), serial as usize);
    assert_eq!(edit.native_edit_serial, serial);
    assert_eq!(edit.text, text);
    assert_eq!((edit.anchor_utf16, edit.head_utf16), selection);
    assert_eq!(edit.composing, composing);
    assert_eq!(
        edit.mutation,
        mutation.map(|(range_utf16, inserted)| crate::editing::InputMutation {
            range_utf16,
            inserted: inserted.into(),
        })
    );
}

pub fn native_replacements_preserve_operation_ranges_and_same_text_serials() {
    let (surface, _, input) = field();
    input.model.borrow_mut().set_text("a😀中z");
    let edits = observe(&surface, &input);
    expect_edit(&edits, 1, "a😀中z", (5, 5), false, None);
    ime::replace_text_in_range(&surface, Some(1..4), "😀中文");
    expect_edit(
        &edits,
        2,
        "a😀中文z",
        (5, 5),
        false,
        Some(((1, 4), "😀中文")),
    );
    ime::replace_text_in_range(&surface, Some(1..5), "😀中文");
    expect_edit(
        &edits,
        3,
        "a😀中文z",
        (5, 5),
        false,
        Some(((1, 5), "😀中文")),
    );
    assert_eq!(input.count.get(), 2);
}

pub fn native_selection_and_reregistration_have_no_mutation() {
    let (surface, view, input) = field();
    input.model.borrow_mut().set_text("a😀中z");
    let edits = observe(&surface, &input);
    ime::set_selected_text_range(&surface, Range { start: 4, end: 1 });
    expect_edit(&edits, 2, "a😀中z", (4, 1), false, None);
    report_selection(&surface, &view, &input);
    assert_eq!(edits.borrow().len(), 2);
    let hooks = input.hooks.borrow().clone();
    assert!(set_hooks(&surface, &identity(&surface, &input), hooks));
    expect_edit(&edits, 3, "a😀中z", (4, 1), false, None);
    assert!(
        native_snapshot(&surface, &input)
            .expect("snapshot")
            .mutation
            .is_none()
    );
    assert_eq!(input.count.get(), 0);
}

pub fn native_marked_update_commit_cancel_and_reversed_surrogates_are_exact() {
    let (surface, _, input) = field();
    input.model.borrow_mut().set_text("a😀中z");
    let edits = observe(&surface, &input);
    ime::replace_and_mark_text_in_range(
        &surface,
        Some(Range { start: 4, end: 1 }),
        "候😀",
        Some(3..3),
    );
    expect_edit(&edits, 2, "a候😀z", (4, 4), true, Some(((1, 4), "候😀")));
    ime::replace_and_mark_text_in_range(&surface, None, "候😀", Some(3..3));
    expect_edit(&edits, 3, "a候😀z", (4, 4), true, Some(((1, 4), "候😀")));
    ime::replace_text_in_range(&surface, None, "候😀");
    expect_edit(&edits, 4, "a候😀z", (4, 4), false, None);
    ime::replace_and_mark_text_in_range(&surface, Some(1..4), "中文", Some(2..2));
    expect_edit(&edits, 5, "a中文z", (3, 3), true, Some(((1, 4), "中文")));
    ime::replace_and_mark_text_in_range(&surface, None, "", None);
    expect_edit(&edits, 6, "az", (1, 1), false, Some(((1, 3), "")));
    assert_eq!(input.model.borrow().text(), "az");
}

pub fn native_partial_and_overlapping_composition_ranges_preserve_untouched_text() {
    let (surface, _, input) = field();
    input.model.borrow_mut().set_text("a😀中z");
    let edits = observe(&surface, &input);
    ime::replace_and_mark_text_in_range(&surface, Some(1..3), "候选", Some(2..2));
    expect_edit(&edits, 2, "a候选中z", (3, 3), true, Some(((1, 3), "候选")));
    ime::replace_and_mark_text_in_range(&surface, Some(2..4), "中文", Some(2..2));
    expect_edit(&edits, 3, "a候中文z", (4, 4), true, Some(((2, 4), "中文")));
    ime::replace_text_in_range(&surface, Some(2..3), "新");
    expect_edit(&edits, 4, "a候新文z", (3, 3), false, Some(((2, 3), "新")));
    ime::replace_and_mark_text_in_range(&surface, Some(1..4), "候新文", Some(1..1));
    expect_edit(
        &edits,
        5,
        "a候新文z",
        (2, 2),
        true,
        Some(((1, 4), "候新文")),
    );
    ime::unmark_text(&surface);
    expect_edit(&edits, 6, "a候新文z", (4, 4), false, None);
}

pub fn native_deletes_cut_and_accessibility_report_real_ranges() {
    let (surface, view, input) = field();
    input.model.borrow_mut().set_text("a😀中z");
    input.model.borrow_mut().select(5, 5);
    let edits = observe(&surface, &input);
    let key = |vk| KeyEvent {
        vk,
        down: true,
        repeat: false,
        modifiers: Default::default(),
    };
    assert!(crate::input::key(&surface, key(VK_BACK)));
    expect_edit(&edits, 2, "a中z", (1, 1), false, Some(((1, 3), "")));
    assert!(crate::input::key(&surface, key(VK_DELETE)));
    expect_edit(&edits, 3, "az", (1, 1), false, Some(((1, 2), "")));
    ime::set_selected_text_range(&surface, Range { start: 2, end: 0 });
    expect_edit(&edits, 4, "az", (2, 0), false, None);
    assert!(cut(&surface, &view, &input, false));
    expect_edit(&edits, 5, "", (0, 0), false, Some(((0, 2), "")));
    assert!(set_accessible_value(&surface, &view, "😀中"));
    expect_edit(&edits, 6, "😀中", (3, 3), false, Some(((0, 0), "😀中")));
    assert!(set_accessible_value(&surface, &view, "😀中"));
    expect_edit(&edits, 7, "😀中", (3, 3), false, Some(((0, 3), "😀中")));
    assert!(set_accessible_value(&surface, &view, ""));
    expect_edit(&edits, 8, "", (0, 0), false, Some(((0, 3), "")));
}

pub fn unknown_native_mutations_retire_without_diff_or_fallback() {
    let (surface, view, input) = field();
    input.model.borrow_mut().set_text("same prefix old");
    let edits = observe(&surface, &input);
    input.model.borrow_mut().set_text("same prefix new");
    after_edit(&surface, &view, &input, true);
    assert_eq!(edits.borrow().len(), 1);
    assert_eq!(input.native_serial.get(), 1);
    assert!(input.hooks.borrow().is_none());
    assert_eq!(input.model.borrow().text(), "same prefix new");
}

pub fn native_clipping_and_explicit_same_text_composition_replacement_are_exact() {
    let (surface, _, input) = field();
    input.model.borrow_mut().set_text("az");
    input.config.borrow_mut().max_length = Some(4);
    let edits = observe(&surface, &input);
    ime::replace_and_mark_text_in_range(&surface, Some(1..1), "😀中", Some(3..3));
    expect_edit(&edits, 2, "a😀中z", (4, 4), true, Some(((1, 1), "😀中")));
    ime::unmark_text(&surface);
    expect_edit(&edits, 3, "a😀z", (3, 3), false, Some(((1, 4), "😀")));
    ime::replace_and_mark_text_in_range(&surface, Some(1..3), "😀", Some(2..2));
    expect_edit(&edits, 4, "a😀z", (3, 3), true, Some(((1, 3), "😀")));
    ime::replace_text_in_range(&surface, Some(1..3), "😀");
    expect_edit(&edits, 5, "a😀z", (3, 3), false, Some(((1, 3), "😀")));
}

pub fn native_same_text_paste_is_explicit_and_secret_snapshots_are_absent() {
    let (surface, view, input) = field();
    input.model.borrow_mut().set_text("a😀z");
    input.model.borrow_mut().select(5, 1);
    let completed = Rc::new(RefCell::new(Vec::new()));
    let sink = completed.clone();
    let hooks = crate::editing::InputHooks {
        paste_begin: Rc::new(|_| true),
        paste: Rc::new(|event| {
            crate::editing::PasteDecision::Insert(event.payload.text().to_owned())
        }),
        paste_complete: Rc::new(move |result| sink.borrow_mut().push(result)),
        ..inert_hooks()
    };
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(hooks.clone())
    ));
    session(&surface, &input, hooks).apply(
        &surface,
        &view,
        &input,
        InputPastePayload::Synthetic(nsh_core::PasteEvent::new("😀")),
    );
    let completed = completed.borrow();
    let edit = &completed[0].as_ref().expect("same text paste").edit;
    assert_eq!(edit.text, "a😀z");
    assert_eq!(edit.native_edit_serial, 2);
    assert_eq!((edit.anchor_utf16, edit.head_utf16), (3, 3));
    assert_eq!(
        edit.mutation,
        Some(crate::editing::InputMutation {
            range_utf16: (1, 3),
            inserted: "😀".into(),
        })
    );
    assert_eq!(input.count.get(), 1);
    input.config.borrow_mut().secure = true;
    assert!(native_snapshot(&surface, &input).is_none());
}

fn undo_menu_action(surface: &Surface, input: &InputState) -> Rc<dyn Fn(&Rc<Surface>)> {
    menu_items(surface, input, false)
        .into_iter()
        .find_map(|item| match item {
            crate::chrome::LocalItem::Item {
                role: Some(nsh_core::desktop::MenuRole::Undo),
                enabled,
                action,
                ..
            } => {
                assert!(enabled);
                Some(action)
            }
            _ => None,
        })
        .expect("actual native Undo menu action")
}

pub fn native_menu_undo_calls_policy_once_and_unhandled_history_fails_closed() {
    for accepted in [true, false] {
        let (surface, _, input) = field();
        input.model.borrow_mut().insert("draft", None);
        assert!(input.model.borrow().can_undo());
        let captured = identity(&surface, &input);
        let expected = captured.clone();
        let calls = Rc::new(Cell::new(0));
        let counter = calls.clone();
        let edits = Rc::new(RefCell::new(Vec::new()));
        let sink = edits.clone();
        let focus = Rc::new(RefCell::new(Vec::new()));
        let states = focus.clone();
        assert!(set_hooks(
            &surface,
            &captured,
            Some(crate::editing::InputHooks {
                key: Rc::new(move |key, current| {
                    assert!(expected.same_focus(&current));
                    assert_eq!(key.virtual_key, 0x5A);
                    assert!(key.down);
                    assert!(!key.repeat && !key.shift && !key.alt);
                    assert_eq!(key.meta, cfg!(target_os = "macos"));
                    assert_eq!(key.control, !cfg!(target_os = "macos"));
                    counter.set(counter.get() + 1);
                    accepted
                }),
                edit: Rc::new(move |edit| sink.borrow_mut().push(edit)),
                focus: Rc::new(move |_, active| states.borrow_mut().push(active)),
                ..inert_hooks()
            })
        ));
        let registration = input.hook_registration.borrow().clone();
        let action = undo_menu_action(&surface, &input);
        let shortcuts = Rc::new(Cell::new(0));
        let count = shortcuts.clone();
        surface.shortcuts.borrow_mut().push(Box::new(move |_| {
            count.set(count.get() + 1);
            true
        }));
        arm(&surface);
        action(&surface);
        assert_eq!(calls.get(), 1);
        assert_eq!(shortcuts.get(), 0);
        assert_eq!(input.model.borrow().text(), "draft");
        assert!(input.model.borrow().can_undo());
        assert_eq!(input.count.get(), 0);
        expect_edit(&edits, 1, "draft", (5, 5), false, None);
        FOCUS.with(|focus| assert!(focus.borrow()[&surface.key()].pending.is_empty()));
        if accepted {
            assert!(input.hooks.borrow().is_some());
            assert!(captured.same_focus(&identity(&surface, &input)));
            assert!(Rc::ptr_eq(&registration, &input.hook_registration.borrow()));
            assert_eq!(*focus.borrow(), vec![true]);
        } else {
            assert!(input.hooks.borrow().is_none());
            assert_eq!(input.focus_epoch.get(), captured.focus_epoch + 1);
            assert_eq!(*focus.borrow(), vec![true, false]);
        }
    }
}

pub fn native_menu_history_survives_empty_model_history_and_redo_uses_policy_once() {
    for redo in [false, true] {
        for accepted in [false, true] {
            let (surface, _, input) = field();
            input.model.borrow_mut().set_text("remounted authority");
            assert!(!input.model.borrow().can_undo());
            let captured = identity(&surface, &input);
            let calls = Rc::new(Cell::new(0));
            let count = calls.clone();
            let edits = Rc::new(RefCell::new(Vec::new()));
            let sink = edits.clone();
            assert!(set_hooks(
                &surface,
                &captured,
                Some(crate::editing::InputHooks {
                    key: Rc::new(move |key, _| {
                        count.set(count.get() + 1);
                        assert_eq!(
                            key.virtual_key,
                            if redo && !cfg!(target_os = "macos") {
                                0x59
                            } else {
                                0x5a
                            }
                        );
                        assert_eq!(key.shift, redo && cfg!(target_os = "macos"));
                        assert_eq!(key.meta, cfg!(target_os = "macos"));
                        assert_eq!(key.control, !cfg!(target_os = "macos"));
                        accepted
                    }),
                    edit: Rc::new(move |edit| sink.borrow_mut().push(edit)),
                    ..inert_hooks()
                })
            ));
            let role = if redo {
                nsh_core::desktop::MenuRole::Redo
            } else {
                nsh_core::desktop::MenuRole::Undo
            };
            let action = menu_items(&surface, &input, false)
                .into_iter()
                .find_map(|item| match item {
                    crate::chrome::LocalItem::Item {
                        role: Some(current),
                        enabled,
                        action,
                        ..
                    } if current == role => {
                        assert!(enabled);
                        Some(action)
                    }
                    _ => None,
                })
                .expect("actual history menu action");
            action(&surface);
            assert_eq!(calls.get(), 1);
            assert_eq!(input.model.borrow().text(), "remounted authority");
            assert_eq!(input.native_serial.get(), 1);
            assert_eq!(input.count.get(), 0);
            assert_eq!(edits.borrow().len(), 1);
            assert!(edits.borrow()[0].mutation.is_none());
            assert_eq!(input.hooks.borrow().is_some(), accepted);
            assert_eq!(captured.same_focus(&identity(&surface, &input)), accepted);
        }
    }
}

pub fn native_menu_redo_rejects_composition_stale_registration_and_secure_history() {
    let (surface, _, input) = field();
    input.model.borrow_mut().set_text("remounted");
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let hooks = crate::editing::InputHooks {
        key: Rc::new(move |_, _| {
            count.set(count.get() + 1);
            true
        }),
        ..inert_hooks()
    };
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(hooks.clone())
    ));
    let redo = || {
        menu_items(&surface, &input, false)
            .into_iter()
            .find_map(|item| match item {
                crate::chrome::LocalItem::Item {
                    role: Some(nsh_core::desktop::MenuRole::Redo),
                    enabled,
                    action,
                    ..
                } => {
                    assert!(enabled);
                    Some(action)
                }
                _ => None,
            })
            .expect("actual Redo action")
    };
    let composing_action = redo();
    ime::replace_and_mark_text_in_range(&surface, None, "候😀", None);
    let marked = input.model.borrow().display().into_owned();
    let serial = input.native_serial.get();
    composing_action(&surface);
    assert_eq!(calls.get(), 0);
    assert_eq!(input.model.borrow().display(), marked);
    assert_eq!(input.native_serial.get(), serial);
    assert!(input.hooks.borrow().is_some());
    ime::unmark_text(&surface);
    let stale = redo();
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(hooks)
    ));
    stale(&surface);
    assert_eq!(calls.get(), 0);
    redo()(&surface);
    assert_eq!(calls.get(), 1);
    input.config.borrow_mut().secure = true;
    for item in menu_items(&surface, &input, false) {
        if let crate::chrome::LocalItem::Item {
            role: Some(nsh_core::desktop::MenuRole::Redo),
            enabled,
            ..
        } = item
        {
            assert!(!enabled);
        }
    }
}

pub fn native_menu_undo_excludes_composition_without_queueing_policy() {
    let (surface, _, input) = field();
    input.model.borrow_mut().insert("draft", None);
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    let edits = Rc::new(RefCell::new(Vec::new()));
    let sink = edits.clone();
    let captured = identity(&surface, &input);
    assert!(set_hooks(
        &surface,
        &captured,
        Some(crate::editing::InputHooks {
            key: Rc::new(move |_, _| {
                counter.set(counter.get() + 1);
                true
            }),
            edit: Rc::new(move |edit| sink.borrow_mut().push(edit)),
            ..inert_hooks()
        })
    ));
    let action = undo_menu_action(&surface, &input);
    ime::replace_and_mark_text_in_range(&surface, None, "候😀", Some(3..3));
    expect_edit(&edits, 2, "draft候😀", (8, 8), true, Some(((5, 5), "候😀")));
    arm(&surface);
    FOCUS.with(|focus| {
        let mut focus = focus.borrow_mut();
        let pending = &mut focus.get_mut(&surface.key()).expect("focus").pending;
        pending.push_back(Held::Text("kept".into()));
        pending.push_back(Held::Key(
            KeyEvent {
                vk: 0x5A,
                down: true,
                repeat: false,
                modifiers: command(),
            },
            Some(captured.clone()),
        ));
    });
    action(&surface);
    assert_eq!(calls.get(), 0);
    assert!(input.hooks.borrow().is_some());
    assert!(captured.same_focus(&identity(&surface, &input)));
    assert_eq!(input.count.get(), 0);
    expect_edit(&edits, 2, "draft候😀", (8, 8), true, Some(((5, 5), "候😀")));
    FOCUS.with(|focus| {
        let focus = focus.borrow();
        let pending = &focus[&surface.key()].pending;
        assert_eq!(pending.len(), 1);
        assert!(matches!(pending.front(), Some(Held::Text(text)) if text == "kept"));
    });
    ime::unmark_text(&surface);
    action(&surface);
    assert_eq!(calls.get(), 1);
    assert!(input.hooks.borrow().is_some());
    assert!(captured.same_focus(&identity(&surface, &input)));
    expect_edit(&edits, 3, "draft候😀", (8, 8), false, None);
}

pub fn native_menu_undo_rejects_stale_focus_mount_registration_and_window() {
    for stale in 0..7 {
        let (surface, view, input) = field();
        input.model.borrow_mut().insert("draft", None);
        let calls = Rc::new(Cell::new(0));
        let counter = calls.clone();
        let hooks = crate::editing::InputHooks {
            key: Rc::new(move |_, _| {
                counter.set(counter.get() + 1);
                true
            }),
            ..inert_hooks()
        };
        assert!(set_hooks(
            &surface,
            &identity(&surface, &input),
            Some(hooks.clone())
        ));
        let action = undo_menu_action(&surface, &input);
        let mut current = input.clone();
        match stale {
            0 => {
                window_focus(&surface, false);
            }
            1 => window_visibility(&surface, false),
            2 => {
                set_focus(&surface, None);
                set_focus(&surface, Some(&view));
                assert!(set_hooks(
                    &surface,
                    &identity(&surface, &input),
                    Some(hooks)
                ));
            }
            3 => {
                assert!(set_hooks(
                    &surface,
                    &identity(&surface, &input),
                    Some(hooks)
                ));
            }
            4 => {
                view.unparent();
                surface.mounter.borrow_mut().apply(
                    &surface,
                    &nsh_core::MountItem::Create {
                        id: 2,
                        kind: nsh_core::ViewKind::Input,
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
                let replacement = surface.view(2).expect("replacement field");
                current = input_of(&replacement).expect("replacement input");
                current.model.borrow_mut().insert("replacement", None);
                set_focus(&surface, Some(&replacement));
                assert!(set_hooks(
                    &surface,
                    &identity(&surface, &current),
                    Some(hooks)
                ));
            }
            5 => surface.generation.set(surface.generation.get() + 1),
            6 => {
                let (other, _, other_input) = field();
                other_input.model.borrow_mut().insert("other", None);
                assert!(set_hooks(
                    &other,
                    &identity(&other, &other_input),
                    Some(hooks)
                ));
                let captured = identity(&other, &other_input);
                action(&other);
                assert_eq!(calls.get(), 0);
                assert_eq!(other_input.model.borrow().text(), "other");
                assert!(other_input.hooks.borrow().is_some());
                assert!(captured.same_focus(&identity(&other, &other_input)));
                continue;
            }
            _ => unreachable!(),
        }
        let captured = identity(&surface, &current);
        let registration = current.hook_registration.borrow().clone();
        let attached = current.hooks.borrow().is_some();
        let serial = current.native_serial.get();
        let text = current.model.borrow().text().to_owned();
        action(&surface);
        assert_eq!(calls.get(), 0);
        assert_eq!(current.model.borrow().text(), text);
        assert_eq!(current.native_serial.get(), serial);
        assert_eq!(current.hooks.borrow().is_some(), attached);
        assert!(captured.same_focus(&identity(&surface, &current)));
        assert!(Rc::ptr_eq(
            &registration,
            &current.hook_registration.borrow()
        ));
    }
}

pub fn native_menu_undo_rechecks_target_after_declining_policy() {
    for rebind in [false, true] {
        let (surface, _, input) = field();
        input.model.borrow_mut().insert("draft", None);
        let calls = Rc::new(Cell::new(0));
        let counter = calls.clone();
        let reports = Rc::new(RefCell::new(Vec::new()));
        let output = reports.clone();
        let weak = Rc::downgrade(&surface);
        let hooks = crate::editing::InputHooks {
            key: Rc::new(move |_, current| {
                counter.set(counter.get() + 1);
                let surface = weak.upgrade().expect("surface");
                if rebind {
                    let output = output.clone();
                    assert!(set_hooks(
                        &surface,
                        &current,
                        Some(crate::editing::InputHooks {
                            edit: Rc::new(move |edit| output.borrow_mut().push(edit)),
                            ..inert_hooks()
                        })
                    ));
                } else {
                    set_focus(&surface, None);
                }
                false
            }),
            ..inert_hooks()
        };
        assert!(set_hooks(
            &surface,
            &identity(&surface, &input),
            Some(hooks)
        ));
        let captured = identity(&surface, &input);
        let action = undo_menu_action(&surface, &input);
        action(&surface);
        assert_eq!(calls.get(), 1);
        assert_eq!(input.model.borrow().text(), "draft");
        assert!(input.model.borrow().can_undo());
        assert_eq!(input.native_serial.get(), if rebind { 2 } else { 1 });
        assert_eq!(input.count.get(), 0);
        if rebind {
            let reports = reports.borrow();
            assert_eq!(reports.len(), 1);
            let report = &reports[0];
            assert_eq!(report.native_edit_serial, 2);
            assert_eq!(report.text, "draft");
            assert_eq!((report.anchor_utf16, report.head_utf16), (5, 5));
            assert!(!report.composing);
            assert!(report.mutation.is_none());
            assert!(captured.same_focus(&report.identity));
            assert!(input.hooks.borrow().is_some());
            assert!(captured.same_focus(&identity(&surface, &input)));
        } else {
            assert!(reports.borrow().is_empty());
            assert!(input.hooks.borrow().is_none());
            assert_eq!(input.focus_epoch.get(), captured.focus_epoch + 1);
            assert!(focused(&surface).is_none());
        }
    }
}

pub fn native_opaque_history_is_not_reported_as_a_guessed_mutation() {
    for shift in [false, true] {
        let (surface, _, input) = field();
        let edits = observe(&surface, &input);
        ime::replace_text_in_range(&surface, None, "draft");
        expect_edit(&edits, 2, "draft", (5, 5), false, Some(((0, 0), "draft")));
        assert!(crate::input::key(
            &surface,
            KeyEvent {
                vk: 0x5A,
                down: true,
                repeat: false,
                modifiers: Modifiers {
                    ctrl: !cfg!(target_os = "macos"),
                    meta: cfg!(target_os = "macos"),
                    shift,
                    ..Default::default()
                },
            }
        ));
        assert_eq!(input.model.borrow().text(), "draft");
        assert_eq!(edits.borrow().len(), 2);
        assert!(input.hooks.borrow().is_none());
    }
}

pub fn native_paste_preserves_original_entries_and_opaque_metadata() {
    for attachments in [false, true] {
        let (surface, view, input) = field();
        input.model.borrow_mut().set_text("draft");
        input.model.borrow_mut().select(5, 5);
        let mut item = gpui::ClipboardItem {
            entries: vec![
                gpui::ClipboardEntry::String(gpui::ClipboardString {
                    text: "a".into(),
                    metadata: None,
                }),
                gpui::ClipboardEntry::String(gpui::ClipboardString {
                    text: "中".into(),
                    metadata: Some(String::new()),
                }),
                gpui::ClipboardEntry::String(gpui::ClipboardString {
                    text: "".into(),
                    metadata: Some("<p>opaque, not HTML</p>\0not-json".into()),
                }),
            ],
        };
        if attachments {
            item.entries.insert(
                1,
                gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
                    gpui::ImageFormat::Png,
                    vec![137, 80, 78, 71, 13, 10, 26, 10],
                )),
            );
            item.entries
                .push(gpui::ClipboardEntry::ExternalPaths(gpui::ExternalPaths(
                    [std::path::PathBuf::from("/tmp/original name.png")]
                        .into_iter()
                        .collect(),
                )));
        }
        let expected = item.clone();
        let text_ptr = match &item.entries[0] {
            gpui::ClipboardEntry::String(value) => value.text.as_ptr(),
            _ => unreachable!(),
        };
        let payload = InputPastePayload::native(Some(item));
        assert_eq!(payload.text(), "a中");
        assert!(payload.html().is_empty());
        assert_eq!(payload.files().count(), usize::from(attachments));
        assert_eq!(payload.uris().len(), usize::from(attachments));
        let carriers = Rc::new(RefCell::new(None));
        let observed = carriers.clone();
        let completed = Rc::new(Cell::new(0));
        let count = completed.clone();
        let expected_hook = expected.clone();
        let hooks = crate::editing::InputHooks {
            paste_begin: Rc::new(|_| true),
            paste: Rc::new(move |event| {
                let item = event.payload.original().expect("original item");
                assert_eq!(item, &expected_hook);
                assert!(event.payload.html().is_empty());
                observed.replace(Some(Rc::downgrade(event.payload)));
                crate::editing::PasteDecision::Insert(if attachments {
                    String::new()
                } else {
                    event.payload.text().to_owned()
                })
            }),
            paste_complete: Rc::new(move |result| {
                let result = result.expect("completed original paste");
                let item = result.payload.original().expect("owned original");
                let bound = carriers.borrow();
                let payload = bound.as_ref().expect("approved carrier");
                assert!(payload.ptr_eq(&Rc::downgrade(&result.payload)));
                assert_eq!(item, &expected);
                let gpui::ClipboardEntry::String(value) = &item.entries[0] else {
                    panic!("first string");
                };
                assert_eq!(value.text.as_ptr(), text_ptr);
                assert!(result.payload.html().is_empty());
                assert_eq!(result.edit.native_edit_serial, 2);
                if attachments {
                    assert_eq!(result.edit.text, "draft");
                    assert_eq!(result.edit.mutation, None);
                } else {
                    assert_eq!(result.edit.text, "drafta中");
                    assert_eq!(
                        result.edit.mutation,
                        Some(crate::editing::InputMutation {
                            range_utf16: (5, 5),
                            inserted: "a中".into(),
                        })
                    );
                }
                count.set(count.get() + 1);
            }),
            ..inert_hooks()
        };
        assert!(set_hooks(
            &surface,
            &identity(&surface, &input),
            Some(hooks.clone())
        ));
        session(&surface, &input, hooks).apply(&surface, &view, &input, payload);
        assert_eq!(completed.get(), 1);
        assert_eq!(input.count.get(), u32::from(!attachments));
    }
}

pub fn native_paste_none_and_refusal_do_not_fabricate_original_or_edit() {
    for empty_item in [false, true] {
        for rejected in [false, true] {
            let (surface, view, input) = field();
            let completed = Rc::new(Cell::new(0));
            let count = completed.clone();
            let hooks = crate::editing::InputHooks {
                paste_begin: Rc::new(|_| true),
                paste: Rc::new(move |event| {
                    assert_eq!(event.payload.original().is_some(), empty_item);
                    assert!(event.payload.is_empty());
                    if rejected {
                        crate::editing::PasteDecision::Reject
                    } else {
                        crate::editing::PasteDecision::Insert(String::new())
                    }
                }),
                paste_complete: Rc::new(move |result| {
                    assert_eq!(result.is_none(), rejected);
                    if let Some(result) = result {
                        assert_eq!(result.payload.original().is_some(), empty_item);
                        assert_eq!(result.edit.native_edit_serial, 2);
                        assert_eq!(result.edit.mutation, None);
                    }
                    count.set(count.get() + 1);
                }),
                ..inert_hooks()
            };
            assert!(set_hooks(
                &surface,
                &identity(&surface, &input),
                Some(hooks.clone())
            ));
            session(&surface, &input, hooks).apply(
                &surface,
                &view,
                &input,
                InputPastePayload::native(empty_item.then(|| gpui::ClipboardItem {
                    entries: Vec::new(),
                })),
            );
            assert_eq!(completed.get(), 1);
            assert_eq!(input.native_serial.get(), if rejected { 1 } else { 2 });
            assert_eq!(input.count.get(), 0);
            assert_eq!(input.model.borrow().text(), "");
        }
    }
}

pub fn native_paste_through_hooks(
    item: Option<gpui::ClipboardItem>,
    hooks: impl FnOnce(crate::editing::InputIdentity) -> crate::editing::InputHooks,
) -> impl FnOnce() {
    let (surface, view, input) = field();
    let hooks = hooks(identity(&surface, &input));
    assert!(set_hooks(
        &surface,
        &identity(&surface, &input),
        Some(hooks.clone())
    ));
    session(&surface, &input, hooks).apply(
        &surface,
        &view,
        &input,
        InputPastePayload::native(item),
    );
    move || drop((surface, view, input))
}

pub fn native_paste_full_image_budget_uses_original_borrowed_files() {
    for count in [2, 4] {
        let size = 16 * 1024 * 1024 / count;
        let mut entries = vec![gpui::ClipboardEntry::String(gpui::ClipboardString {
            text: String::new(),
            metadata: Some("opaque, not HTML".into()),
        })];
        for _ in 0..count {
            let mut bytes = Vec::with_capacity(size + 4096);
            bytes.resize(size, 0);
            bytes[..8].copy_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
            entries.push(gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
                gpui::ImageFormat::Png,
                bytes,
            )));
        }
        let original = gpui::ClipboardItem { entries };
        let expected = original.clone();
        let seal = Rc::new(RefCell::new(None));
        let approved = seal.clone();
        let retire = native_paste_through_hooks(Some(original), move |_| {
            crate::editing::InputHooks {
                paste_begin: Rc::new(|_| true),
                paste: Rc::new(move |event| {
                    let item = event.payload.original().expect("original");
                    assert_eq!(event.payload.files().count(), count);
                    assert_eq!(
                        event
                            .payload
                            .files()
                            .map(|file| file.capacity())
                            .sum::<usize>(),
                        16 * 1024 * 1024
                    );
                    for file in event.payload.files() {
                        let crate::editing::InputPasteFile::Native(image) = file else {
                            panic!("native file")
                        };
                        assert_eq!(image.bytes.len(), size);
                        assert_eq!(image.bytes.capacity(), size);
                        assert!(item.entries().iter().any(|entry| matches!(entry, gpui::ClipboardEntry::Image(candidate) if std::ptr::eq(candidate, image))));
                    }
                    approved.replace(Some(Rc::downgrade(event.payload)));
                    crate::editing::PasteDecision::Insert(String::new())
                }),
                paste_complete: Rc::new(move |completion| {
                    let completion = completion.expect("completed");
                    assert!(
                        seal.borrow()
                            .as_ref()
                            .expect("seal")
                            .ptr_eq(&Rc::downgrade(&completion.payload))
                    );
                    assert_eq!(completion.payload.original(), Some(&expected));
                    assert!(completion.payload.html().is_empty());
                    assert_eq!(completion.edit.native_edit_serial, 2);
                    assert_eq!(completion.edit.mutation, None);
                }),
                ..inert_hooks()
            }
        });
        retire();
    }
}
