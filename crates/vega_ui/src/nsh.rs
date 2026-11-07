use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{App, AppContext, Context, Entity, Focusable, Window};
use nsh_core::element::{Component, EngineContext, NoEvent, Root, Scope, engine_context};
use nsh_core::prelude::{
    AnyElement, IntoElement, Signal, SignalEquals, signal, signal_with, spawn, view,
};
use nsh_platform_gpui::{App as NshApp, AppOptions, AppView, CacheOptions, Locale};
use tokio::sync::{mpsc, oneshot, watch};

pub const COMMAND_CAPACITY: usize = 64;

#[derive(Clone)]
pub(crate) struct ProjectionId(Arc<()>);

impl ProjectionId {
    pub(crate) fn new() -> Self {
        Self(Arc::new(()))
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub(crate) struct OrderedCommands<C> {
    sender: mpsc::Sender<C>,
    previous: Rc<RefCell<Option<oneshot::Receiver<()>>>>,
    pub failed: Signal<bool>,
}

impl<C> Clone for OrderedCommands<C> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            previous: self.previous.clone(),
            failed: self.failed,
        }
    }
}

impl<C: 'static> OrderedCommands<C> {
    pub(crate) fn new(sender: mpsc::Sender<C>) -> Self {
        Self {
            sender,
            previous: Rc::new(RefCell::new(None)),
            failed: signal(false),
        }
    }

    pub(crate) fn send(&self, command: C) {
        let (finished, pending) = oneshot::channel();
        let previous = self.previous.borrow_mut().replace(pending);
        let sender = self.sender.clone();
        let failed = self.failed;
        spawn(async move {
            if let Some(previous) = previous {
                let _ = previous.await;
            }
            if sender.send(command).await.is_err() {
                failed.set(true);
            }
            let _ = finished.send(());
        });
    }
}

pub type Renderer<S, C> = fn(Signal<S>, mpsc::Sender<C>) -> AnyElement;

struct Bridge<S, C> {
    state: watch::Receiver<S>,
    commands: mpsc::Sender<C>,
    renderer: Renderer<S, C>,
}

struct Page<S, C>(PhantomData<fn() -> (S, C)>);

impl<S, C> Component for Page<S, C>
where
    S: Clone + Send + Sync + 'static,
    C: Send + 'static,
{
    type Props = ();
    type Event = NoEvent;

    fn view(_: (), _: Scope<NoEvent>) -> impl IntoElement {
        let Some(bridge) = engine_context::<Bridge<S, C>>() else {
            return view().into_element();
        };
        let mut receiver = bridge.state.clone();
        let state = signal_with(receiver.borrow_and_update().clone(), SignalEquals::Never);
        spawn(async move {
            while receiver.changed().await.is_ok() {
                let next = receiver.borrow_and_update().clone();
                state.set(next);
            }
        });
        (bridge.renderer)(state, bridge.commands.clone())
    }
}

pub struct Surface<S, C> {
    view: Entity<AppView>,
    state: watch::Sender<S>,
    errors: watch::Receiver<Option<String>>,
    language: &'static str,
    appearance: vega_theme::Appearance,
    command: PhantomData<fn(C)>,
}

impl<S, C> Surface<S, C>
where
    S: Clone + Send + Sync + 'static,
    C: Send + 'static,
{
    pub fn new<T: 'static>(
        initial: S,
        renderer: Renderer<S, C>,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> (Self, mpsc::Receiver<C>) {
        let (state, receiver) = watch::channel(initial);
        let (commands, requests) = mpsc::channel(COMMAND_CAPACITY);
        let (failures, errors) = watch::channel(None);
        let bridge = Bridge {
            state: receiver,
            commands,
            renderer,
        };
        let language = vega_i18n::language();
        let appearance = vega_theme::theme(cx).appearance;
        let view = cx.new(|cx| {
            let view = AppView::new(
                AppOptions {
                    app_id: "ai.vega.desktop".into(),
                    cache: CacheOptions::off(),
                    context: EngineContext::default().with(bridge),
                    ..Default::default()
                },
                window,
                cx,
            );
            view.on_app_error(move |error| {
                failures.send_replace(Some(error.message.clone()));
            });
            view.set_locale(Locale::parse(language), cx);
            view.set_appearance(nsh_appearance(appearance), cx);
            view.load(NshApp::Rust(Root::of::<Page<S, C>>()));
            view
        });
        (
            Self {
                view,
                state,
                errors,
                language,
                appearance,
                command: PhantomData,
            },
            requests,
        )
    }

    pub fn publish(&self, state: S) {
        self.state.send_replace(state);
    }

    pub fn errors(&self) -> watch::Receiver<Option<String>> {
        self.errors.clone()
    }

    pub fn entity(&self) -> Entity<AppView> {
        self.view.clone()
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.view.read(cx).focus_handle(cx), cx);
    }

    pub fn sync_environment(&mut self, cx: &mut App) {
        let language = vega_i18n::language();
        let appearance = vega_theme::theme(cx).appearance;
        if self.language != language || self.appearance != appearance {
            self.language = language;
            self.appearance = appearance;
            self.view.update(cx, |view, cx| {
                view.set_locale(Locale::parse(language), cx);
                view.set_appearance(nsh_appearance(appearance), cx);
            });
        }
    }
}

fn nsh_appearance(value: vega_theme::Appearance) -> nsh_platform_gpui::Appearance {
    match value {
        vega_theme::Appearance::Light => nsh_platform_gpui::Appearance::Light,
        vega_theme::Appearance::Dark => nsh_platform_gpui::Appearance::Dark,
    }
}

pub fn color(value: gpui_kit::Rgba) -> nsh_core::Rgba {
    nsh_core::Rgba {
        r: value.r,
        g: value.g,
        b: value.b,
        a: value.a,
    }
}
