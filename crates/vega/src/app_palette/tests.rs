use super::*;
use gpui_kit::{prelude::*, test::TestWindowExt};
use vega_theme::Theme;

struct Host {
    palette: AppPalette,
    focus: FocusHandle,
}

impl Host {
    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.close_requested {
            self.palette.close(window, cx);
        }
        if let Some(view) = &self.palette.view {
            window.focus(&view.read(cx).focus_handle(cx), cx);
            return;
        }
        self.palette.prior_focus = window.focused(cx);
        self.palette.entry.sync(true, true, window, cx);
        let entry = self.palette.entry.clone();
        let view = cx.new(|cx| {
            CommandPalette::new(vec![PaletteAction::ToggleSidebar], cx).with_overlay_entry(entry)
        });
        view.update(cx, |view, cx| view.apply(Ok(PaletteSearch::default()), cx));
        cx.subscribe(&view, |this, _, _: &PaletteClosed, cx| {
            this.palette.request_close(cx);
            cx.notify();
        })
        .detach();
        window.focus(&view.read(cx).focus_handle(cx), cx);
        self.palette.view = Some(view);
        cx.notify();
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.palette.close_requested {
            self.palette.close(window, cx);
        }
        self.palette
            .entry
            .sync(self.palette.view.is_some(), true, window, cx);
        div()
            .id("prior")
            .size_full()
            .track_focus(&self.focus)
            .children(self.palette.view.clone())
    }
}

fn fixture(cx: &mut TestAppContext) -> WindowHandle<Host> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(Theme::dark());
        cx.set_reduce_motion(false);
    });
    let window = cx.add_window(|_, cx| Host {
        palette: AppPalette::default(),
        focus: cx.focus_handle(),
    });
    window
        .update(cx, |host, window, cx| {
            window.activate_window();
            window.focus(&host.focus, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

fn frame(window: WindowHandle<Host>, cx: &mut TestAppContext) -> f32 {
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let color: Hsla = Theme::dark().colors.bg_elevated.into();
        let quads = window.painted_quads();
        let backdrop: Hsla = Theme::dark().colors.bg_sidebar.opacity(0.85).into();
        assert!(
            quads
                .iter()
                .filter_map(|quad| quad.background.as_solid())
                .any(|paint| {
                    (paint.h - backdrop.h).abs() < 0.0001
                        && (paint.s - backdrop.s).abs() < 0.0001
                        && (paint.l - backdrop.l).abs() < 0.0001
                        && (paint.a - backdrop.a).abs() < 0.0001
                })
        );
        quads
            .iter()
            .find_map(|quad| {
                let paint = quad.background.as_solid()?;
                (is_card(quad, window)
                    && (paint.h - color.h).abs() < 0.0001
                    && (paint.s - color.s).abs() < 0.0001
                    && (paint.l - color.l).abs() < 0.0001)
                    .then_some(paint.a)
            })
            .unwrap_or(0.)
    })
    .unwrap()
}

fn is_card(quad: &Quad, window: &Window) -> bool {
    let width = vega_theme::Layout::COMMAND_PALETTE_WIDTH.min(
        f32::from(window.viewport_size().width)
            - vega_theme::Layout::COMMAND_PALETTE_SIDE_INSET * 2.,
    );
    (quad.bounds.size.width.0 - width * window.scale_factor()).abs() < 0.01
        && (quad.bounds.origin.y.0
            - vega_theme::Layout::COMMAND_PALETTE_TOP_OFFSET * window.scale_factor())
        .abs()
            < 0.01
}

#[gpui_kit::test]
fn real_palette_card_reopens_from_owner_paint_across_view_recreation(cx: &mut TestAppContext) {
    let window = fixture(cx);
    window
        .update(cx, |host, window, cx| host.open(window, cx))
        .unwrap();
    assert_eq!(frame(window, cx), 0.);
    cx.background_executor
        .advance_clock(Duration::from_millis(45));
    let middle = frame(window, cx);
    assert!(middle > 0. && middle < 1.);
    let original = window
        .update(cx, |host, window, cx| {
            let original = host.palette.view.as_ref().unwrap().entity_id();
            host.open(window, cx);
            assert_eq!(host.palette.view.as_ref().unwrap().entity_id(), original);
            original
        })
        .unwrap();
    assert!((frame(window, cx) - middle).abs() < 0.0001);
    window
        .update(cx, |host, _, cx| {
            host.palette
                .view
                .as_ref()
                .unwrap()
                .update(cx, |_, cx| cx.emit(PaletteClosed));
        })
        .unwrap();
    window
        .update(cx, |host, window, cx| {
            assert!(host.palette.close_requested);
            host.open(window, cx);
            assert_ne!(host.palette.view.as_ref().unwrap().entity_id(), original);
            assert!(
                host.palette
                    .view
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        })
        .unwrap();
    assert!((frame(window, cx) - middle).abs() < 0.0001);
    window
        .update(cx, |host, window, cx| {
            host.palette.close(window, cx);
            assert!(host.palette.cancel.is_cancelled());
            assert!(host.palette.view.is_none());
            assert!(host.focus.is_focused(window));
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            !window
                .painted_quads()
                .iter()
                .any(|quad| is_card(quad, window))
        );
    })
    .unwrap();
    window
        .update(cx, |host, window, cx| host.open(window, cx))
        .unwrap();
    assert!((frame(window, cx) - middle).abs() < 0.0001);
    cx.background_executor
        .advance_clock(Duration::from_millis(120));
    assert!((frame(window, cx) - 1.).abs() < 0.0001);
}

#[gpui_kit::test]
fn real_palette_failure_is_immediately_opaque_and_route_invalidation_resets_entry(
    cx: &mut TestAppContext,
) {
    let window = fixture(cx);
    window
        .update(cx, |host, window, cx| host.open(window, cx))
        .unwrap();
    frame(window, cx);
    cx.background_executor
        .advance_clock(Duration::from_millis(30));
    let middle = frame(window, cx);
    assert!(middle > 0. && middle < 1.);
    window
        .update(cx, |host, _, cx| {
            host.palette.view.as_ref().unwrap().update(cx, |view, cx| {
                view.apply(Err(PaletteError::Unavailable), cx)
            });
        })
        .unwrap();
    assert!((frame(window, cx) - 1.).abs() < 0.0001);
    window
        .update(cx, |host, window, cx| {
            host.palette.close(window, cx);
            host.palette.route = Some((Some("old-project".into()), None));
            host.palette
                .invalidate_route(&(Some("new-project".into()), None));
            host.open(window, cx);
        })
        .unwrap();
    assert_eq!(frame(window, cx), 0.);
}
