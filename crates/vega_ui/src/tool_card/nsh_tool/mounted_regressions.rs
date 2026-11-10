use super::*;
use gpui_kit::{Bounds, Pixels, Point};
use std::sync::Mutex;

type NativeBounds = nsh_core::geometry::Bounds<nsh_core::geometry::Points>;

#[derive(Clone)]
pub(crate) struct Observed {
    snapshot: Snapshot,
    bounds: NativeBounds,
}

static CONTROLS: Mutex<Vec<Observed>> = Mutex::new(Vec::new());

fn same_snapshot(observed: &Snapshot, current: &Snapshot) -> bool {
    observed.id.matches(&current.id)
        && observed.same_content(current)
        && match (&observed.layout, &current.layout) {
            (Some(observed), Some(current)) => observed.matches(current),
            _ => false,
        }
}

pub(super) fn measure_control(
    state: Signal<Snapshot>,
    target: nsh_core::prelude::ElementRef,
    observed: Snapshot,
) {
    nsh_core::prelude::spawn(async move {
        let Some(measurement) = target.measure().await else {
            return;
        };
        if !same_snapshot(&observed, &state.get_untracked()) {
            return;
        }
        let bounds = NativeBounds {
            origin: measurement.page,
            size: measurement.frame.size,
        };
        if ![
            bounds.origin.x.0,
            bounds.origin.y.0,
            bounds.size.width.0,
            bounds.size.height.0,
        ]
        .iter()
        .all(|value| value.is_finite())
            || bounds.size.width.0 <= 0.
            || bounds.size.height.0 <= 0.
        {
            return;
        }
        let mut controls = CONTROLS.lock().unwrap();
        controls.retain(|control| !control.snapshot.id.matches(&observed.id));
        controls.push(Observed {
            snapshot: observed,
            bounds,
        });
        if controls.len() > 64 {
            controls.remove(0);
        }
    });
}

pub(crate) fn observe(card: &ToolCard, cx: &App) -> Option<Observed> {
    let page = card.presentation.as_ref()?;
    assert!(page.surface.errors().borrow().is_none());
    let current = card.nsh_snapshot(
        page.id.clone(),
        page.headline.as_ref(),
        theme(cx).colors,
        cx.should_auto_hide_scrollbars(),
    );
    let observation = CONTROLS
        .lock()
        .unwrap()
        .iter()
        .find(|control| same_snapshot(&control.snapshot, &current))
        .cloned()?;
    assert!(observation.bounds.size.width.0 > 0.);
    assert!(observation.bounds.size.height.0 > 0.);
    card.measured_size(cx)?;
    let texts = page
        .surface
        .entity()
        .read(cx)
        .integration_text_observation()?;
    let control = current.shell_control.as_ref()?;
    if !texts.iter().any(|text| text.text == control.label)
        || current
            .notice
            .as_ref()
            .is_some_and(|notice| !texts.iter().any(|text| &text.text == notice))
    {
        return None;
    }
    Some(observation)
}

pub(crate) fn diagnostic(card: &ToolCard, cx: &App) -> String {
    let Some(page) = &card.presentation else {
        return "presentation=absent".into();
    };
    let current = card.nsh_snapshot(
        page.id.clone(),
        page.headline.as_ref(),
        theme(cx).colors,
        cx.should_auto_hide_scrollbars(),
    );
    let same_layout = |snapshot: &Snapshot| match (&snapshot.layout, &current.layout) {
        (Some(observed), Some(current)) => observed.matches(current),
        _ => false,
    };
    let records = CONTROLS
        .lock()
        .unwrap()
        .iter()
        .rev()
        .take(8)
        .map(|control| {
            (
                control.snapshot.id.matches(&current.id),
                control.snapshot.same_content(&current),
                same_layout(&control.snapshot),
                control.bounds,
            )
        })
        .collect::<Vec<_>>();
    let texts = page
        .surface
        .entity()
        .read(cx)
        .integration_text_observation()
        .map(|texts| {
            texts
                .into_iter()
                .take(8)
                .map(|text| {
                    (
                        text.node,
                        text.text.chars().take(160).collect::<String>(),
                        text.frame,
                        text.content,
                        text.lines.len(),
                    )
                })
                .collect::<Vec<_>>()
        });
    format!(
        "presentation=present surface_error={:?} mounted_width={:?} measured_size={:?} published_id_current={} published_content_current={} published_layout_current={} current_layout_width={:?} current_control={:?} current_notice={:?} control_records(id,content,layout,bounds)={records:?} current_native_texts={texts:?}",
        page.surface.errors().borrow().as_ref(),
        page.mount.upgrade().map(|mount| mount.borrow().width),
        card.measured_size(cx),
        page.observed.id.matches(&current.id),
        page.observed.same_content(&current),
        same_layout(&page.observed),
        current.layout.as_ref().map(|layout| layout.width),
        current.shell_control.as_ref().map(|control| (
            &control.operation,
            &control.label,
            control.enabled
        )),
        current.notice,
    )
}

impl Observed {
    pub(crate) fn position(&self, host: Bounds<Pixels>) -> Point<Pixels> {
        host.origin
            + gpui_kit::point(
                px(self.bounds.origin.x.0 + self.bounds.size.width.0 / 2.),
                px(self.bounds.origin.y.0 + self.bounds.size.height.0 / 2.),
            )
    }

    pub(crate) fn assert_control(&self, key: &'static str, enabled: bool) {
        let control = self.snapshot.shell_control.as_ref().unwrap();
        assert_eq!(control.label, vega_i18n::t(key));
        assert_eq!(control.enabled, enabled);
        assert!(!self.snapshot.expanded);
    }

    pub(crate) fn assert_failure(&self, failed: bool) {
        assert_eq!(self.snapshot.control_failed, failed);
        if failed {
            assert_eq!(
                self.snapshot.notice.as_deref(),
                Some(vega_i18n::t("ui.shell_session.control_failed")),
            );
            assert!(
                self.snapshot
                    .label
                    .contains(vega_i18n::t("ui.shell_session.control_failed"))
            );
        }
    }
}

pub(crate) fn reject(card: &mut ToolCard, observed: &Observed, cx: &mut Context<ToolCard>) {
    let (reply, mut response) = oneshot::channel();
    card.nsh_request(
        Request::ShellControl {
            observed: Box::new(observed.snapshot.clone()),
            reply,
        },
        cx,
    );
    assert!(!response.try_recv().unwrap());
}

pub(crate) fn changed(card: &ToolCard, observed: &Observed, cx: &App) -> bool {
    card.presentation.as_ref().is_some_and(|page| {
        !observed.snapshot.id.matches(&page.id)
            || !observed.snapshot.same_content(&card.nsh_snapshot(
                page.id.clone(),
                page.headline.as_ref(),
                theme(cx).colors,
                cx.should_auto_hide_scrollbars(),
            ))
    })
}

pub(crate) fn reject_wrong_operation(
    card: &mut ToolCard,
    observed: &Observed,
    cx: &mut Context<ToolCard>,
) {
    let mut altered = observed.clone();
    altered.snapshot.shell_control.as_mut().unwrap().operation = "different-operation".into();
    reject(card, &altered, cx);
}

pub(crate) fn assert_pending(card: &ToolCard, pending: bool, failed: bool) {
    assert_eq!(card.shell_control_pending, pending);
    assert_eq!(card.shell_control_failed, failed);
}

pub(crate) fn stop(card: &mut ToolCard, cx: &mut Context<ToolCard>) {
    if let Some(page) = card.presentation.take() {
        page.surface.entity().update(cx, |view, _| view.stop());
    }
}
