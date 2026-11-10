use super::*;
use gpui_kit::{AppContext as _, Render, TestAppContext, VisualTestContext};
use nsh_platform_gpui::editing::{InputEdit, InputKey};
use std::time::{Duration, Instant};

struct MountedEditor {
    input: Entity<TextInput>,
}

impl Render for MountedEditor {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        gpui_kit::div()
            .w(gpui_kit::px(640.))
            .child(TextInput::render_nsh_input(self.input.clone(), window, cx))
    }
}

struct Fixture {
    input: Entity<TextInput>,
    visual: VisualTestContext,
}

impl Fixture {
    fn frame(&mut self) {
        self.drain();
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        self.drain();
    }

    fn drain(&mut self) {
        let started = Instant::now();
        for _ in 0..512 {
            if !self.visual.cx.background_executor.tick() {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "editor scheduler deadline"
            );
        }
        panic!("editor scheduler did not quiesce");
    }

    fn wait(&mut self, stage: &str, mut ready: impl FnMut(&mut Self) -> bool) {
        let started = Instant::now();
        loop {
            self.frame();
            if ready(self) {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "editor stage: {stage}"
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn ready(&mut self) {
        self.wait("fresh registration and authority receipt", |this| {
            this.visual.update(|_, cx| {
                let input = this.input.read(cx);
                input.nsh_composer.as_ref().is_some_and(|editor| {
                    editor.node.borrow().is_some() && input.composer_editor_ready()
                })
            })
        });
        self.visual.update(|window, cx| {
            let (view, node, focus) = {
                let input = self.input.read(cx);
                let editor = input.nsh_composer.as_ref().expect("mounted editor");
                let node = (*editor.node.borrow()).expect("real input node");
                (
                    editor.surface.entity(),
                    node,
                    editor.surface.focus_handle(cx),
                )
            };
            window.focus(&focus, cx);
            assert!(view.read(cx).integration_edit_focus(node));
        });
        self.wait("focused registration", |this| {
            this.visual.update(|window, cx| {
                let input = this.input.read(cx);
                let editor = input.nsh_composer.as_ref().expect("editor");
                let node = (*editor.node.borrow()).expect("node");
                let identity = editor
                    .surface
                    .entity()
                    .read(cx)
                    .input_identity(node)
                    .expect("identity");
                input.composer_editor_ready()
                    && input.composer_contains_focus(window, cx)
                    && !editor
                        .adapter
                        .as_ref()
                        .expect("adapter")
                        .needs_registration(&identity, true)
            })
        });
    }

    fn native(&mut self, apply: impl FnOnce(&nsh_platform_gpui::AppView, u32)) {
        self.visual.update(|_, cx| {
            let input = self.input.read(cx);
            let editor = input.nsh_composer.as_ref().expect("mounted editor");
            let node = (*editor.node.borrow()).expect("real input node");
            apply(editor.surface.entity().read(cx), node);
        });
    }

    fn snapshot(&mut self) -> InputEdit {
        let mut snapshot = None;
        self.native(|view, node| snapshot = view.integration_edit_snapshot(node));
        snapshot.expect("nonsecure native snapshot")
    }

    fn text(&mut self) -> String {
        self.visual
            .update(|_, cx| self.input.read(cx).text().to_owned())
    }

    fn history(&mut self, redo: bool, expected: &str) {
        self.native(|view, node| assert!(view.integration_edit_menu(node, redo)));
        self.wait("actual history menu authoritative commit", |this| {
            this.text() == expected
        });
        self.ready();
        assert_eq!(self.snapshot().text, expected);
    }
}

pub fn mounted_native_recovery_history_ax_and_submit_close_production_loop(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        assert!(
            !cx.text_system().all_font_names().is_empty(),
            "actual text system required"
        );
        cx.set_global(vega_theme::Theme::light());
    });
    let input = cx.new(|cx| {
        TextInput::new(cx, "", false)
            .with_image_paste()
            .with_row_bounds(1, 8)
    });
    input.update(cx, |input, cx| {
        input.set_text("@keep tail", cx);
        input.push_mention(0..5, "keep");
    });
    let images = Rc::new(RefCell::new(Vec::new()));
    let sink = images.clone();
    let submits = Rc::new(Cell::new(0));
    let submitted = submits.clone();
    let observer = cx.new(|_| ());
    let image_subscription = observer.update(cx, |_, cx| {
        cx.subscribe(&input, move |_, _, event: &super::super::ImagePaste, _| {
            sink.borrow_mut().push(event.0.clone());
        })
    });
    let submit_subscription = observer.update(cx, |_, cx| {
        cx.subscribe(&input, move |_, input, event: &ComposerShortcut, cx| {
            assert!(event.matches(input.read(cx), ComposerRoute::default()));
            if matches!(event.key, ComposerKey::Send) {
                submitted.set(submitted.get() + 1);
            }
        })
    });
    let source = input.clone();
    let window = cx.add_window(move |_, _| MountedEditor { input: source });
    let visual = VisualTestContext::from_window(window.into(), cx);
    let mut fixture = Fixture { input, visual };
    fixture.visual.update(|window, _| window.activate_window());
    fixture.ready();
    let first = fixture.snapshot();
    assert_eq!(first.text, "@keep tail");
    assert!(first.mutation.is_none());
    let original = gpui_kit::ClipboardItem {
        entries: vec![
            gpui_kit::ClipboardEntry::String(gpui_kit::ClipboardString {
                text: String::new(),
                metadata: Some("not-json <b>opaque</b>".into()),
            }),
            gpui_kit::ClipboardEntry::Image(gpui_kit::Image::from_bytes(
                gpui_kit::ImageFormat::Png,
                b"\x89PNG\r\n\x1a\n".to_vec(),
            )),
        ],
    };
    fixture.native(|view, node| assert!(view.integration_edit_paste(node, Some(original.clone()))));
    fixture.wait("authority ImagePaste", |_| images.borrow().len() == 1);
    fixture.ready();
    assert_eq!(*images.borrow()[0], original);
    assert_eq!(fixture.text(), "@keep tail");
    assert!(fixture.snapshot().mutation.is_none());
    fixture.native(|view, node| assert!(view.integration_edit_type(node, "!")));
    fixture.wait("typed authority", |this| this.text() == "@keep tail!");
    fixture.ready();
    fixture.history(false, "@keep tail");
    fixture.history(true, "@keep tail!");
    fixture.visual.update(|_, cx| {
        assert_eq!(fixture.input.read(cx).mentions[0].range(), 0..5);
    });
    fixture.native(|view, node| assert!(view.integration_edit_set_value(node, "AX 😀中")));
    fixture.wait("AX ordinary full replacement", |this| {
        this.text() == "AX 😀中"
    });
    fixture.ready();
    let ax = fixture.visual.update(|_, cx| {
        fixture
            .input
            .read(cx)
            .nsh_composer
            .as_ref()
            .expect("editor")
            .native
            .local_draft()
    });
    assert_eq!(
        ax.mutation.as_ref().expect("AX replacement").range_utf16,
        (0, 11)
    );
    assert_eq!(
        ax.mutation
            .as_ref()
            .expect("AX replacement")
            .inserted
            .as_ref(),
        "AX 😀中"
    );
    assert!(fixture.snapshot().mutation.is_none());
    assert_eq!(images.borrow().len(), 1, "AX is not paste");
    fixture
        .visual
        .update(|_, cx| assert!(fixture.input.read(cx).mentions.is_empty()));
    fixture.history(false, "@keep tail!");
    fixture.native(|view, node| assert!(view.integration_edit_mark(node, "候😀")));
    fixture.wait("actual marked display authority", |this| {
        this.visual
            .update(|_, cx| this.input.read(cx).is_composing())
    });
    let marked = fixture.snapshot();
    assert!(marked.composing);
    fixture.native(|view, node| assert!(view.integration_edit_menu(node, false)));
    fixture.frame();
    assert_eq!(fixture.snapshot().text, marked.text);
    assert!(
        !fixture
            .visual
            .update(|_, cx| fixture.input.read(cx).composer_editor_ready())
    );
    fixture.native(|view, _| view.integration_edit_unmark());
    fixture.wait("composition committed", |this| {
        this.visual
            .update(|_, cx| !this.input.read(cx).is_composing())
    });
    fixture.ready();
    fixture.history(false, "@keep tail!");
    let old_identity = fixture.snapshot().identity;
    let (old_id, old_surface) = fixture.visual.update(|_, cx| {
        let editor = fixture
            .input
            .read(cx)
            .nsh_composer
            .as_ref()
            .expect("editor");
        (editor.id.clone(), editor.surface.entity())
    });
    fixture.visual.update(|_, cx| {
        let input = fixture.input.read(cx);
        let editor = input.nsh_composer.as_ref().expect("editor");
        let node = (*editor.node.borrow()).expect("node");
        assert!(
            editor
                .surface
                .entity()
                .read(cx)
                .integration_edit_type(node, "unsent 中文")
        );
        editor.adapter.as_ref().expect("adapter").fail();
    });
    fixture.wait("real recovery page", |this| {
        this.visual.update(|_, cx| {
            this.input
                .read(cx)
                .nsh_composer
                .as_ref()
                .and_then(|editor| editor.recovery_page.as_ref())
                .is_some_and(|page| {
                    page.entity()
                        .read(cx)
                        .integration_edit_press_ready(vega_i18n::t("settings.action.reload"))
                })
        })
    });
    assert_eq!(fixture.text(), "@keep tail!");
    fixture.visual.update(|_, cx| {
        fixture.input.update(cx, |input, cx| {
            let revision = input.nsh_selection_revision;
            input.nsh_selection_revision = u64::MAX;
            assert!(!input.reload_native_editor(&old_id, cx));
            input.nsh_selection_revision = revision;
            let page = input
                .nsh_composer
                .as_ref()
                .expect("editor")
                .recovery_page
                .as_ref()
                .expect("recovery");
            assert!(
                page.entity()
                    .read(cx)
                    .integration_edit_press(vega_i18n::t("settings.action.reload"))
            );
        });
    });
    fixture.wait("reload new native owner", |this| {
        this.visual.update(|_, cx| {
            this.input
                .read(cx)
                .nsh_composer
                .as_ref()
                .is_some_and(|editor| !editor.id.matches(&old_id))
        })
    });
    fixture.ready();
    assert_eq!(fixture.text(), "@keep tail!");
    fixture.visual.update(|_, cx| {
        assert!(
            old_surface
                .read(cx)
                .input_identity(old_identity.node)
                .is_none()
        );
        fixture.input.update(cx, |input, cx| {
            assert!(!input.reload_native_editor(&old_id, cx));
            assert_eq!(input.mentions[0].range(), 0..5);
            assert_eq!(
                input
                    .nsh_composer
                    .as_ref()
                    .expect("editor")
                    .draft
                    .saved()
                    .as_deref(),
                Some("@keep tail!unsent 中文")
            );
        });
    });
    fixture.wait("restore control mounted", |this| {
        this.visual.update(|_, cx| {
            let editor = this.input.read(cx).nsh_composer.as_ref().expect("editor");
            editor
                .surface
                .entity()
                .read(cx)
                .integration_edit_press(vega_i18n::t("ui.restore_local_draft"))
        })
    });
    fixture.wait("explicit restored authority", |this| {
        this.text() == "@keep tail!unsent 中文"
    });
    fixture.ready();
    fixture.history(false, "@keep tail!");
    fixture
        .visual
        .update(|_, cx| assert_eq!(fixture.input.read(cx).mentions[0].range(), 0..5));
    fixture.history(true, "@keep tail!unsent 中文");
    fixture.native(|view, node| assert!(view.integration_edit_type(node, "!")));
    fixture.wait("continued native edit", |this| {
        this.text() == "@keep tail!unsent 中文!"
    });
    fixture.ready();
    fixture.native(|view, _| {
        assert!(view.integration_edit_key(InputKey {
            virtual_key: 0x0d,
            down: true,
            repeat: false,
            shift: false,
            control: !cfg!(target_os = "macos"),
            meta: cfg!(target_os = "macos"),
            alt: false,
            context: None,
        }));
    });
    fixture.wait("single eligible production submit event", |_| {
        submits.get() == 1
    });
    assert_eq!(
        images.borrow().len(),
        1,
        "recovery/history must not replay attachment"
    );
    drop((image_subscription, submit_subscription));
    fixture.visual.update(|_, cx| {
        fixture
            .input
            .update(cx, |input, cx| input.suspend_nsh_composer(cx))
    });
}
