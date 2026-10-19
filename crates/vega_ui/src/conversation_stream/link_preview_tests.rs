use std::{cell::RefCell, io::Cursor, rc::Rc, time::Duration};

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{
    AppContext, Context, Entity, FocusHandle, Focusable, FontStyle, FontWeight, InputEvent,
    IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Render, ScrollDelta, ScrollHandle, TestAppContext, VisualTestContext, Window,
    WindowHandle, div, point,
    prelude::*,
    px, size,
    test::{TestSupportExt, TestWindowExt},
};
use vega_conversation::{
    link_preview_test_support::{ManualClock, Reply, ScriptedNetwork},
    types::{PaletteFilePreview, UpdatePhase, UpdateProjection},
};
use vega_theme::{DARK, LIGHT, Theme};

use super::{
    StreamCounters, StreamModel,
    links::link_ranges,
    model::{MessageCopy, coalesce, flatten_inlines},
    render_rows,
    thinking::{ThinkingBlock, link_preview_fixture},
};
use crate::{
    file_preview::FilePreview,
    link_preview::{
        MarkdownLinkPreviewLayer, MarkdownReadingScope,
        test_support::{LinkObservation, install_dependencies, visible_links},
    },
    settings::SettingsView,
    text_input::TextInput,
};

const TARGET: &str = "https://github.com/example/project/issues/1#discussion";
const REQUEST: &str = "https://github.com/example/project/issues/1";

#[derive(Clone, Copy, Debug)]
enum HostKind {
    Ordinary,
    Selectable,
    User,
    File,
    ReleaseNotes,
    Thinking,
}

struct Host {
    kind: HostKind,
    source: String,
    model: StreamModel,
    copy: MessageCopy,
    reading: FocusHandle,
    composer: Entity<TextInput>,
    scroll: ScrollHandle,
    scope: String,
    show_body: bool,
    file: Entity<FilePreview>,
    settings: Entity<SettingsView>,
    thinking: Entity<ThinkingBlock>,
}

fn model(source: &str) -> StreamModel {
    let mut parser = vega_markdown::MarkdownStream::new();
    parser.append(source);
    parser.finish();
    let mut model = StreamModel::default();
    model.sync(&parser.snapshot(), &StreamCounters::default());
    model
}

impl Host {
    fn new(kind: HostKind, source: &str, cx: &mut Context<Self>) -> Self {
        Self {
            kind,
            source: source.into(),
            model: model(source),
            copy: MessageCopy::new(source),
            reading: cx.focus_handle(),
            composer: cx.new(|cx| TextInput::new(cx, "Message", false)),
            scroll: ScrollHandle::new(),
            scope: "reading-session-a".into(),
            show_body: true,
            file: cx.new(|cx| {
                FilePreview::new(
                    PaletteFilePreview {
                        relative_path: "fixture.md".into(),
                        content: source.into(),
                    },
                    cx,
                )
            }),
            settings: cx.new(|cx| {
                let mut settings = SettingsView::from_config(Default::default(), None, cx);
                settings.apply_update_projection(
                    UpdateProjection {
                        notes: source.into(),
                        current_version: "1.0.0".into(),
                        phase: UpdatePhase::Current,
                        ..Default::default()
                    },
                    cx,
                );
                settings.updater_notes_open = true;
                settings
            }),
            thinking: link_preview_fixture::restored(source, cx),
        }
    }

    fn replace_source(&mut self, source: &str, cx: &mut Context<Self>) {
        self.source = source.into();
        self.model = model(source);
        cx.notify();
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = vega_theme::theme(cx).colors;
        let body = match self.kind {
            HostKind::Ordinary => {
                render_rows::markdown_item(&self.model, None, &colors, "ordinary")
            }
            HostKind::Selectable => render_rows::selectable_markdown_content(
                &self.model,
                &self.copy,
                &self.reading,
                &colors,
                true,
                window,
                cx,
            ),
            HostKind::User => render_rows::user_message_item(
                &render_rows::user_message_lines(1, &self.source),
                &colors,
            ),
            HostKind::File => self.file.clone().into_any_element(),
            HostKind::ReleaseNotes => self
                .settings
                .update(cx, |settings, cx| settings.render_updater(cx)),
            HostKind::Thinking => {
                ThinkingBlock::render_stage_source(self.thinking.clone(), window, cx)
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .child(gpui_kit::base::TextSelectionLayer)
            .child(
                div()
                    .id("outside")
                    .test_support()
                    .h(px(24.))
                    .child("Outside"),
            )
            .child(
                MarkdownReadingScope::new(self.scope.clone(), self.reading.clone())
                    .track_scroll(self.scroll.clone())
                    .child(
                        div()
                            .id("body-frame")
                            .test_support()
                            .w(px(340.))
                            .h(px(240.))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .children(self.show_body.then_some(body)),
                    ),
            )
            .child(
                div()
                    .id("composer-frame")
                    .test_support()
                    .h(px(40.))
                    .child(self.composer.clone()),
            )
            .child(MarkdownLinkPreviewLayer)
    }
}

struct Harness {
    window: WindowHandle<Host>,
    network: ScriptedNetwork,
    clock: ManualClock,
    opened: Rc<RefCell<Vec<String>>>,
}

fn harness(cx: &mut TestAppContext, kind: HostKind, source: &str) -> Harness {
    let network = ScriptedNetwork::default();
    let clock = ManualClock::default();
    let opened = Rc::new(RefCell::new(Vec::new()));
    let record = opened.clone();
    let service = network.service(&clock);
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(Theme::light());
        crate::init(cx);
        install_dependencies(
            service,
            Rc::new(move |target| record.borrow_mut().push(target.into())),
            cx,
        );
    });
    let window = cx.open_window(size(px(600.), px(500.)), |_, cx| {
        Host::new(kind, source, cx)
    });
    let (composer, scroll) = window
        .read_with(cx, |host, cx| {
            (host.composer.read(cx).focus_handle(cx), host.scroll.clone())
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.activate_window();
        window.focus(&composer, cx);
        window.render_frame(cx);
        if matches!(kind, HostKind::ReleaseNotes) {
            scroll.scroll_to_bottom();
            window.render_frame(cx);
        }
    })
    .unwrap();
    cx.run_until_parked();
    Harness {
        window,
        network,
        clock,
        opened,
    }
}

fn links(cx: &mut TestAppContext, harness: &Harness) -> Vec<LinkObservation> {
    cx.update_window(harness.window.into(), |_, window, cx| {
        window.render_frame(cx);
        visible_links(window, cx)
    })
    .unwrap()
}

fn tick(cx: &mut TestAppContext, harness: &Harness, millis: u64) {
    harness.clock.advance(Duration::from_millis(millis));
    cx.executor().advance_clock(Duration::from_millis(millis));
    cx.run_until_parked();
    cx.update_window(harness.window.into(), |_, window, cx| {
        window.render_frame(cx)
    })
    .unwrap();
    cx.run_until_parked();
}

fn move_pointer(cx: &mut TestAppContext, harness: &Harness, position: Point<Pixels>) {
    cx.update_window(harness.window.into(), |_, window, cx| {
        window.dispatch_event(
            MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn hover(cx: &mut TestAppContext, harness: &Harness, index: usize) {
    let position = links(cx, harness)[index].text_bounds[0].center();
    move_pointer(cx, harness, position);
}

fn card_visible(cx: &mut TestAppContext, harness: &Harness) -> bool {
    cx.update_window(harness.window.into(), |_, window, _| {
        window.try_find("markdown-link-preview").is_some()
    })
    .unwrap()
}

fn reply() -> Reply {
    Reply::html(
        "<head><meta property=og:title content='Real target title'><meta property=og:description content='Real target summary'></head>",
    )
}

#[test]
fn link_occurrences_survive_mixed_styles_and_equal_target_neighbors_without_source_bytes() {
    let mut spans = Vec::new();
    flatten_inlines(
        &[
            vega_markdown::Inline::Link {
                url: TARGET.into(),
                title: None,
                spans: vec![
                    vega_markdown::Inline::Strong(vec![vega_markdown::Inline::Text(
                        "strong".into(),
                    )]),
                    vega_markdown::Inline::Text(" plain".into()),
                ],
            },
            vega_markdown::Inline::Link {
                url: TARGET.into(),
                title: None,
                spans: vec![vega_markdown::Inline::Text("neighbor".into())],
            },
        ],
        &mut spans,
    );
    let spans = coalesce(spans);
    assert_eq!(
        spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "strong plainneighbor"
    );
    let links = link_ranges(&spans);
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].range, 0..12);
    assert_eq!(links[1].range, 12..20);
    assert_ne!(links[0].occurrence, links[1].occurrence);
}

#[test]
fn link_styles_use_accent_medium_and_preserve_strong_italic_code_and_heading_semantics() {
    let source = format!("[plain **strong** *italic* `code`]({TARGET})");
    let model = model(&source);
    let spans = &model.committed_lines[0].spans;
    for colors in [LIGHT, DARK] {
        let runs = render_rows::block_text_runs(
            spans,
            render_rows::message_run_style(colors.text_primary),
            &colors,
        );
        assert!(
            runs.iter()
                .all(|run| run.color == colors.accent.into() && run.underline.is_none())
        );
        assert_eq!(runs[0].font.weight, FontWeight::MEDIUM);
        assert!(runs.iter().any(|run| run.font.weight == FontWeight::BOLD));
        assert!(runs.iter().any(|run| run.font.style == FontStyle::Italic));
        assert!(runs.iter().any(|run| run.background_color.is_some()
            && run.font.family.as_ref() == super::model::MONOFONT));
        let heading = render_rows::block_text_runs(
            spans,
            render_rows::heading_run_style(1, &colors),
            &colors,
        );
        assert!(
            heading
                .iter()
                .all(|run| run.font.weight >= FontWeight::MEDIUM)
        );
        assert_eq!(
            heading[0].font.weight,
            render_rows::heading_run_style(1, &colors).font_weight
        );
    }
}

#[gpui_kit::test]
fn all_real_reading_hosts_register_one_icon_and_no_load_during_materialization_or_paint(
    cx: &mut TestAppContext,
) {
    for kind in [
        HostKind::Ordinary,
        HostKind::Selectable,
        HostKind::User,
        HostKind::File,
        HostKind::ReleaseNotes,
        HostKind::Thinking,
    ] {
        let source = if matches!(kind, HostKind::User) {
            TARGET.into()
        } else {
            format!("[**A** `code` long 中英文 link]({TARGET})")
        };
        let test = harness(cx, kind, &source);
        cx.run_until_parked();
        let observations = links(cx, &test);
        assert_eq!(observations.len(), 1, "{kind:?}");
        let link = &observations[0];
        assert_eq!(link.target, TARGET);
        assert_eq!(link.icon_bounds.len(), 1, "{kind:?}");
        let icon = link.icon_bounds[0];
        assert_eq!(icon.size.width, px(16.));
        assert_eq!(icon.size.height, px(16.));
        assert_eq!(link.text_bounds[0].left() - icon.right(), px(4.));
        assert!(test.network.requests().is_empty());
        assert!(test.network.dns_calls().is_empty());
        assert!(!card_visible(cx, &test));
    }
}

#[gpui_kit::test]
fn wrapped_table_and_adjacent_equal_href_links_keep_independent_hit_targets_and_one_icon(
    cx: &mut TestAppContext,
) {
    for kind in [HostKind::Ordinary, HostKind::Selectable, HostKind::Thinking] {
        let source = format!(
            "| Left | Right |\n| --- | --- |\n| [中文 repeated long label label label label]({TARGET}) | [A]({TARGET})[B]({TARGET}) |\n\n`https://github.com/code`\n\n```text\nhttps://github.com/fenced\n```"
        );
        let test = harness(cx, kind, &source);
        let observations = links(cx, &test);
        assert_eq!(observations.len(), 3, "{kind:?}");
        assert!(observations.iter().all(|link| link.icon_bounds.len() == 1));
        assert!(observations[0].text_bounds.len() > 1);
        assert_ne!(observations[1].occurrence, observations[2].occurrence);
        assert_eq!(observations[1].label, "A");
        assert_eq!(observations[2].label, "B");
    }
}

#[gpui_kit::test]
fn hover_and_focus_paint_real_dashes_without_changing_geometry_or_copy(cx: &mut TestAppContext) {
    let source = format!(
        "before [linked label that wraps across the narrow reading column]({TARGET}) after"
    );
    let test = harness(cx, HostKind::Selectable, &source);
    let before = links(cx, &test);
    cx.update_window(test.window.into(), |_, window, _| {
        let scale = window.scale_factor();
        assert!(window.painted_quads().iter().all(|quad| {
            let width = quad.bounds.size.width.0 / scale;
            let height = quad.bounds.size.height.0 / scale;
            let left = quad.bounds.left().0 / scale;
            let top = quad.bounds.top().0 / scale;
            !(height <= 1.01
                && width > 0.
                && before[0].text_bounds.iter().any(|bounds| {
                    left >= f32::from(bounds.left())
                        && left < f32::from(bounds.right())
                        && top >= f32::from(bounds.top())
                        && top <= f32::from(bounds.bottom())
                }))
        }));
    })
    .unwrap();
    let before_copy = test
        .window
        .read_with(cx, |host, _| host.copy.original_text())
        .unwrap();
    hover(cx, &test, 0);
    let after = links(cx, &test);
    assert_eq!(before[0].text_bounds, after[0].text_bounds);
    assert_eq!(before[0].icon_bounds, after[0].icon_bounds);
    assert_eq!(before_copy, source);
    assert_eq!(
        test.window
            .read_with(cx, |host, _| host.copy.original_text())
            .unwrap(),
        source
    );
    cx.update_window(test.window.into(), |_, window, _| {
        let scale = window.scale_factor();
        let mut dashes = window
            .painted_quads()
            .into_iter()
            .filter(|quad| {
                let width = quad.bounds.size.width.0 / scale;
                let height = quad.bounds.size.height.0 / scale;
                height <= 1.01 && width > 0. && width <= 2.01
            })
            .collect::<Vec<_>>();
        assert!(dashes.len() >= 3);
        dashes.sort_by(|left, right| {
            left.bounds
                .top()
                .0
                .total_cmp(&right.bounds.top().0)
                .then_with(|| left.bounds.left().0.total_cmp(&right.bounds.left().0))
        });
        let same_row = dashes
            .windows(2)
            .filter(|pair| (pair[0].bounds.top().0 - pair[1].bounds.top().0).abs() < 0.01)
            .collect::<Vec<_>>();
        assert!(!same_row.is_empty());
        for pair in same_row {
            let gap = (pair[1].bounds.left().0 - pair[0].bounds.right().0) / scale;
            assert!((gap - 2.).abs() < 0.02, "dash gap: {gap}");
        }
        for dash in &dashes {
            let left = dash.bounds.left().0 / scale;
            let right = dash.bounds.right().0 / scale;
            let top = dash.bounds.top().0 / scale;
            assert!(after[0].text_bounds.iter().any(|bounds| left
                >= f32::from(bounds.left()) - 0.01
                && right <= f32::from(bounds.right()) + 0.01
                && top >= f32::from(bounds.top())
                && top <= f32::from(bounds.bottom())));
        }
    })
    .unwrap();
    move_pointer(cx, &test, point(px(590.), px(490.)));
    cx.update_window(test.window.into(), |_, window, cx| {
        window.focus(&after[0].focus, cx)
    })
    .unwrap();
    tick(cx, &test, 0);
    assert_eq!(before[0].text_bounds, links(cx, &test)[0].text_bounds);
    cx.update_window(test.window.into(), |_, window, _| {
        let scale = window.scale_factor();
        assert!(
            window
                .painted_quads()
                .iter()
                .filter(|quad| {
                    quad.bounds.size.height.0 / scale <= 1.01
                        && quad.bounds.size.width.0 / scale > 0.
                        && quad.bounds.size.width.0 / scale <= 2.01
                })
                .count()
                >= 3
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn hover_delay_card_entry_leave_delay_and_escape_suppression_use_the_same_window_card(
    cx: &mut TestAppContext,
) {
    let test = harness(cx, HostKind::Ordinary, &format!("[Target]({TARGET})"));
    test.network.respond(REQUEST, reply());
    let composer = test
        .window
        .read_with(cx, |host, cx| host.composer.read(cx).focus_handle(cx))
        .unwrap();
    cx.update_window(test.window.into(), |_, window, cx| {
        window.focus(&composer, cx)
    })
    .unwrap();
    hover(cx, &test, 0);
    tick(cx, &test, 399);
    assert!(!card_visible(cx, &test));
    assert!(test.network.requests().is_empty());
    tick(cx, &test, 1);
    assert!(card_visible(cx, &test));
    assert_eq!(test.network.requests().len(), 1);
    cx.update_window(test.window.into(), |_, window, cx| {
        assert!(composer.is_focused(window));
        assert_eq!(
            window.find("link-preview-title").label(),
            Some("Real target title")
        );
        assert_eq!(
            window.find("link-preview-description").label(),
            Some("Real target summary")
        );
        assert_eq!(window.find("link-preview-host").label(), Some("github.com"));
        window.hover("markdown-link-preview", cx);
    })
    .unwrap();
    tick(cx, &test, 200);
    assert!(card_visible(cx, &test));
    move_pointer(cx, &test, point(px(590.), px(490.)));
    tick(cx, &test, 149);
    assert!(card_visible(cx, &test));
    tick(cx, &test, 1);
    assert!(!card_visible(cx, &test));
    hover(cx, &test, 0);
    tick(cx, &test, 400);
    assert!(card_visible(cx, &test));
    cx.simulate_keystrokes(test.window.into(), "escape");
    tick(cx, &test, 800);
    assert!(!card_visible(cx, &test));
    move_pointer(cx, &test, point(px(590.), px(490.)));
    hover(cx, &test, 0);
    tick(cx, &test, 400);
    assert!(card_visible(cx, &test));
    assert_eq!(test.network.requests().len(), 1);
    assert!(test.opened.borrow().is_empty());
}

#[gpui_kit::test]
fn moving_between_wrapped_fragments_keeps_the_original_dwell_and_quick_passes_do_not_fetch(
    cx: &mut TestAppContext,
) {
    let test = harness(
        cx,
        HostKind::Ordinary,
        &format!("[long long long long long long long long long link]({TARGET})"),
    );
    test.network.respond(REQUEST, reply());
    let regions = links(cx, &test)[0].text_bounds.clone();
    assert!(regions.len() > 1);
    move_pointer(cx, &test, regions[0].center());
    tick(cx, &test, 250);
    move_pointer(cx, &test, regions[1].center());
    tick(cx, &test, 149);
    assert!(!card_visible(cx, &test));
    tick(cx, &test, 1);
    assert!(card_visible(cx, &test));
    assert_eq!(test.network.requests().len(), 1);
    let quick = harness(cx, HostKind::Ordinary, &format!("[Quick]({TARGET})"));
    hover(cx, &quick, 0);
    tick(cx, &quick, 250);
    move_pointer(cx, &quick, point(px(590.), px(490.)));
    tick(cx, &quick, 500);
    assert!(quick.network.requests().is_empty());
    assert!(!card_visible(cx, &quick));
}

fn press_cycle(
    cx: &mut TestAppContext,
    test: &Harness,
    down: Point<Pixels>,
    up: Point<Pixels>,
    count: usize,
    button: MouseButton,
    modifiers: Modifiers,
) {
    cx.update_window(test.window.into(), |_, window, cx| {
        window.dispatch_event(
            MouseDownEvent {
                position: down,
                button,
                click_count: count,
                modifiers,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseMoveEvent {
                position: up,
                pressed_button: Some(button),
                modifiers,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                position: up,
                button,
                click_count: count,
                modifiers,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn icon_and_text_clicks_open_once_after_arbitration_and_other_gestures_do_not_open(
    cx: &mut TestAppContext,
) {
    for icon in [false, true] {
        let test = harness(cx, HostKind::Ordinary, &format!("[Target]({TARGET})"));
        let link = &links(cx, &test)[0];
        let position = if icon {
            link.icon_bounds[0].center()
        } else {
            link.text_bounds[0].center()
        };
        press_cycle(
            cx,
            &test,
            position,
            position + point(px(1.), px(0.)),
            1,
            MouseButton::Left,
            Modifiers::default(),
        );
        tick(cx, &test, 499);
        assert!(test.opened.borrow().is_empty());
        tick(cx, &test, 1);
        assert_eq!(*test.opened.borrow(), [TARGET]);
        tick(cx, &test, 1000);
        assert_eq!(test.opened.borrow().len(), 1);
    }
    for (count, button, shift, dx) in [
        (1, MouseButton::Left, false, 5.),
        (2, MouseButton::Left, false, 0.),
        (3, MouseButton::Left, false, 0.),
        (1, MouseButton::Right, false, 0.),
        (1, MouseButton::Left, true, 0.),
    ] {
        let menus = Rc::new(RefCell::new(Vec::new()));
        let observed_menus = menus.clone();
        let menu_subscription = cx.update(|cx| {
            cx.observe_new::<PopupMenu>(move |_, _, cx| {
                observed_menus.borrow_mut().push(cx.entity().downgrade())
            })
        });
        let test = harness(
            cx,
            HostKind::Selectable,
            &format!("[Target label]({TARGET})"),
        );
        let position = links(cx, &test)[0].text_bounds[0].center();
        press_cycle(
            cx,
            &test,
            position,
            position + point(px(dx), px(0.)),
            count,
            button,
            Modifiers {
                shift,
                ..Default::default()
            },
        );
        tick(cx, &test, 1000);
        assert!(
            test.opened.borrow().is_empty(),
            "count={count}, shift={shift}, dx={dx}"
        );
        assert!(!card_visible(cx, &test));
        if button == MouseButton::Right {
            cx.update_window(test.window.into(), |_, window, _| {
                assert!(window.find("popup-menu").visible())
            })
            .unwrap();
            cx.simulate_keystrokes(test.window.into(), "escape");
            tick(cx, &test, 0);
            cx.update_window(test.window.into(), |_, window, _| {
                assert!(window.try_find("popup-menu").is_none())
            })
            .unwrap();
            assert!(test.opened.borrow().is_empty());
            cx.update_window(test.window.into(), |_, window, _| window.remove_window())
                .unwrap();
            cx.run_until_parked();
            cx.update(|cx| {
                for weak in menus.borrow().iter() {
                    if let Some(menu) = weak.upgrade() {
                        menu.update(cx, |_, cx| cx.emit(gpui_kit::DismissEvent));
                    }
                }
            });
            cx.run_until_parked();
            assert!(menus.borrow().iter().all(|menu| menu.upgrade().is_none()));
        }
        drop(menu_subscription);
    }
}

#[gpui_kit::test]
fn selection_copies_original_label_bytes_and_never_the_icon_or_preview(cx: &mut TestAppContext) {
    let test = harness(
        cx,
        HostKind::Selectable,
        &format!("[选择 **中文** code]({TARGET})"),
    );
    let link = links(cx, &test).remove(0);
    let from = link.text_bounds.first().unwrap().origin + point(px(0.1), px(4.));
    let last = link.text_bounds.last().unwrap();
    let to = point(last.right() - px(0.1), last.bottom() - px(4.));
    cx.update_window(test.window.into(), |_, window, cx| {
        window.drag(from, to, cx)
    })
    .unwrap();
    tick(cx, &test, 1000);
    cx.update_window(test.window.into(), |_, window, cx| {
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "选择 中文 code"
        );
    })
    .unwrap();
    let copy = test
        .window
        .read_with(cx, |host, _| host.copy.clone())
        .unwrap();
    cx.update(|cx| copy.copy_selected_text(cx));
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some("选择 中文 code")
    );
    assert_eq!(
        test.window
            .read_with(cx, |host, _| host.copy.original_text())
            .unwrap(),
        format!("[选择 **中文** code]({TARGET})")
    );
    assert!(test.opened.borrow().is_empty());
    assert!(!card_visible(cx, &test));
}

#[gpui_kit::test]
fn changed_text_cancels_pending_click_but_preserves_the_same_preview_occurrence(
    cx: &mut TestAppContext,
) {
    let source = format!("[Target]({TARGET}) trailing");
    let test = harness(cx, HostKind::Ordinary, &source);
    test.network.respond(REQUEST, reply());
    let position = links(cx, &test)[0].text_bounds[0].center();
    press_cycle(
        cx,
        &test,
        position,
        position,
        1,
        MouseButton::Left,
        Modifiers::default(),
    );
    test.window
        .update(cx, |host, _, cx| {
            host.replace_source(&format!("{source} streamed"), cx)
        })
        .unwrap();
    tick(cx, &test, 500);
    assert!(test.opened.borrow().is_empty());
    move_pointer(cx, &test, point(px(590.), px(490.)));
    hover(cx, &test, 0);
    tick(cx, &test, 400);
    assert!(card_visible(cx, &test));
    test.window
        .update(cx, |host, _, cx| {
            host.replace_source(&format!("{source} further streamed"), cx)
        })
        .unwrap();
    tick(cx, &test, 0);
    assert!(card_visible(cx, &test));
    assert_eq!(test.network.requests().len(), 1);
}

#[gpui_kit::test]
fn stale_completions_cannot_replace_a_new_target_or_reopen_after_close(cx: &mut TestAppContext) {
    let second = "https://example.org/second";
    let test = harness(
        cx,
        HostKind::Ordinary,
        &format!("[A]({TARGET}) [B]({second})"),
    );
    let gate = test
        .network
        .gated(REQUEST, Reply::html("<head><title>Late A</title></head>"));
    test.network
        .respond(second, Reply::html("<head><title>Current B</title></head>"));
    hover(cx, &test, 0);
    tick(cx, &test, 400);
    hover(cx, &test, 1);
    tick(cx, &test, 400);
    let _ = gate.send(());
    tick(cx, &test, 0);
    cx.update_window(test.window.into(), |_, window, _| {
        assert_eq!(window.find("link-preview-title").label(), Some("Current B"))
    })
    .unwrap();
    cx.simulate_keystrokes(test.window.into(), "escape");
    tick(cx, &test, 2000);
    assert!(!card_visible(cx, &test));
    assert!(test.opened.borrow().is_empty());
    let pending = harness(cx, HostKind::Ordinary, &format!("[Pending]({TARGET})"));
    let gate = pending.network.gated(REQUEST, reply());
    hover(cx, &pending, 0);
    tick(cx, &pending, 400);
    assert!(card_visible(cx, &pending));
    assert_eq!(pending.network.requests().len(), 1);
    cx.simulate_keystrokes(pending.window.into(), "escape");
    tick(cx, &pending, 0);
    assert!(!card_visible(cx, &pending));
    assert!(pending.network.request_cancelled(0));
    let _ = gate.send(());
    tick(cx, &pending, 1000);
    assert!(!card_visible(cx, &pending));
    assert!(pending.opened.borrow().is_empty());
}

#[gpui_kit::test]
fn scroll_scope_change_unmount_external_click_and_window_deactivation_close_immediately(
    cx: &mut TestAppContext,
) {
    for reason in [
        "wheel",
        "programmatic",
        "scope",
        "unmount",
        "outside",
        "deactivate",
    ] {
        let test = harness(
            cx,
            HostKind::Ordinary,
            &format!("[Target]({TARGET})\n\n{}", "tail\n\n".repeat(40)),
        );
        test.network.respond(REQUEST, reply());
        hover(cx, &test, 0);
        tick(cx, &test, 400);
        assert!(card_visible(cx, &test), "{reason}");
        match reason {
            "wheel" => cx
                .update_window(test.window.into(), |_, window, cx| {
                    window.scroll(
                        "body-frame",
                        ScrollDelta::Pixels(point(px(0.), px(-30.))),
                        cx,
                    )
                })
                .unwrap(),
            "programmatic" => test
                .window
                .update(cx, |host, _, cx| {
                    host.scroll.set_offset(point(px(0.), px(-30.)));
                    cx.notify();
                })
                .unwrap(),
            "scope" => test
                .window
                .update(cx, |host, _, cx| {
                    host.scope = "reading-session-b".into();
                    cx.notify();
                })
                .unwrap(),
            "unmount" => test
                .window
                .update(cx, |host, _, cx| {
                    host.show_body = false;
                    cx.notify();
                })
                .unwrap(),
            "outside" => cx
                .update_window(test.window.into(), |_, window, cx| {
                    window.click("outside", cx)
                })
                .unwrap(),
            "deactivate" => {
                VisualTestContext::from_window(test.window.into(), cx).deactivate_window()
            }
            _ => unreachable!(),
        }
        tick(cx, &test, 0);
        assert!(!card_visible(cx, &test), "{reason}");
        tick(cx, &test, 1000);
        assert!(!card_visible(cx, &test), "{reason}");
        assert!(test.opened.borrow().is_empty());
    }
}

#[gpui_kit::test]
fn failure_retry_and_open_are_independent_and_missing_fields_do_not_leave_image_space(
    cx: &mut TestAppContext,
) {
    let test = harness(
        cx,
        HostKind::Ordinary,
        &format!("[Label is not page title]({TARGET})"),
    );
    test.network.fail(
        REQUEST,
        vega_conversation::types::LinkPreviewFailure::Network,
    );
    hover(cx, &test, 0);
    tick(cx, &test, 400);
    cx.update_window(test.window.into(), |_, window, cx| {
        assert!(window.find("link-preview-retry").visible());
        assert!(window.find("link-preview-open").visible());
        assert!(window.try_find("link-preview-image").is_none());
        window.click("link-preview-open", cx);
    })
    .unwrap();
    assert_eq!(*test.opened.borrow(), [TARGET]);
    test.network.respond(
        REQUEST,
        Reply::html("<head></head><body><h1>Not a title</h1></body>"),
    );
    cx.update_window(test.window.into(), |_, window, cx| {
        window.click("link-preview-retry", cx)
    })
    .unwrap();
    tick(cx, &test, 0);
    cx.update_window(test.window.into(), |_, window, _| {
        assert!(window.try_find("link-preview-retry").is_none());
        assert_eq!(window.find("link-preview-title").label(), Some(TARGET));
        assert!(window.try_find("link-preview-description").is_none());
        assert!(window.try_find("link-preview-image").is_none());
        assert!(window.find("link-preview-open").visible());
    })
    .unwrap();
    assert_eq!(test.network.requests().len(), 2);
    assert_eq!(test.opened.borrow().len(), 1);
}

#[gpui_kit::test]
fn tab_enter_open_retry_escape_and_unmount_preserve_nonmodal_focus_rules(cx: &mut TestAppContext) {
    let test = harness(
        cx,
        HostKind::Selectable,
        &format!("[First]({TARGET}) [Second](mailto:reader@example.org)"),
    );
    test.network.fail(
        REQUEST,
        vega_conversation::types::LinkPreviewFailure::Network,
    );
    let observations = links(cx, &test);
    let (reading, composer) = test
        .window
        .read_with(cx, |host, cx| {
            (
                host.reading.clone(),
                host.composer.read(cx).focus_handle(cx),
            )
        })
        .unwrap();
    cx.update_window(test.window.into(), |_, window, cx| {
        window.focus(&reading, cx);
        window.press("tab", cx);
        assert!(observations[0].focus.is_focused(window));
    })
    .unwrap();
    tick(cx, &test, 400);
    assert!(card_visible(cx, &test));
    cx.update_window(test.window.into(), |_, window, cx| {
        window.press("tab", cx);
        assert_eq!(window.find("link-preview-open").focused(), Some(true));
        window.press("enter", cx);
    })
    .unwrap();
    assert_eq!(*test.opened.borrow(), [TARGET]);
    cx.simulate_keystrokes(test.window.into(), "tab");
    cx.update_window(test.window.into(), |_, window, _| {
        assert_eq!(window.find("link-preview-retry").focused(), Some(true))
    })
    .unwrap();
    test.network.respond(REQUEST, reply());
    cx.simulate_keystrokes(test.window.into(), "enter");
    tick(cx, &test, 0);
    cx.simulate_keystrokes(test.window.into(), "escape");
    tick(cx, &test, 400);
    assert!(!card_visible(cx, &test));
    cx.update_window(test.window.into(), |_, window, _| {
        assert!(observations[0].focus.is_focused(window))
    })
    .unwrap();
    cx.simulate_keystrokes(test.window.into(), "tab");
    cx.update_window(test.window.into(), |_, window, _| {
        assert!(observations[1].focus.is_focused(window))
    })
    .unwrap();
    cx.simulate_keystrokes(test.window.into(), "enter");
    assert_eq!(
        test.opened.borrow().last().map(String::as_str),
        Some("mailto:reader@example.org")
    );
    cx.simulate_keystrokes(test.window.into(), "shift-tab");
    tick(cx, &test, 400);
    test.window
        .update(cx, |host, _, cx| {
            host.show_body = false;
            cx.notify();
        })
        .unwrap();
    tick(cx, &test, 0);
    cx.update_window(test.window.into(), |_, window, cx| {
        assert!(reading.is_focused(window));
        assert!(!composer.is_focused(window));
        window.press("tab", cx);
        assert!(!observations[0].focus.is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn preview_is_window_scoped_clamped_and_has_accessible_link_names_without_icon_focus(
    cx: &mut TestAppContext,
) {
    let first = harness(cx, HostKind::File, &format!("[Readable label]({TARGET})"));
    first.network.respond(REQUEST, reply());
    hover(cx, &first, 0);
    tick(cx, &first, 400);
    cx.update_window(first.window.into(), |_, window, cx| {
        let card = window.find("markdown-link-preview").bounds();
        assert!(card.left() >= px(12.) && card.right() <= window.viewport_size().width - px(12.));
        assert!(card.top() >= px(12.) && card.bottom() <= window.viewport_size().height - px(12.));
        let target = visible_links(window, cx).remove(0);
        assert_eq!(target.label, "Readable label");
        assert_eq!(target.target, TARGET);
        assert_eq!(target.role, gpui_kit::Role::Link);
        assert_eq!(target.description, TARGET);
    })
    .unwrap();
    let second = harness(cx, HostKind::Ordinary, "No links here");
    assert!(!card_visible(cx, &second));
    assert!(second.network.requests().is_empty());
    assert!(first.opened.borrow().is_empty());
}

#[gpui_kit::test]
fn a_pending_single_click_is_cancelled_by_second_click_blur_unmount_or_deactivation(
    cx: &mut TestAppContext,
) {
    for reason in ["second-click", "blur", "unmount", "deactivate"] {
        let test = harness(cx, HostKind::Ordinary, &format!("[Target]({TARGET})"));
        let position = links(cx, &test)[0].text_bounds[0].center();
        press_cycle(
            cx,
            &test,
            position,
            position,
            1,
            MouseButton::Left,
            Modifiers::default(),
        );
        tick(cx, &test, 200);
        match reason {
            "second-click" => press_cycle(
                cx,
                &test,
                position,
                position,
                2,
                MouseButton::Left,
                Modifiers::default(),
            ),
            "blur" => {
                let focus = test
                    .window
                    .read_with(cx, |host, cx| host.composer.read(cx).focus_handle(cx))
                    .unwrap();
                cx.update_window(test.window.into(), |_, window, cx| window.focus(&focus, cx))
                    .unwrap();
            }
            "unmount" => test
                .window
                .update(cx, |host, _, cx| {
                    host.show_body = false;
                    cx.notify();
                })
                .unwrap(),
            "deactivate" => {
                VisualTestContext::from_window(test.window.into(), cx).deactivate_window()
            }
            _ => unreachable!(),
        }
        tick(cx, &test, 700);
        assert!(test.opened.borrow().is_empty(), "{reason}");
    }
    let test = harness(
        cx,
        HostKind::Ordinary,
        &format!("[A]({TARGET}) [B](https://example.org/other)"),
    );
    let observations = links(cx, &test);
    press_cycle(
        cx,
        &test,
        observations[0].text_bounds[0].center(),
        observations[1].text_bounds[0].center(),
        1,
        MouseButton::Left,
        Modifiers::default(),
    );
    tick(cx, &test, 700);
    assert!(test.opened.borrow().is_empty());
}

#[gpui_kit::test]
fn successful_image_content_is_rendered_in_ratio_and_omits_an_absent_summary(
    cx: &mut TestAppContext,
) {
    let test = harness(cx, HostKind::Ordinary, &format!("[Target]({TARGET})"));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(40, 20)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    test.network.respond(
        REQUEST,
        Reply::html(
            "<head><title>With image</title><meta property=og:image content='/cover.png'></head>",
        ),
    );
    test.network.respond(
        "https://github.com/cover.png",
        Reply::bytes("image/png", bytes.into_inner()),
    );
    hover(cx, &test, 0);
    tick(cx, &test, 400);
    cx.update_window(test.window.into(), |_, window, _| {
        assert_eq!(
            window.find("link-preview-title").label(),
            Some("With image")
        );
        assert!(window.try_find("link-preview-description").is_none());
        let image = window.find("link-preview-image").bounds();
        assert!(image.size.height > px(0.));
        assert!((f32::from(image.size.width) / f32::from(image.size.height) - 2.).abs() < 0.02);
        assert!(window.find("link-preview-open").visible());
    })
    .unwrap();
    assert!(test.opened.borrow().is_empty());
}

#[gpui_kit::test]
fn blocked_login_nonweb_and_mail_cards_keep_open_without_an_automatic_retry_loop(
    cx: &mut TestAppContext,
) {
    for (target, response) in [
        ("https://127.0.0.1/private", None),
        ("https://github.com@example.org/private", None),
        (
            "https://example.org/login",
            Some(Reply::html("<head><title>Sign in</title></head>").status(401)),
        ),
        (
            "https://example.org/document.pdf",
            Some(Reply::bytes("application/pdf", b"%PDF".to_vec())),
        ),
        ("mailto:reader@example.org", None),
    ] {
        let test = harness(cx, HostKind::Ordinary, &format!("[Label]({target})"));
        if let Some(response) = response {
            test.network.respond(target, response);
        }
        hover(cx, &test, 0);
        tick(cx, &test, 400);
        cx.update_window(test.window.into(), |_, window, cx| {
            assert!(window.find("link-preview-open").visible());
            assert!(window.try_find("link-preview-retry").is_none());
            assert!(window.try_find("link-preview-image").is_none());
            assert!(window.try_find("link-preview-description").is_none());
            window.click("link-preview-open", cx);
        })
        .unwrap();
        assert_eq!(*test.opened.borrow(), [target]);
        tick(cx, &test, 2000);
        assert_eq!(
            test.network.requests().len(),
            if matches!(
                target,
                "https://example.org/login" | "https://example.org/document.pdf"
            ) {
                1
            } else {
                0
            }
        );
    }
}

#[gpui_kit::test]
fn long_address_details_are_keyboard_accessible_and_tab_can_leave_the_nonmodal_card(
    cx: &mut TestAppContext,
) {
    let target = format!(
        "https://example.org/{}?keep=1#fragment",
        "long-address-".repeat(25)
    );
    let request = target.split('#').next().unwrap();
    let test = harness(
        cx,
        HostKind::Ordinary,
        &format!("[Target]({target}) [Next](mailto:next@example.org)"),
    );
    test.network.respond(request, reply());
    let observations = links(cx, &test);
    cx.update_window(test.window.into(), |_, window, cx| {
        window.focus(&observations[0].focus, cx)
    })
    .unwrap();
    tick(cx, &test, 400);
    cx.simulate_keystrokes(test.window.into(), "tab tab");
    cx.update_window(test.window.into(), |_, window, cx| {
        assert_eq!(
            window.find("link-preview-address-toggle").focused(),
            Some(true)
        );
        window.press("enter", cx);
        assert_eq!(
            window.find("link-preview-address").label(),
            Some(target.as_str())
        );
        let card = window.find("markdown-link-preview").bounds();
        assert!(card.size.height <= px(420.));
        assert!(card.right() <= window.viewport_size().width - px(12.));
    })
    .unwrap();
    cx.simulate_keystrokes(test.window.into(), "tab");
    cx.update_window(test.window.into(), |_, window, _| {
        assert!(observations[1].focus.is_focused(window))
    })
    .unwrap();
    assert!(test.opened.borrow().is_empty());
}

#[gpui_kit::test]
fn relative_unsupported_and_code_targets_stay_plain_without_icons_focus_or_requests(
    cx: &mut TestAppContext,
) {
    let source = "[relative](./page.md) [anchor](#section) [unsafe](javascript:alert) [file](file:///tmp/document) `https://example.org/inline`\n\n```text\nhttps://example.org/fenced\n```";
    for kind in [
        HostKind::Ordinary,
        HostKind::Selectable,
        HostKind::File,
        HostKind::Thinking,
    ] {
        let test = harness(cx, kind, source);
        assert!(links(cx, &test).is_empty(), "{kind:?}");
        cx.update_window(test.window.into(), |_, window, cx| {
            window.hover("body-frame", cx)
        })
        .unwrap();
        tick(cx, &test, 1000);
        assert!(!card_visible(cx, &test));
        assert!(test.network.requests().is_empty());
        assert!(test.network.dns_calls().is_empty());
        assert!(test.opened.borrow().is_empty());
    }
}
