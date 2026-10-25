use crate::{
    conversation_stream::{ConversationStream, FileIndexCancelled, FileIndexFailureCode},
    file_selector::{AcceptFile, CancelFile},
    settings::SettingsOpen,
};
use gpui_kit::{
    AppContext, Context, Entity, Focusable, IntoElement, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, WindowHandle, div, gpui, test::TestWindowExt,
};
use std::{cell::Cell, rc::Rc, time::Duration};
use vega_conversation::types::{
    FileIndexSnapshot, PermissionMode, TaskBackend, Thread, ThreadMode, ThreadStatus,
};
use vega_theme::theme;

fn fixture(cx: &mut TestAppContext) -> WindowHandle<ConversationStream> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::dark());
        cx.set_global(crate::sidebar::SelectedProject(Some("project".into())));
        cx.set_global(SettingsOpen(false));
        cx.set_reduce_motion(false);
    });
    let window = cx.add_window(|_, cx| new_stream("thread", cx));
    window
        .update(cx, |stream, window, cx| {
            window.activate_window();
            stream.focus_composer(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

fn new_stream(id: &str, cx: &mut Context<ConversationStream>) -> ConversationStream {
    ConversationStream::new(
        Thread {
            parent_thread_id: None,
            forked_from_thread_id: None,
            forked_from_message_id: None,
            id: id.into(),
            backend: TaskBackend::Native,
            project_id: "project".into(),
            title: "Files".into(),
            mode: ThreadMode::Ask,
            permission_mode: PermissionMode::ReadOnly,
            model: String::new(),
            status: ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 0,
            updated_at: 0,
        },
        cx,
    )
}

fn frame<V: Render + 'static>(window: WindowHandle<V>, cx: &mut TestAppContext) -> f32 {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    let bounds = VisualTestContext::from_window(window.into(), cx)
        .debug_bounds("overlay-entry-surface")
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        let center = bounds.center().scale(window.scale_factor());
        let color: gpui_kit::Hsla = theme(cx).colors.bg_elevated.into();
        window
            .painted_quads()
            .iter()
            .find_map(|quad| {
                let paint = quad.background.as_solid()?;
                (quad.bounds.contains(&center)
                    && (paint.h - color.h).abs() < 0.0001
                    && (paint.s - color.s).abs() < 0.0001
                    && (paint.l - color.l).abs() < 0.0001)
                    .then_some(paint.a)
            })
            .unwrap_or(0.)
    })
    .unwrap()
}

fn open(window: WindowHandle<ConversationStream>, cx: &mut TestAppContext) {
    window
        .update(cx, |stream, _, cx| {
            let input = stream.input.clone();
            input.update(cx, |input, cx| input.set_text("@", cx));
            stream.sync_at_query(&input, cx);
        })
        .unwrap();
}

#[gpui_kit::test]
fn file_menu_paints_entry_without_restarting_on_results_and_accepts_immediately(
    cx: &mut TestAppContext,
) {
    let window = fixture(cx);
    open(window, cx);
    assert_eq!(frame(window, cx), 0.);
    cx.background_executor
        .advance_clock(Duration::from_millis(60));
    let middle = frame(window, cx);
    assert!(middle > 0. && middle < 1.);
    window
        .update(cx, |stream, _, cx| {
            assert!(stream.apply_file_index_result(
                stream.file_index_generation,
                Ok(FileIndexSnapshot {
                    entries: vec!["src/main.rs".into()],
                }),
                cx
            ));
        })
        .unwrap();
    assert!((frame(window, cx) - middle).abs() < 0.0001);
    cx.background_executor
        .advance_clock(Duration::from_millis(60));
    assert!((frame(window, cx) - 1.).abs() < 0.0001);
    window
        .update(cx, |stream, _, cx| {
            stream.close_file_selector_and_cancel(cx);
            stream.file_entry.invalidate();
            let input = stream.input.clone();
            stream.sync_at_query(&input, cx);
        })
        .unwrap();
    assert_eq!(frame(window, cx), 0.);
    window
        .update(cx, |stream, window, cx| {
            stream.on_selector_accept(&AcceptFile, window, cx);
            assert!(!stream.file_selector_wanted);
            assert!(!stream.file_selector.is_open());
            assert!(stream.input.read(cx).text().contains("src/main.rs"));
            assert!(stream.input.read(cx).focus_handle(cx).is_focused(window));
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("overlay-entry-surface")
            .is_none()
    );
}

#[gpui_kit::test]
fn file_close_reopen_preserves_painted_origin_and_cancel_fence_while_failure_snaps(
    cx: &mut TestAppContext,
) {
    let window = fixture(cx);
    let cancelled = Rc::new(Cell::new(0));
    let stream = window.root(cx).unwrap();
    let count = cancelled.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, _: &FileIndexCancelled, _| {
            count.set(count.get() + 1);
        })
        .detach()
    });
    open(window, cx);
    frame(window, cx);
    cx.background_executor
        .advance_clock(Duration::from_millis(45));
    let middle = frame(window, cx);
    assert!(middle > 0. && middle < 1.);
    let old_generation = window
        .update(cx, |stream, window, cx| {
            let old = stream.file_index_generation;
            stream.on_selector_cancel(&CancelFile, window, cx);
            assert!(!stream.file_selector_wanted);
            assert!(!stream.file_index_loading);
            assert!(stream.input.read(cx).focus_handle(cx).is_focused(window));
            let input = stream.input.clone();
            stream.sync_at_query(&input, cx);
            old
        })
        .unwrap();
    assert_eq!(cancelled.get(), 1);
    assert!((frame(window, cx) - middle).abs() < 0.0001);
    window
        .update(cx, |stream, _, cx| {
            assert!(!stream.apply_file_index_result(
                old_generation,
                Ok(FileIndexSnapshot::default()),
                cx
            ));
            assert!(stream.apply_file_index_result(
                stream.file_index_generation,
                Err(FileIndexFailureCode::Traversal),
                cx
            ));
        })
        .unwrap();
    assert!((frame(window, cx) - 1.).abs() < 0.0001);
    window
        .update(cx, |stream, _, cx| {
            stream.close_file_selector_and_cancel(cx);
            stream.invalidate_file_index(cx);
            let input = stream.input.clone();
            stream.sync_at_query(&input, cx);
        })
        .unwrap();
    assert_eq!(frame(window, cx), 0.);
}

struct RetainedHost {
    streams: [Entity<ConversationStream>; 2],
    selected: Option<usize>,
}

impl RetainedHost {
    fn select(&mut self, selected: Option<usize>, cx: &mut Context<Self>) {
        if self.selected != selected
            && let Some(previous) = self.selected
        {
            self.streams[previous].update(cx, ConversationStream::suspend_decorative_overlays);
        }
        self.selected = selected;
        cx.notify();
    }
}

impl Render for RetainedHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .children(self.selected.map(|index| self.streams[index].clone()))
    }
}

#[gpui_kit::test]
fn loaded_file_menu_retained_across_page_and_thread_unmount_resumes_opaque(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::dark());
        cx.set_global(crate::sidebar::SelectedProject(Some("project".into())));
        cx.set_global(SettingsOpen(false));
        cx.set_reduce_motion(false);
    });
    let window = cx.add_window(|_, cx| RetainedHost {
        streams: [
            cx.new(|cx| new_stream("thread", cx)),
            cx.new(|cx| new_stream("other", cx)),
        ],
        selected: Some(0),
    });
    window
        .update(cx, |host, window, cx| {
            window.activate_window();
            host.streams[0].update(cx, |stream, window_cx| {
                stream.focus_composer(window, window_cx)
            });
        })
        .unwrap();
    cx.run_until_parked();
    for destination in [None, Some(1)] {
        let (stream, generation) = window
            .update(cx, |host, _, cx| {
                let stream = host.streams[0].clone();
                let generation = stream.update(cx, |stream, cx| {
                    stream.invalidate_file_index(cx);
                    let input = stream.input.clone();
                    input.update(cx, |input, cx| input.set_text("@", cx));
                    stream.sync_at_query(&input, cx);
                    assert!(stream.apply_file_index_result(
                        stream.file_index_generation,
                        Ok(FileIndexSnapshot {
                            entries: vec!["src/main.rs".into()]
                        }),
                        cx,
                    ));
                    assert!(!stream.file_index_loading);
                    stream.file_index_generation
                });
                (stream, generation)
            })
            .unwrap();
        assert_eq!(frame(window, cx), 0.);
        cx.background_executor
            .advance_clock(Duration::from_millis(45));
        let middle = frame(window, cx);
        assert!(middle > 0. && middle < 1.);
        window
            .update(cx, |host, window, cx| {
                let focus = window.focused(cx);
                cx.set_global(SettingsOpen(destination.is_none()));
                host.select(destination, cx);
                assert_eq!(window.focused(cx), focus);
            })
            .unwrap();
        for _ in 0..2 {
            cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
                .unwrap();
            assert!(
                VisualTestContext::from_window(window.into(), cx)
                    .debug_bounds("overlay-entry-surface")
                    .is_none()
            );
        }
        cx.background_executor
            .advance_clock(Duration::from_millis(200));
        window
            .update(cx, |host, _, cx| {
                assert_eq!(host.streams[0], stream);
                let retained = stream.read(cx);
                assert!(retained.file_selector_wanted);
                assert!(retained.file_selector.is_open());
                assert!(retained.file_index_loaded);
                assert!(!retained.file_index_loading);
                assert_eq!(retained.file_index_generation, generation);
                assert_eq!(retained.file_snapshot.entries, vec!["src/main.rs"]);
                cx.set_global(SettingsOpen(false));
                host.select(Some(0), cx);
            })
            .unwrap();
        assert!((frame(window, cx) - 1.).abs() < 0.0001);
        window
            .update(cx, |_, window, cx| {
                window.simulate_next_frame(cx);
            })
            .unwrap();
        assert!((frame(window, cx) - 1.).abs() < 0.0001);
        window
            .update(cx, |_, window, cx| {
                assert_eq!(window.simulate_next_frame(cx), 0)
            })
            .unwrap();
    }
}
