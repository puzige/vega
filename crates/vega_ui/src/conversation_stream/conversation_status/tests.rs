use super::*;
use nsh_core::{ApproxTextEngine, Core, EventKind, MountItem, element::JsEvent};
use std::cell::RefCell;
use vega_theme::Theme;

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static UPDATES: RefCell<Option<mpsc::Sender<Snapshot>>> = const { RefCell::new(None) };
    static REPORTS: RefCell<Option<mpsc::Receiver<Measurement>>> = const { RefCell::new(None) };
}

#[ns::component]
fn ProductionStatus() -> impl ns::IntoElement {
    let state = ns::signal_with(
        INITIAL.with(|initial| initial.borrow_mut().take().expect("snapshot")),
        ns::SignalEquals::Never,
    );
    let (updates, mut receiver) = mpsc::channel(8);
    UPDATES.with(|slot| *slot.borrow_mut() = Some(updates));
    ns::spawn(async move {
        while let Some(snapshot) = receiver.recv().await {
            state.set(snapshot);
        }
    });
    let (measurements, reports) = mpsc::channel(64);
    REPORTS.with(|slot| *slot.borrow_mut() = Some(reports));
    render(state, measurements)
}

fn label(text: &str, color: Rgba, title: bool) -> Label {
    let mut font = gpui_kit::font(".SystemUIFont");
    font.weight = if title {
        Typography::EMPTY_STATE_TITLE_WEIGHT
    } else {
        gpui_kit::FontWeight::NORMAL
    };
    Label {
        text: text.into(),
        color,
        font,
        size: if title {
            Typography::EMPTY_STATE_TITLE
        } else {
            Typography::METADATA
        },
        line_height: if title { 44. } else { 20. },
    }
}

fn snapshot(kind: Kind, width: f32, labels: Vec<Label>) -> Snapshot {
    Snapshot {
        id: ProjectionId::new(),
        input: Input { kind, labels },
        width,
    }
}

fn mounted(snapshot: Snapshot) -> (Core<ApproxTextEngine>, Vec<MountItem>) {
    let width = snapshot.width;
    let height = snapshot.input.initial_height();
    INITIAL.with(|initial| *initial.borrow_mut() = Some(snapshot));
    let mut core = Core::new(ApproxTextEngine, ns::size(ns::px(width), ns::px(height)));
    core.mount_root(&nsh_core::element::Root::of::<ProductionStatus>());
    let items = settle(&mut core);
    (core, items)
}

fn settle(core: &mut Core<ApproxTextEngine>) -> Vec<MountItem> {
    let mut items = Vec::new();
    for _ in 0..8 {
        let batch = core.commit();
        let mut events = Vec::new();
        core.take_events(&mut events);
        if batch.is_none() && events.is_empty() {
            return items;
        }
        if let Some(batch) = batch {
            items.extend(batch.items);
        }
        for event in events {
            match event {
                JsEvent::Event(id, kind, payload) => {
                    core.component_event(id, kind, &payload);
                }
                JsEvent::LayoutListener {
                    host,
                    id,
                    listener,
                    frame,
                } => core.component_layout(host, id, &listener, frame),
                _ => {}
            }
        }
        core.run_component_tasks();
    }
    panic!("status renderer did not settle");
}

fn reports() -> Vec<Measurement> {
    REPORTS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let receiver = slot.as_mut().expect("layout receiver");
        let mut reports = Vec::new();
        while let Ok(report) = receiver.try_recv() {
            reports.push(report);
        }
        reports
    })
}

fn assert_passive(items: &[MountItem]) {
    for item in items {
        match item {
            MountItem::Events { events, .. } => {
                for kind in EventKind::ALL {
                    if *kind != EventKind::Layout {
                        assert!(!events.contains(*kind), "unexpected event: {kind:?}");
                    }
                }
            }
            MountItem::ViewProps { props, .. } => {
                assert!(props.accessibility.actions.is_empty());
                assert_eq!(props.accessibility.role, nsh_core::AccessibilityRole::None);
                assert!(!props.accessibility.hidden);
            }
            _ => {}
        }
    }
}

#[test]
fn production_location_wraps_and_reports_real_height_beyond_bootstrap_viewport() {
    let source = "完整定位状态 e\u{301} 👩\u{200d}💻 long status with several words ".repeat(6);
    let initial = snapshot(
        Kind::Location,
        120.,
        vec![label(&source, Theme::light().colors.warning, false)],
    );
    let id = initial.id.clone();
    let bootstrap = initial.input.initial_height();
    let (mut core, items) = mounted(initial);
    assert_passive(&items);
    let text = items.iter().find_map(|item| match item {
        MountItem::Text { text, .. } if text.text == source => Some(text),
        _ => None,
    });
    let text = text.expect("actual production text");
    assert_eq!(text.max_lines, 0);
    assert!(!text.no_wrap);
    assert_eq!(text.align, nsh_core::TextAlign::Left);
    assert!(text.spans.iter().all(|span| {
        span.style.font_size.0 == Typography::METADATA
            && span.style.color == crate::nsh::color(Theme::light().colors.warning).into()
    }));
    assert!(items.iter().any(|item| matches!(item,
        MountItem::ViewProps { props, .. }
            if props.accessibility.label.as_deref() == Some(source.as_str())
    )));
    let measured = reports()
        .into_iter()
        .find(|report| report.accepts(&id, 120.))
        .expect("real on_layout height");
    assert!(measured.height > bootstrap * 2.);
    core.set_root_size(
        nsh_core::ROOT_ID,
        ns::size(ns::px(120.), ns::px(measured.height)),
        Default::default(),
    );
    let _ = settle(&mut core);
    assert!(reports().iter().all(|next| next.height == measured.height));
}

#[test]
fn production_location_replaces_old_text_style_and_height_after_width_change() {
    let old = "OLD source that must disappear";
    let (mut core, _) = mounted(snapshot(
        Kind::Location,
        320.,
        vec![label(old, Theme::light().colors.text_secondary, false)],
    ));
    let _ = reports();
    let source = "新状态 👩\u{200d}💻 wrapping message ".repeat(4);
    let next = snapshot(
        Kind::Location,
        100.,
        vec![label(&source, Theme::dark().colors.danger, false)],
    );
    let id = next.id.clone();
    UPDATES.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("update sender")
            .try_send(next)
            .expect("new projection");
    });
    core.set_root_size(
        nsh_core::ROOT_ID,
        ns::size(ns::px(100.), ns::px(28.)),
        Default::default(),
    );
    core.run_component_tasks();
    let items = settle(&mut core);
    assert_passive(&items);
    assert!(!items.iter().any(|item| matches!(item,
        MountItem::Text { text, .. } if text.text == old
    )));
    assert!(items.iter().any(|item| matches!(item,
        MountItem::Text { text, .. } if text.text == source
            && text.spans.iter().all(|span| span.style.color == crate::nsh::color(Theme::dark().colors.danger).into())
    )));
    assert!(items.iter().any(|item| match item {
        MountItem::ViewProps { props, .. } => {
            props.accessibility.label.as_deref() == Some(source.as_str())
        }
        MountItem::AccessibilityLabel { label, .. } => {
            label.as_deref() == Some(source.as_str())
        }
        _ => false,
    }));
    assert!(
        reports()
            .iter()
            .any(|report| report.accepts(&id, 100.) && report.height > 60.)
    );
}

#[test]
fn production_empty_keeps_title_subtitle_gap_and_expands_for_wrapping() {
    let colors = Theme::light().colors;
    let title = "标题 with several words";
    let subtitle = "副标题 complete label";
    let (_, items) = mounted(snapshot(
        Kind::Empty,
        120.,
        vec![
            label(title, colors.text_primary, true),
            label(subtitle, colors.text_secondary, false),
        ],
    ));
    assert_passive(&items);
    let mut text_ids = Vec::new();
    for (source, size, weight, color) in [
        (
            title,
            Typography::EMPTY_STATE_TITLE,
            Typography::EMPTY_STATE_TITLE_WEIGHT.0 as u16,
            colors.text_primary,
        ),
        (subtitle, Typography::METADATA, 400, colors.text_secondary),
    ] {
        let (id, text) = items
            .iter()
            .find_map(|item| match item {
                MountItem::Text { id, text, .. } if text.text == source => Some((*id, text)),
                _ => None,
            })
            .expect("production empty text");
        text_ids.push(id);
        assert_eq!(text.align, nsh_core::TextAlign::Center);
        assert_eq!(text.max_lines, 0);
        assert!(!text.no_wrap);
        assert!(text.spans.iter().all(|span| span.style.font_size.0 == size
            && span.style.font_weight == weight
            && span.style.color == crate::nsh::color(color).into()));
        assert!(items.iter().any(|item| matches!(item,
            MountItem::ViewProps { props, .. }
                if props.accessibility.label.as_deref() == Some(source)
        )));
    }
    let frames = text_ids
        .iter()
        .map(|target| {
            items
                .iter()
                .find_map(|item| match item {
                    MountItem::Frame { id, frame } if id == target => Some(*frame),
                    _ => None,
                })
                .expect("text frame")
        })
        .collect::<Vec<_>>();
    assert!(
        (frames[1].origin.y.0 - frames[0].origin.y.0 - frames[0].size.height.0 - EMPTY_GAP).abs()
            < 0.01
    );
    let measured = reports().into_iter().last().expect("empty content height");
    assert!(measured.height > EMPTY_HEIGHT);
    assert!(measured.height >= frames[1].origin.y.0 + frames[1].size.height.0);
}

#[test]
fn omitted_thinking_notice_preserves_wrapped_text_gap_and_independent_row_identity() {
    assert_ne!(
        Kind::OmittedThinking(7, 1).key(),
        Kind::OmittedThinking(7, 2).key()
    );
    assert_ne!(
        Kind::OmittedThinking(7, 1).key(),
        Kind::OmittedThinking(8, 1).key()
    );
    let source = "较早的思考内容已省略 Earlier thinking omitted ".repeat(3);
    let initial = snapshot(
        Kind::OmittedThinking(7, 1),
        120.,
        vec![label(&source, Theme::dark().colors.text_tertiary, false)],
    );
    let id = initial.id.clone();
    let bootstrap = initial.input.initial_height();
    let (_, items) = mounted(initial);
    assert_passive(&items);
    let (text_id, text) = items
        .iter()
        .find_map(|item| match item {
            MountItem::Text { id, text, .. } if text.text == source => Some((*id, text)),
            _ => None,
        })
        .expect("production omitted-thinking text");
    assert_eq!(text.max_lines, 0);
    assert!(!text.no_wrap);
    assert_eq!(text.align, nsh_core::TextAlign::Left);
    assert!(text.spans.iter().all(|span| {
        span.style.font_size.0 == Typography::METADATA
            && span.style.color == crate::nsh::color(Theme::dark().colors.text_tertiary).into()
    }));
    assert!(items.iter().any(|item| matches!(item,
        MountItem::ViewProps { props, .. }
            if props.accessibility.label.as_deref() == Some(source.as_str())
    )));
    let frame = items
        .iter()
        .filter_map(|item| match item {
            MountItem::Frame { id, frame } if *id == text_id => Some(*frame),
            _ => None,
        })
        .next_back()
        .expect("production text frame");
    assert_eq!(frame.origin.x.0, 0.);
    assert_eq!(frame.origin.y.0, Layout::PROCESS_ITEM_GAP);
    assert_eq!(frame.size.width.0, 120.);
    let measured = reports()
        .into_iter()
        .find(|report| report.accepts(&id, 120.))
        .expect("current natural omitted-thinking height");
    assert!(measured.height > bootstrap);
    assert!((measured.height - frame.size.height.0 - Layout::PROCESS_ITEM_GAP).abs() < 0.01);
}

#[test]
fn production_error_notice_keeps_full_text_color_and_natural_wrapped_height() {
    let source = "错误 原文 👩\u{200d}💻 e\u{301} retry after checking connection ".repeat(5);
    let color = Theme::dark().colors.danger;
    let mut text = label(&source, color, false);
    text.size = Typography::BODY;
    text.line_height = Typography::BODY * Typography::BODY_LINE_HEIGHT;
    let initial = snapshot(Kind::Notice, 120., vec![text]);
    let id = initial.id.clone();
    let bootstrap = initial.input.initial_height();
    let (_, items) = mounted(initial);
    assert_passive(&items);
    let text = items
        .iter()
        .find_map(|item| match item {
            MountItem::Text { text, .. } if text.text == source => Some(text),
            _ => None,
        })
        .expect("complete native error text");
    assert_eq!(text.max_lines, 0);
    assert!(!text.no_wrap);
    assert_eq!(text.align, nsh_core::TextAlign::Left);
    assert!(text.spans.iter().all(|span| {
        span.style.font_size.0 == Typography::BODY
            && span.style.color == crate::nsh::color(color).into()
    }));
    assert!(items.iter().any(|item| matches!(item,
        MountItem::ViewProps { props, .. }
            if props.accessibility.label.as_deref() == Some(source.as_str())
    )));
    let measured = reports()
        .into_iter()
        .find(|report| report.accepts(&id, 120.))
        .expect("current natural native error height");
    assert!(measured.height > bootstrap * 2.);
    assert!(!measured.accepts(&ProjectionId::new(), 120.));
    assert!(!measured.accepts(&id, 121.));
}

#[test]
fn compacting_notice_aligns_original_icon_to_first_line_and_measures_wrapped_label() {
    let source = "Compacting context 正在压缩上下文 ".repeat(4);
    let color = Theme::light().colors.text_secondary;
    let mut text = label(&source, color, false);
    text.size = Typography::CONVERSATION;
    text.line_height = Typography::CONVERSATION * Typography::CONVERSATION_LINE_HEIGHT;
    let line_height = text.line_height;
    let initial = snapshot(Kind::Compaction, 150., vec![text]);
    let id = initial.id.clone();
    let (_, items) = mounted(initial);
    let target = items
        .iter()
        .find_map(|item| match item {
            MountItem::Text { id, text, .. } if text.text == source => {
                assert!(!text.no_wrap);
                assert_eq!(text.max_lines, 0);
                Some(*id)
            }
            _ => None,
        })
        .expect("complete compacting label");
    let frame = items
        .iter()
        .filter_map(|item| match item {
            MountItem::Frame { id, frame } if *id == target => Some(*frame),
            _ => None,
        })
        .next_back()
        .expect("native label frame");
    let mut origin = frame.origin;
    let mut node = target;
    let mut visited = std::collections::HashSet::new();
    while let Some(parent) = items.iter().rev().find_map(|item| match item {
        MountItem::Insert { parent, child, .. } if *child == node => Some(*parent),
        _ => None,
    }) {
        assert!(visited.insert(parent), "native layout parent cycle");
        if let Some(parent_frame) = items.iter().rev().find_map(|item| match item {
            MountItem::Frame { id, frame } if *id == parent => Some(*frame),
            _ => None,
        }) {
            origin.x.0 += parent_frame.origin.x.0;
            origin.y.0 += parent_frame.origin.y.0;
        }
        node = parent;
    }
    assert!((origin.x.0 - Layout::ICON_SIZE - Layout::CONVERSATION_INLINE_GAP).abs() < 0.01);
    let padding = (Layout::CONVERSATION_TOOL_HEIGHT - line_height) / 2.;
    assert!((origin.y.0 - padding).abs() < 0.01);
    assert!(frame.size.height.0 > line_height * 2.);
    let measured = reports()
        .into_iter()
        .find(|report| report.accepts(&id, 150.))
        .expect("measured compacting content");
    assert!((measured.height - frame.size.height.0 - padding * 2.).abs() < 0.01);
    assert!(items.iter().any(|item| matches!(item,
        MountItem::ViewProps { props, .. }
            if props.accessibility.label.as_deref() == Some(source.as_str())
    )));
}

#[test]
fn measurement_rejects_old_source_width_and_invalid_dimensions() {
    let id = ProjectionId::new();
    let current = Measurement {
        id: id.clone(),
        width: 140.,
        height: 80.,
    };
    assert!(current.accepts(&id, 140.));
    assert!(!current.accepts(&ProjectionId::new(), 140.));
    assert!(!current.accepts(&id, 141.));
    for height in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(
            !Measurement {
                id: id.clone(),
                width: 140.,
                height
            }
            .accepts(&id, 140.)
        );
    }
    for width in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(
            !Measurement {
                id: id.clone(),
                width,
                height: 80.
            }
            .accepts(&id, width)
        );
    }
}
