use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use crate::{
    events::scroll_bridge::{ScrollRemainder, ScrollRoute},
    surface::Surface,
    view::NshView,
};
use gpui::{App, Window};

type Consumer = Rc<RefCell<dyn FnMut(ScrollRemainder, &mut Window, &mut App)>>;

fn ancestry(target: &NshView) -> Vec<NshView> {
    let mut path = vec![target.clone()];
    while let Some(parent) = path.last().and_then(NshView::parent_view) {
        path.push(parent);
    }
    path
}

pub(crate) struct NativeScope {
    pub(crate) scope: ScrollScope,
    surface: std::rc::Weak<Surface>,
    generation: u64,
    path: Vec<NshView>,
    parent: Option<Rc<()>>,
}

impl NativeScope {
    pub(crate) fn new(surface: &Rc<Surface>, target: NshView) -> Self {
        Self {
            scope: ScrollScope::for_native(surface, target.clone()),
            surface: Rc::downgrade(surface),
            generation: surface.generation.get(),
            path: ancestry(&target),
            parent: current().map(|scope| scope.identity),
        }
    }

    pub(crate) fn matches(
        &self,
        surface: &Rc<Surface>,
        target: &NshView,
        parent: Option<&ScrollScope>,
    ) -> bool {
        self.surface
            .upgrade()
            .is_some_and(|previous| Rc::ptr_eq(&previous, surface))
            && self.generation == surface.generation.get()
            && self.path == ancestry(target)
            && match (&self.parent, parent) {
                (Some(previous), Some(current)) => Rc::ptr_eq(previous, &current.identity),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Drop for NativeScope {
    fn drop(&mut self) {
        self.scope.route.cancel();
        if let Some(surface) = self.surface.upgrade() {
            crate::input::retire_scroll_gesture(&surface);
        }
    }
}

#[doc = "Retained native-child and Surface scroll route shared by one host subtree."]
#[derive(Clone)]
pub struct ScrollScope {
    pub(crate) identity: Rc<()>,
    pub(crate) route: ScrollRoute,
    consume: Consumer,
}

thread_local! {
    static CURRENT: RefCell<Option<(ScrollScope, Rc<Cell<bool>>)>> = const { RefCell::new(None) };
}

struct Restore(Option<(ScrollScope, Rc<Cell<bool>>)>);

impl Drop for Restore {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = self.0.take());
    }
}

impl ScrollScope {
    #[doc = "Returns the enclosing route bound during the current subtree traversal."]
    pub fn current() -> Option<Self> { current() }

    #[doc = "Compares retained route identity rather than geometry or callback values."]
    pub fn same(&self, other: &Self) -> bool { Rc::ptr_eq(&self.identity, &other.identity) }

    #[doc = "Synchronously delivers only the supplied remainder to this original ancestor consumer."]
    pub fn consume(&self, remaining: ScrollRemainder, window: &mut Window, cx: &mut App) {
        (self.consume.borrow_mut())(remaining, window, cx);
    }

    #[doc = "Creates a stable route with synchronous remainder delivery to the original host scroll consumer."]
    pub fn new(
        route: ScrollRoute,
        consume: impl FnMut(ScrollRemainder, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            identity: Rc::new(()),
            route,
            consume: Rc::new(RefCell::new(consume)),
        }
    }

    pub(crate) fn for_native(surface: &Rc<Surface>, target: NshView) -> Self {
        let parent = current();
        let weak = Rc::downgrade(surface);
        let generation = surface.generation.get();
        let path = ancestry(&target);
        Self::new(ScrollRoute::new(), move |remaining, window, cx| {
            let Some(surface) = weak.upgrade().filter(|surface| {
                surface.generation.get() == generation
                    && surface
                        .view(target.id())
                        .is_some_and(|current| current == target)
                    && !surface.window_hidden.get()
                    && ancestry(&target) == path
            }) else {
                return;
            };
            let remaining = crate::events::scroll_bridge::consume_from(
                &surface,
                Some(target.clone()),
                remaining,
            );
            if let Some(parent) = &parent {
                (parent.consume.borrow_mut())(remaining, window, cx);
            }
            crate::rt::flush(cx);
        })
    }

    #[doc = "Revokes this route's current gesture ownership without replaying pending input."]
    pub fn cancel(&self) {
        self.route.cancel();
    }

    #[doc = "Binds the route while prepainting or painting a subtree, restoring the enclosing route afterward."]
    pub fn with<R>(&self, run: impl FnOnce() -> R) -> R {
        let _restore = Restore(
            CURRENT
                .with(|current| current.replace(Some((self.clone(), Rc::new(Cell::new(false)))))),
        );
        run()
    }
}

pub(crate) fn current() -> Option<ScrollScope> {
    CURRENT.with(|current| current.borrow().as_ref().map(|(scope, _)| scope.clone()))
}

pub(crate) fn install(window: &mut Window, _cx: &mut App) {
    let scope = CURRENT.with(|current| {
        current
            .borrow()
            .as_ref()
            .and_then(|(scope, installed)| (!installed.replace(true)).then(|| scope.clone()))
    });
    if let Some(scope) = scope {
        let consume = scope.consume;
        scope.route.install(window, move |remaining, window, cx| {
            (consume.borrow_mut())(remaining, window, cx);
        });
    }
}
