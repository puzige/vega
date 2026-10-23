use super::controller::{CardAction, CardController, CardEffect};
use super::model::{self, PreparationModel};
use super::native::{self, DesktopGeometry, Screen};
use super::placement::{Rect, follow_target, perch};
use super::{CallbackIdentity, PermissionPane};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, DisplayId, Entity, FocusHandle, Image, ImageFormat, TitlebarOptions,
    Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    point, px, size,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use vega_theme::Layout;

mod view;

const FOLLOW_INTERVAL: Duration = Duration::from_millis(150);
const INITIAL_SETTINGS_ATTEMPTS: u8 = 20;

pub type ReturnHandler = Box<dyn Fn(String, &mut App)>;

impl super::PreparationRequested {
    pub fn is_current(&self, stream: &crate::conversation_stream::ConversationStream) -> bool {
        self.thread_id == stream.thread.id && self.context == stream.computer_use_task_context()
    }
}

#[derive(Debug)]
pub enum OpenError {
    Executable(std::io::Error),
    DisplayUnavailable,
    Window(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Executable(error) => write!(f, "无法确认当前 Vega 的位置：{error}"),
            Self::DisplayUnavailable => f.write_str("找不到可用显示器，无法打开电脑操控准备。"),
            Self::Window(error) => write!(f, "无法打开电脑操控准备：{error}"),
        }
    }
}

impl std::error::Error for OpenError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PanelFault {
    Geometry,
    Movement,
}

impl PanelFault {
    fn message(self) -> &'static str {
        match self {
            Self::Geometry => "无法定位系统设置。请重新打开引导；挡住操作时可关闭。",
            Self::Movement => "无法跟随系统设置。请重新打开引导；挡住操作时可关闭。",
        }
    }
}

#[derive(Clone, Copy)]
enum Tracking {
    Seeking(u8),
    Following,
    Waiting,
    Disabled,
}

struct InitialPlacement {
    bounds: Bounds<gpui_kit::Pixels>,
    display: DisplayId,
    origin: (f64, f64),
    tracking: Tracking,
    fault: Option<PanelFault>,
}

pub struct GuidanceCard {
    controller: CardController,
    return_handler: ReturnHandler,
    has_task: bool,
    preparation: Entity<PreparationModel>,
    preparation_owner: Option<CallbackIdentity>,
    app_icon: Arc<Image>,
    path_error: Option<String>,
    permission_pane: PermissionPane,
    marker: String,
    _marker_lifetime: Arc<()>,
    origin: (f64, f64),
    tracking: Tracking,
    panel_fault: Option<PanelFault>,
    cancel: CancellationToken,
    focus: FocusHandle,
}

pub fn open(
    task: &str,
    return_handler: ReturnHandler,
    cx: &mut App,
) -> Result<WindowHandle<GuidanceCard>, OpenError> {
    let executable = std::env::current_exe().map_err(OpenError::Executable)?;
    let placement = initial_placement(cx)?;
    let lifetime = Arc::new(());
    let marker = format!("Vega preparation {:p}", Arc::as_ptr(&lifetime));
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(placement.bounds)),
                display_id: Some(placement.display),
                titlebar: Some(TitlebarOptions {
                    title: Some(marker.clone().into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(
                        px(-Layout::PREPARATION_CARD_WIDTH),
                        px(-Layout::PREPARATION_CARD_HEIGHT),
                    )),
                }),
                focus: false,
                show: false,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let mut card = GuidanceCard::new(
                        task,
                        &executable,
                        return_handler,
                        marker,
                        lifetime,
                        placement,
                        cx,
                    );
                    window.set_window_title(&card.marker);
                    card.start(window, cx);
                    card
                })
            },
        )
        .map_err(|error| OpenError::Window(error.to_string()))?;
    handle
        .update(cx, |card, window, cx| {
            if let Err(error) = native::show_panel(&card.marker) {
                tracing::warn!(?error, "preparation panel presentation failed");
                card.close(window, cx);
                return Err(OpenError::Window(
                    "无法安全显示浮层。请重新打开引导，或在系统设置中手动授权。".into(),
                ));
            }
            Ok(())
        })
        .map_err(|error| OpenError::Window(error.to_string()))??;
    Ok(handle)
}

impl GuidanceCard {
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dispatch(CardAction::Close, window, cx);
    }

    pub fn is_open(&self) -> bool {
        self.controller.view().open
    }

    fn new(
        task: &str,
        executable: &Path,
        return_handler: ReturnHandler,
        marker: String,
        marker_lifetime: Arc<()>,
        placement: InitialPlacement,
        cx: &mut Context<Self>,
    ) -> Self {
        let preparation = model::shared(cx);
        let preparation_owner = Some(preparation.update(cx, |model, cx| model.acquire(true, cx)));
        cx.observe(&preparation, |_, _, cx| cx.notify()).detach();
        Self {
            controller: CardController::new(task, executable),
            return_handler,
            has_task: !task.is_empty(),
            preparation,
            preparation_owner,
            app_icon: Arc::new(Image::from_bytes(
                ImageFormat::Svg,
                include_bytes!("../../../../assets/logo/vega-icon-smile-light.svg").to_vec(),
            )),
            path_error: None,
            permission_pane: PermissionPane::Accessibility,
            marker,
            _marker_lifetime: marker_lifetime,
            origin: placement.origin,
            tracking: placement.tracking,
            panel_fault: placement.fault,
            cancel: CancellationToken::new(),
            focus: cx.focus_handle(),
        }
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_release(|card, cx| card.invalidate(cx)).detach();
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = weak.update(cx, |card, cx| card.invalidate(cx));
            true
        });
        window.focus(&self.focus, cx);
        let cancel = self.cancel.clone();
        cx.spawn(async move |weak, cx| {
            while !cancel.is_cancelled() {
                cx.background_executor().timer(FOLLOW_INTERVAL).await;
                if cancel.is_cancelled() {
                    break;
                }
                if weak
                    .update(cx, |card, cx| {
                        if !cancel.is_cancelled() {
                            card.tick(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn release_preparation(&mut self, cx: &mut App) {
        if let Some(owner) = self.preparation_owner.take() {
            self.preparation
                .update(cx, |model, cx| model.release(&owner, true, cx));
        }
    }

    fn invalidate(&mut self, cx: &mut App) {
        self.controller.act(CardAction::Close);
        self.cancel.cancel();
        self.release_preparation(cx);
    }

    pub fn seek_settings(&mut self, pane: PermissionPane, cx: &mut Context<Self>) {
        if !self.controller.view().open {
            return;
        }
        self.permission_pane = pane;
        if matches!(self.tracking, Tracking::Waiting) {
            self.tracking = Tracking::Seeking(INITIAL_SETTINGS_ATTEMPTS);
        }
        cx.notify();
    }

    fn dispatch(&mut self, action: CardAction, window: &mut Window, cx: &mut Context<Self>) {
        let effects = self.controller.act(action);
        self.apply_effects(effects, window, cx);
        cx.notify();
    }

    fn apply_effects(
        &mut self,
        effects: Vec<CardEffect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for effect in effects {
            match effect {
                CardEffect::Inspect { .. } => {
                    self.controller.cancel_inspection();
                    self.preparation.update(cx, |model, cx| model.recheck(cx));
                }
                CardEffect::OpenUrl(url) => {
                    cx.open_url(&url);
                    if matches!(self.tracking, Tracking::Waiting) {
                        self.tracking = Tracking::Seeking(INITIAL_SETTINGS_ATTEMPTS);
                    }
                }
                CardEffect::RevealInFinder(path) => {
                    if path.exists() {
                        cx.reveal_path(&path);
                        self.path_error = None;
                    } else {
                        self.path_error = Some("当前 Vega 的位置已不存在。请重新打开应用。".into());
                    }
                }
                CardEffect::CancelPending => {
                    self.cancel.cancel();
                    self.release_preparation(cx);
                }
                CardEffect::CloseWindow => window.remove_window(),
                CardEffect::ReturnToTask(task) => {
                    if !task.is_empty() {
                        (self.return_handler)(task, cx);
                    }
                }
            }
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.controller.view().open {
            self.follow(cx);
        }
    }

    fn follow(&mut self, cx: &mut Context<Self>) {
        if !self.controller.view().following
            || matches!(self.tracking, Tracking::Waiting | Tracking::Disabled)
        {
            return;
        }
        let geometry = match native::geometry() {
            Ok(geometry) => geometry,
            Err(error) => {
                tracing::warn!(?error, "preparation window geometry unavailable");
                self.panel_fault = Some(PanelFault::Geometry);
                self.tracking = Tracking::Disabled;
                cx.notify();
                return;
            }
        };
        let Some(settings) = geometry.settings else {
            if let Tracking::Seeking(remaining) = self.tracking {
                self.tracking = if remaining > 1 {
                    Tracking::Seeking(remaining - 1)
                } else {
                    Tracking::Waiting
                };
                if matches!(self.tracking, Tracking::Waiting) {
                    cx.notify();
                }
            }
            return;
        };
        self.tracking = Tracking::Following;
        let Some(screen) = settings_screen(&geometry, settings) else {
            return;
        };
        if let Some(origin) = follow_target(
            Some(settings),
            outer_size(),
            screen.visible,
            self.origin,
            !self.controller.view().following,
        ) {
            match native::move_panel(&self.marker, screen, origin) {
                Ok(()) => self.origin = origin,
                Err(error) => {
                    tracing::warn!(?error, "preparation panel move failed");
                    self.panel_fault = Some(PanelFault::Movement);
                    self.tracking = Tracking::Disabled;
                    cx.notify();
                }
            }
        }
    }
}

fn outer_size() -> (f64, f64) {
    (
        f64::from(Layout::PREPARATION_CARD_WIDTH + Layout::PREPARATION_CARD_SHADOW_PADDING * 2.0),
        f64::from(Layout::PREPARATION_CARD_HEIGHT + Layout::PREPARATION_CARD_SHADOW_PADDING * 2.0),
    )
}

fn settings_screen(geometry: &DesktopGeometry, settings: Rect) -> Option<&Screen> {
    geometry
        .screens
        .iter()
        .filter(|screen| screen.frame.intersects(settings))
        .max_by(|a, b| {
            overlap(a.frame, settings)
                .total_cmp(&overlap(b.frame, settings))
                .then_with(|| a.id.cmp(&b.id))
        })
}

fn overlap(a: Rect, b: Rect) -> f64 {
    (a.max_x().min(b.max_x()) - a.x.max(b.x)).max(0.0)
        * (a.max_y().min(b.max_y()) - a.y.max(b.y)).max(0.0)
}

fn initial_placement(cx: &App) -> Result<InitialPlacement, OpenError> {
    let main = cx.primary_display().ok_or(OpenError::DisplayUnavailable)?;
    let geometry = native::geometry();
    if let Ok(geometry) = &geometry {
        let screen = geometry
            .settings
            .and_then(|settings| settings_screen(geometry, settings))
            .or_else(|| {
                geometry
                    .screens
                    .iter()
                    .find(|screen| DisplayId::new(screen.id) == main.id())
            });
        if let Some(screen) = screen {
            let origin = perch(geometry.settings, outer_size(), screen.visible);
            let (width, height) = outer_size();
            return Ok(InitialPlacement {
                bounds: Bounds::new(
                    point(
                        px((origin.0 - screen.frame.x) as f32),
                        px((origin.1 - screen.frame.y) as f32),
                    ),
                    size(px(width as f32), px(height as f32)),
                ),
                display: DisplayId::new(screen.id),
                origin,
                tracking: if geometry.settings.is_some() {
                    Tracking::Following
                } else {
                    Tracking::Seeking(INITIAL_SETTINGS_ATTEMPTS)
                },
                fault: None,
            });
        }
    }
    tracing::warn!(
        ?geometry,
        "preparation initial placement using main display fallback"
    );
    let visible = main.visible_bounds();
    let visible = Rect::new(
        f64::from(f32::from(visible.origin.x)),
        f64::from(f32::from(visible.origin.y)),
        f64::from(f32::from(visible.size.width)),
        f64::from(f32::from(visible.size.height)),
    );
    let origin = perch(None, outer_size(), visible);
    let (width, height) = outer_size();
    Ok(InitialPlacement {
        bounds: Bounds::new(
            point(px(origin.0 as f32), px(origin.1 as f32)),
            size(px(width as f32), px(height as f32)),
        ),
        display: main.id(),
        origin,
        tracking: Tracking::Disabled,
        fault: Some(PanelFault::Geometry),
    })
}
