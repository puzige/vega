use super::*;
use gpui_kit::{TestAppContext, VisualTestContext};
use nsh_platform_gpui::editing::InputKey;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct MountedRename {
    block: Entity<ThreadsBlock>,
    thread: Thread,
}

impl Render for MountedRename {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(320.))
            .h(px(160.))
            .child(self.block.update(cx, |block, cx| {
                block.render_recent_row(&self.thread, &None, false, "thread-row-", true, cx)
            }))
    }
}

struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    block: Entity<ThreadsBlock>,
    visual: VisualTestContext,
    thread: Thread,
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
                "rename scheduler deadline"
            );
        }
        panic!("rename scheduler did not quiesce");
    }

    fn wait(&mut self, stage: &str, mut predicate: impl FnMut(&mut Self) -> bool) {
        let started = Instant::now();
        loop {
            self.frame();
            if predicate(self) {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "rename stage: {stage}; {}",
                self.state()
            );
            std::thread::sleep(Duration::from_millis(10));
            self.visual
                .cx
                .background_executor
                .advance_clock(Duration::from_millis(10));
        }
    }

    fn state(&mut self) -> String {
        self.visual.update(|window, cx| {
            let block = self.block.read(cx);
            let session = block.editing.as_ref().map(|session| {
                format!(
                    "{} {}",
                    session.binding.test_state(block, cx),
                    session.input.read(cx).nsh_test_state(window, cx),
                )
            });
            let persisted = cx
                .global::<VegaStore>()
                .0
                .as_ref()
                .ok()
                .and_then(|store| conversation::get_thread(store, &self.thread.id).ok())
                .map(|thread| thread.title.chars().take(96).collect::<String>());
            let error = block
                .error
                .as_ref()
                .map(|error| error.render().chars().take(256).collect::<String>());
            format!(
                "session={session:?} persisted={persisted:?} pending={}/{}/{} project={:?} thread_present={} error={error:?}",
                block.action_pending,
                task_mutation_busy(cx),
                block.visit_pending,
                block.loaded_project,
                block.threads.iter().chain(block.archived.iter()).any(|thread| thread.id == self.thread.id),
            )
        })
    }

    fn input(&mut self) -> Entity<TextInput> {
        self.visual.update(|_, cx| {
            self.block
                .read(cx)
                .editing
                .as_ref()
                .expect("rename session")
                .input
                .clone()
        })
    }

    fn native(&mut self, operation: impl FnOnce(&nsh_platform_gpui::AppView, u32)) {
        let input = self.input();
        self.visual.update(|_, cx| {
            let (view, node) = input
                .read(cx)
                .nsh_test_host()
                .expect("actual NSH input node");
            operation(view.read(cx), node);
        });
    }

    fn ready(&mut self) {
        self.wait("mounted native authority", |this| {
            let input = this.input();
            this.visual.update(|_, cx| {
                input.read(cx).nsh_test_host().is_some() && input.read(cx).composer_editor_ready()
            })
        });
        let input = self.input();
        self.visual.update(|window, cx| {
            let (view, node) = input.read(cx).nsh_test_host().expect("native host");
            window.focus(&view.read(cx).focus_handle(cx), cx);
            assert!(view.read(cx).integration_edit_focus(node));
        });
        self.wait("native focus registration", |this| {
            let input = this.input();
            this.visual
                .update(|window, cx| input.read(cx).nsh_test_registered(window, cx))
        });
    }

    fn key(&mut self, vk: u16) {
        self.native(|view, _| {
            view.integration_edit_key(InputKey {
                virtual_key: vk,
                shift: false,
                control: false,
                alt: false,
                meta: false,
                down: true,
                repeat: false,
                context: None,
            });
        });
    }

    fn replace(&mut self, text: &str) {
        self.native(|view, node| assert!(view.integration_edit_set_value(node, text)));
        self.wait("accepted native replacement", |this| {
            let input = this.input();
            this.visual.update(|_, cx| {
                input.read(cx).text() == text && input.read(cx).composer_editor_ready()
            })
        });
        self.ready();
    }

    fn editing(&mut self) -> bool {
        self.visual
            .update(|_, cx| self.block.read(cx).editing.is_some())
    }

    fn title(&mut self) -> String {
        self.visual.update(|_, cx| {
            let VegaStore(store) = cx.global::<VegaStore>();
            conversation::get_thread(store.as_ref().expect("store"), &self.thread.id)
                .expect("persisted thread")
                .title
        })
    }

    fn reopen(&mut self) {
        self.visual.update(|window, cx| {
            self.block.update(cx, |block, cx| {
                block.start_rename(
                    &self.thread,
                    format!("thread-row-{}", self.thread.id),
                    window,
                    cx,
                );
            });
        });
        self.ready();
    }
}

pub fn mounted_rename_native_ack_ime_cancel_rebind_and_persistence(cx: &mut TestAppContext) {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let scratch =
        Scratch(std::env::temp_dir().join(format!("vega-rename-{}-{suffix}", std::process::id())));
    std::fs::create_dir(&scratch.0).expect("isolated scratch directory");
    let store = Store::open(scratch.0.join("rename.sqlite3")).expect("real file-backed store");
    store.migrate().expect("production schema migrations");
    let project = projects::create(
        store.conn(),
        scratch.0.to_str().expect("path"),
        "Rename",
        None,
    )
    .expect("project");
    let thread = conversation::create_thread(&store, &project.id, "", "confirm").expect("thread");
    let original = thread.title.clone();
    cx.update(|cx| {
        assert!(
            !cx.text_system().all_font_names().is_empty(),
            "actual platform fonts required"
        );
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(VegaStore(Ok(store)));
        cx.set_global(SelectedProject(Some(project.id)));
        cx.set_global(OpenedThread(None));
        cx.set_global(SettingsOpen(false));
    });
    let block = cx.new(ThreadsBlock::new);
    let source = block.clone();
    let mounted_thread = thread.clone();
    let window = cx.add_window(move |_, _| MountedRename {
        block: source,
        thread: mounted_thread,
    });
    let handle = window.into();
    let visual = VisualTestContext::from_window(handle, cx);
    let mut fixture = Fixture {
        block,
        visual,
        thread,
    };
    fixture.visual.update(|window, _| window.activate_window());
    fixture.reopen();
    fixture.replace("paste ");
    fixture.native(|view, node| {
        assert!(view.integration_edit_paste(
            node,
            Some(gpui_kit::ClipboardItem::new_string("文😀".into()))
        ));
    });
    fixture.wait("native paste acknowledgment", |this| {
        let input = this.input();
        this.visual.update(|_, cx| {
            input.read(cx).text() == "paste 文😀" && input.read(cx).composer_editor_ready()
        })
    });
    fixture.ready();
    fixture.native(|view, node| assert!(view.integration_edit_menu(node, false)));
    fixture.wait("native undo acknowledgment", |this| {
        let input = this.input();
        this.visual.update(|_, cx| {
            input.read(cx).text() == "paste " && input.read(cx).composer_editor_ready()
        })
    });
    fixture.ready();
    fixture.native(|view, node| assert!(view.integration_edit_menu(node, true)));
    fixture.wait("native redo acknowledgment", |this| {
        let input = this.input();
        this.visual.update(|_, cx| {
            input.read(cx).text() == "paste 文😀" && input.read(cx).composer_editor_ready()
        })
    });
    fixture.ready();
    fixture.replace("  native 中文😀  ");
    fixture.native(|view, node| assert!(view.integration_edit_mark(node, "候选")));
    fixture.key(0x0d);
    fixture.key(0x1b);
    fixture.frame();
    assert!(fixture.editing());
    assert_eq!(fixture.title(), original);
    fixture.native(|view, _| view.integration_edit_unmark());
    fixture.ready();
    fixture.replace("  native 中文😀  ");
    fixture.native(|view, node| assert!(view.integration_edit_type(node, "x")));
    fixture.key(0x0d);
    fixture.frame();
    assert!(fixture.editing());
    assert_eq!(fixture.title(), original);
    fixture.ready();
    let accepted = fixture.input();
    let expected = "native 中文😀  x";
    fixture
        .visual
        .update(|_, cx| assert_eq!(accepted.read(cx).text(), "  native 中文😀  x"));
    fixture.key(0x0d);
    fixture.wait("acknowledged rename persisted", |this| !this.editing());
    assert_eq!(fixture.title(), expected);
    for (pasted, normalized) in [
        ("alpha\nbeta", "alpha beta"),
        ("alpha\r\nbeta", "alpha\r beta"),
        ("alpha\rbeta", "alpha\rbeta"),
    ] {
        fixture.reopen();
        fixture.replace("中😀文");
        fixture.native(|view, _| {
            assert!(view.integration_edit_key(InputKey {
                virtual_key: 0x41,
                shift: false,
                control: !cfg!(target_os = "macos"),
                alt: false,
                meta: cfg!(target_os = "macos"),
                down: true,
                repeat: false,
                context: None,
            }));
        });
        fixture.wait("native select all acknowledgment", |this| {
            let input = this.input();
            this.visual.update(|_, cx| {
                let Some((view, node)) = input.read(cx).nsh_test_host() else {
                    return false;
                };
                view.read(cx)
                    .integration_edit_snapshot(node)
                    .is_some_and(|edit| {
                        edit.anchor_utf16.min(edit.head_utf16) == 0
                            && edit.anchor_utf16.max(edit.head_utf16) == 4
                    })
                    && input.read(cx).composer_editor_ready()
            })
        });
        fixture.native(|view, node| {
            assert!(view.integration_edit_paste(
                node,
                Some(gpui_kit::ClipboardItem::new_string_with_metadata(
                    pasted.into(),
                    "opaque-not-html".into()
                ))
            ));
        });
        fixture.wait("single line paste acknowledgment", |this| {
            let input = this.input();
            this.visual.update(|_, cx| {
                input.read(cx).text() == normalized && input.read(cx).composer_editor_ready()
            })
        });
        fixture.ready();
        fixture.key(0x0d);
        fixture.wait("single line native paste persisted", |this| !this.editing());
        assert_eq!(fixture.title(), normalized);
    }
    fixture.reopen();
    fixture.replace(expected);
    fixture.key(0x0d);
    fixture.wait("original accepted title restored", |this| !this.editing());
    assert_eq!(fixture.title(), expected);
    fixture.reopen();
    fixture.replace("   ");
    fixture.key(0x0d);
    fixture.wait("blank cancellation", |this| !this.editing());
    assert_eq!(fixture.title(), expected);
    fixture.reopen();
    fixture.replace("discard escape");
    fixture.key(0x1b);
    fixture.wait("native escape cancellation", |this| !this.editing());
    assert_eq!(fixture.title(), expected);
    fixture.reopen();
    fixture.replace("discard outside");
    fixture
        .visual
        .simulate_click(point(px(310.), px(140.)), gpui_kit::Modifiers::none());
    fixture.wait("outside cancellation", |this| !this.editing());
    assert_eq!(fixture.title(), expected);
    fixture.reopen();
    fixture.replace("must not be submitted by old session");
    fixture.key(0x0d);
    fixture.visual.update(|window, cx| {
        fixture.block.update(cx, |block, cx| {
            let input = block.editing.as_ref().expect("old session").input.clone();
            block.bind_rename(
                fixture.thread.id.clone(),
                format!("thread-row-{}", fixture.thread.id),
                input,
                window,
                cx,
            );
        });
    });
    fixture.frame();
    assert!(fixture.editing());
    assert_eq!(fixture.title(), expected);
    fixture.ready();
    fixture.key(0x1b);
    fixture.visual.update(|window, cx| {
        fixture.block.update(cx, |block, cx| {
            block.cancel_rename(cx);
            block.start_rename(
                &fixture.thread,
                format!("thread-row-{}", fixture.thread.id),
                window,
                cx,
            );
        });
    });
    fixture.frame();
    assert!(fixture.editing());
    assert_eq!(fixture.title(), expected);
    fixture.ready();
    fixture.replace("must not cross navigation");
    fixture.key(0x0d);
    fixture
        .visual
        .update(|_, cx| cx.set_global(SettingsOpen(true)));
    fixture.frame();
    assert!(fixture.editing());
    assert_eq!(fixture.title(), expected);
    fixture.visual.update(|window, cx| {
        cx.set_global(SettingsOpen(false));
        fixture
            .block
            .update(cx, |block, cx| block.cancel_rename(cx));
        window.refresh();
    });
}
