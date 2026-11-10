use super::*;
use core::prelude::v1::test;
use nsh_core::element::{JsEvent, Root};
use nsh_core::{AccessibilityRole, ApproxTextEngine, Core, MountItem};
use std::cell::RefCell;

thread_local! {
    static INITIAL: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    static REPORTS: RefCell<Option<mpsc::Receiver<Measurement>>> = const { RefCell::new(None) };
}

#[ns::component]
fn SeparatorFixture() -> impl ns::IntoElement {
    let snapshot = INITIAL.with(|slot| slot.borrow_mut().take().expect("separator snapshot"));
    let (sender, reports) = mpsc::channel(64);
    REPORTS.with(|slot| *slot.borrow_mut() = Some(reports));
    render(ns::signal_with(snapshot, ns::SignalEquals::Never), sender)
}

fn settle(core: &mut Core<ApproxTextEngine>) -> Vec<MountItem> {
    let mut items = Vec::new();
    for _ in 0..24 {
        core.run_component_tasks();
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
                } => {
                    core.component_layout(host, id, &listener, frame);
                }
                _ => {}
            }
        }
    }
    panic!("separator renderer did not settle");
}

#[test]
fn production_separator_preserves_localized_text_weight_center_padding_and_natural_height() {
    for (day, suffix, accessible) in [
        ("Today", " at 12:34", "Monday, November 9, 2026 at 12:34"),
        ("今天", " 12:34", "2026年11月9日 星期一 12:34"),
    ] {
        for colors in [vega_theme::LIGHT, vega_theme::DARK] {
            let snapshot = Snapshot {
                id: ProjectionId::new(),
                input: Input {
                    day: day.into(),
                    suffix: suffix.into(),
                    accessible: accessible.into(),
                    color: colors.text_tertiary,
                    family: ".SystemUIFont".into(),
                },
                width: 480.,
            };
            INITIAL.with(|slot| *slot.borrow_mut() = Some(snapshot.clone()));
            let mut core = Core::new(
                ApproxTextEngine,
                ns::size(ns::px(480.), ns::px(initial_height())),
            );
            core.mount_root(&Root::of::<SeparatorFixture>());
            let items = settle(&mut core);
            let texts = items
                .iter()
                .filter_map(|item| match item {
                    MountItem::Text { id, text, .. } => Some((*id, text)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                texts
                    .iter()
                    .map(|(_, text)| text.text.as_str())
                    .collect::<Vec<_>>(),
                vec![day, suffix]
            );
            for (index, (_, text)) in texts.iter().enumerate() {
                assert!(text.spans.iter().all(|span| {
                    span.style.font_weight == (if index == 0 { 500 } else { 400 })
                        && span.style.font_size.0 == Typography::BODY
                        && span.style.color == crate::nsh::color(colors.text_tertiary).into()
                }));
            }
            let day_frame = core.measure(texts[0].0).expect("day geometry");
            let suffix_frame = core.measure(texts[1].0).expect("suffix geometry");
            let separator = items
                .iter()
                .find_map(|item| match item {
                    MountItem::ViewProps { id, props }
                        if props.accessibility.label.as_deref() == Some(accessible) =>
                    {
                        Some(*id)
                    }
                    _ => None,
                })
                .expect("native separator root");
            let separator_frame = core.measure(separator).expect("native separator geometry");
            let surface_frame = core.measure(nsh_core::ROOT_ID);
            let left = day_frame.page.x.0;
            let right = suffix_frame.page.x.0 + suffix_frame.frame.size.width.0;
            eprintln!(
                "timestamp geometry day={day:?} suffix={suffix:?} left={left} right={right} initial_height={} day_frame={day_frame:?} suffix_frame={suffix_frame:?} separator_frame={separator_frame:?} surface_frame={surface_frame:?}",
                initial_height()
            );
            let combined_width = day_frame.frame.size.width.0 + suffix_frame.frame.size.width.0;
            let continuous_left = separator_frame.page.x.0
                + (separator_frame.frame.size.width.0 - combined_width) / 2.;
            let expected_left = (continuous_left + 0.5).floor();
            assert_eq!(
                left, expected_left,
                "day={day:?} suffix={suffix:?} continuous_left={continuous_left} day_frame={day_frame:?} suffix_frame={suffix_frame:?} separator_frame={separator_frame:?}"
            );
            assert_eq!(right, expected_left + combined_width);
            assert_eq!(
                suffix_frame.page.x.0,
                day_frame.page.x.0 + day_frame.frame.size.width.0,
                "day and suffix must remain adjacent"
            );
            assert!(
                (day_frame.page.y.0 - Layout::CONVERSATION_STAGE_TOP).abs() < 1.,
                "day={day:?} padding={} day_frame={day_frame:?} suffix_frame={suffix_frame:?} separator_frame={separator_frame:?} surface_frame={surface_frame:?}",
                Layout::CONVERSATION_STAGE_TOP
            );
            assert!(
                items
                    .iter()
                    .any(|item| matches!(item, MountItem::ViewProps { props, .. }
                if props.accessibility.label.as_deref() == Some(accessible)))
            );
            assert!(
                !items
                    .iter()
                    .any(|item| matches!(item, MountItem::ViewProps { props, .. }
                if props.accessibility.role == AccessibilityRole::Button))
            );
            let mut reports =
                REPORTS.with(|slot| slot.borrow_mut().take().expect("measurement receiver"));
            let mut accepted = Vec::new();
            while let Ok(report) = reports.try_recv() {
                if report.accepts(&snapshot) {
                    accepted.push(report);
                }
            }
            let report = accepted.last().expect("actual NSH natural layout report");
            let text_height = day_frame
                .frame
                .size
                .height
                .0
                .max(suffix_frame.frame.size.height.0);
            assert_eq!(report.height, separator_frame.frame.size.height.0);
            assert_eq!(
                report.height,
                text_height + Layout::CONVERSATION_STAGE_TOP * 2.,
                "day={day:?} actual_height={} initial_height={} day_frame={day_frame:?} suffix_frame={suffix_frame:?} separator_frame={separator_frame:?} surface_frame={surface_frame:?}",
                report.height,
                initial_height()
            );
        }
    }
}

#[test]
fn natural_measurement_rejects_replaced_presentation_width_and_invalid_dimensions() {
    let snapshot = Snapshot {
        id: ProjectionId::new(),
        input: Input {
            day: "Today".into(),
            suffix: " at 12:34".into(),
            accessible: "timestamp".into(),
            color: vega_theme::LIGHT.text_tertiary,
            family: ".SystemUIFont".into(),
        },
        width: 480.,
    };
    let report = Measurement {
        id: snapshot.id.clone(),
        width: 480.,
        height: initial_height(),
    };
    assert!(report.accepts(&snapshot));
    let mut replacement = snapshot.clone();
    replacement.id = ProjectionId::new();
    assert!(!report.accepts(&replacement));
    replacement = snapshot.clone();
    replacement.width = 240.;
    assert!(!report.accepts(&replacement));
    for height in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(
            !Measurement {
                id: snapshot.id.clone(),
                width: 480.,
                height
            }
            .accepts(&snapshot)
        );
    }
}
