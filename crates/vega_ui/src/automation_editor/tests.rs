use super::*;
use core::prelude::v1::test;
use std::cell::RefCell;
use std::rc::Rc;
use vega_conversation::types::AutomationStatus;

fn task() -> AutomationTask {
    AutomationTask {
        thread_id: "editor-thread".into(),
        kind: AutomationKind::Goal,
        identity: "goal".into(),
        revision: 1,
        instruction: "First line\nSecond line".into(),
        status: AutomationStatus::Paused,
        token_budget: Some(100),
        used_tokens: 12,
        usage_complete: true,
        elapsed_ms: 2000,
        interval_seconds: None,
        next_due_ms: 1000,
        expires_at_ms: None,
        no_progress: 0,
        blocker_count: 0,
        blocker: None,
        reason: None,
        updated_at_ms: 1000,
    }
}

fn open(
    cx: &mut TestAppContext,
    height: f32,
) -> (WindowHandle<AutomationEditor>, Entity<AutomationEditor>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::init(cx);
        cx.set_global(vega_theme::Theme::light());
    });
    let editor = cx.new(|cx| AutomationEditor::new(task(), cx));
    let view = editor.clone();
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(420.), px(height)),
                    cx,
                ))),
                ..Default::default()
            },
            move |_, _| view,
        )
        .unwrap()
    });
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            window.focus(&view.focus_handle(cx), cx)
        })
        .unwrap();
    (window, editor)
}

fn edit(view: &Entity<AutomationEditor>, text: &str, cx: &mut TestAppContext) {
    view.update(cx, |view, cx| {
        view.input.update(cx, |input, cx| input.set_text(text, cx))
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn multiline_keyboard_save_and_late_callback_preserve_newer_draft(cx: &mut TestAppContext) {
    let (window, view) = open(cx, 600.);
    assert!(!view.read_with(cx, |view, cx| view.can_save(cx)));
    let captured = Rc::new(RefCell::new(Vec::new()));
    let events = captured.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &AutomationEditorEvent, _| {
            if let AutomationEditorEvent::Save(submitted) = event {
                events.borrow_mut().push(submitted.clone());
            }
        })
        .detach()
    });
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        view.read_with(cx, |view, cx| view.instruction(cx).to_owned()),
        "First line\nSecond line\n"
    );
    cx.simulate_keystrokes(window.into(), "cmd-s");
    assert_eq!(captured.borrow().len(), 1);
    let submitted = captured.borrow()[0].clone();
    assert!(view.read_with(cx, |view, _| view.is_saving()));
    edit(&view, "newer input", cx);
    let saved = AutomationTask {
        revision: 2,
        instruction: submitted.instruction.clone(),
        ..task()
    };
    view.update(cx, |view, cx| {
        view.finish_save(&submitted, Ok(vec![saved.clone()]), cx)
    });
    assert_eq!(
        view.read_with(cx, |view, cx| view.instruction(cx).to_owned()),
        "newer input"
    );
    assert!(view.read_with(cx, |view, cx| view.is_dirty(cx) && view.can_save(cx)));
    view.update(cx, |view, cx| {
        view.finish_save(&submitted, Err("late duplicate".into()), cx)
    });
    assert!(view.read_with(cx, |view, _| view.error.is_none()));
    view.update(cx, |view, cx| view.revert(cx));
    assert_eq!(
        view.read_with(cx, |view, cx| view.instruction(cx).to_owned()),
        saved.instruction
    );
    assert!(!view.read_with(cx, |view, cx| view.is_dirty(cx)));
}

#[gpui_kit::test]
async fn editor_conflicts_failures_and_replaced_identity_never_discard_input(
    cx: &mut TestAppContext,
) {
    let (_, view) = open(cx, 600.);
    edit(&view, "my draft", cx);
    view.update(cx, |view, cx| view.save(cx));
    let submitted = view.read_with(cx, |view, _| view.pending.clone().unwrap());
    view.update(cx, |view, cx| {
        view.finish_save(&submitted, Err("保存失败，请重试".into()), cx)
    });
    assert_eq!(
        view.read_with(cx, |view, cx| view.instruction(cx).to_owned()),
        "my draft"
    );
    assert!(view.read_with(cx, |view, cx| view.can_save(cx)));
    let latest = AutomationTask {
        revision: 2,
        instruction: "Other window".into(),
        used_tokens: 29,
        ..task()
    };
    view.update(cx, |view, cx| {
        view.refresh(std::slice::from_ref(&latest), Some(9000), cx)
    });
    assert!(view.read_with(cx, |view, cx| view.has_conflict() && !view.can_save(cx)));
    assert_eq!(
        view.read_with(cx, |view, cx| view.instruction(cx).to_owned()),
        "my draft"
    );
    view.update(cx, |view, cx| view.revert(cx));
    assert_eq!(
        view.read_with(cx, |view, cx| view.instruction(cx).to_owned()),
        "Other window"
    );
    edit(&view, "draft retained after clear", cx);
    view.update(cx, |view, cx| view.refresh(&[], None, cx));
    assert!(!view.read_with(cx, |view, cx| view.can_save(cx)));
    let replacement = AutomationTask {
        identity: "replacement".into(),
        ..task()
    };
    view.update(cx, |view, cx| view.refresh(&[replacement], None, cx));
    assert_eq!(view.read_with(cx, |view, _| view.key().identity), "goal");
    assert_eq!(
        view.read_with(cx, |view, cx| view.instruction(cx).to_owned()),
        "draft retained after clear"
    );
    assert!(!view.read_with(cx, |view, cx| view.can_save(cx)));
}

#[gpui_kit::test]
async fn invalid_editor_content_is_visible_and_toolbar_survives_small_pane_scroll(
    cx: &mut TestAppContext,
) {
    let (window, view) = open(cx, 272.);
    for text in ["  ".to_string(), "字".repeat(5462)] {
        edit(&view, &text, cx);
        assert!(!view.read_with(cx, |view, cx| view.can_save(cx)));
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("automation-editor-error").is_some());
        assert!(visual.debug_bounds("automation-editor-save").is_some());
    }
    edit(
        &view,
        &(0..80)
            .map(|index| format!("Line {index}\n"))
            .collect::<String>(),
        cx,
    );
    let input = view.read_with(cx, |view, _| view.input.clone());
    let original = input.read_with(cx, |input, _| input.first_visible_row());
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let save = visual.debug_bounds("automation-editor-save").unwrap();
    let body = visual.debug_bounds("automation-editor-input").unwrap();
    let caret = input.read_with(cx, |input, _| input.painted_cursor_bounds().unwrap());
    let viewport = input.read_with(cx, |input, _| input.painted_viewport_bounds().unwrap());
    assert!(viewport.size.height >= px(Typography::BODY * Typography::BODY_LINE_HEIGHT));
    assert!(caret.top() >= viewport.top() && caret.bottom() <= viewport.bottom());
    assert!(caret.top() >= body.top() && caret.bottom() <= body.bottom());
    assert!(caret.bottom() <= px(272.));
    assert!(input.read_with(cx, |input, _| input.visible_rows()) < 12);
    visual.simulate_event(ScrollWheelEvent {
        position: point(body.left() + px(10.), body.top() + px(20.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(120.))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    visual.run_until_parked();
    assert!(input.read_with(cx, |input, _| input.first_visible_row()) < original);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert_eq!(visual.debug_bounds("automation-editor-save").unwrap(), save);
    assert!(f32::from(save.bottom()) <= 272.);
    assert!(view.read_with(cx, |view, cx| view.can_save(cx)));
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let body = visual.debug_bounds("automation-editor-input").unwrap();
    let caret = input.read_with(cx, |input, _| input.painted_cursor_bounds().unwrap());
    let viewport = input.read_with(cx, |input, _| input.painted_viewport_bounds().unwrap());
    assert!(caret.top() >= viewport.top() && caret.bottom() <= viewport.bottom());
    assert!(caret.top() >= body.top() && caret.bottom() <= body.bottom());
    assert!(caret.bottom() <= px(272.));
    assert!(input.read_with(cx, |input, _| input.first_visible_row()) >= original);
    visual.simulate_resize(size(px(420.), px(760.)));
    visual.run_until_parked();
    assert!(input.read_with(cx, |input, _| input.visible_rows()) >= 12);
}

#[test]
fn elapsed_formats_only_actual_execution_duration() {
    assert_eq!(format_elapsed(0), "0s");
    assert_eq!(format_elapsed(1999), "1s");
    assert_eq!(format_elapsed(65000), "1m 5s");
    assert_eq!(format_elapsed(3_720_000), "1h 2m");
}
