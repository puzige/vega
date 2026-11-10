use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Default)]
pub(in super::super) struct Observed {
    pub(in super::super) surfaces: Vec<WeakEntity<nsh_platform_gpui::AppView>>,
    pub(in super::super) stops: Vec<EntityId>,
    pub(in super::super) publications: usize,
    pub(in super::super) bounds: Option<Bounds<Pixels>>,
    pub(in super::super) input_mask: Option<Bounds<Pixels>>,
    pub(in super::super) paint_clip: Option<Bounds<Pixels>>,
}

#[derive(Clone, Default)]
pub(in super::super) struct Observations(
    pub(in super::super) Rc<RefCell<HashMap<&'static str, Observed>>>,
);

impl Global for Observations {}

fn record(key: &'static str, cx: &App, update: impl FnOnce(&mut Observed)) {
    if let Some(observations) = cx.try_global::<Observations>() {
        update(observations.0.borrow_mut().entry(key).or_default());
    }
}

pub(super) fn created(key: &'static str, surface: &Surface<Rgba, ()>, cx: &App) {
    record(key, cx, |observed| {
        observed.surfaces.push(surface.entity().downgrade());
    });
}

pub(super) fn stopped(key: &'static str, id: EntityId, cx: &App) {
    record(key, cx, |observed| observed.stops.push(id));
}

pub(super) fn published(key: &'static str, cx: &App) {
    record(key, cx, |observed| observed.publications += 1);
}

pub(super) fn input(key: &'static str, bounds: Bounds<Pixels>, mask: Bounds<Pixels>, cx: &App) {
    record(key, cx, |observed| {
        observed.bounds = Some(bounds);
        observed.input_mask = Some(mask);
    });
}

pub(super) fn paint(key: &'static str, clip: Bounds<Pixels>, cx: &App) {
    record(key, cx, |observed| observed.paint_clip = Some(clip));
}
