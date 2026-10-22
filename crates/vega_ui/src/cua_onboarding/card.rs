use super::controller::{CardAction, CardController, CardEffect, CheckTicket};
use super::native::{self, DesktopGeometry, Screen};
use super::placement::{Rect, follow_target, perch};
use super::session::{self, ProbeClient, ServiceStatus};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, DisplayId, FocusHandle, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, point, px,
    size,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use vega_conversation::types::ComputerUseError;
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
    Configuration,
    Movement,
}

impl PanelFault {
    fn message(self) -> &'static str {
        match self {
            Self::Geometry => {
                "无法定位系统设置窗口，卡片暂不跟随。可手动打开权限设置；若卡片挡住操作，请关闭后重新打开。"
            }
            Self::Configuration => {
                "无法启用悬浮窗口保护，卡片可能抢占焦点或无法跨桌面显示。请关闭卡片，在系统设置中手动授权后重新打开。"
            }
            Self::Movement => {
                "无法跟随系统设置窗口，权限检测仍可用。若卡片挡住操作，请关闭后重新打开。"
            }
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
    client: Option<ProbeClient>,
    pending: Option<CheckTicket>,
    service_status: ServiceStatus,
    adapter_error: Option<String>,
    external_dispatch: Option<String>,
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
    session::install_quit_hook(cx);
    cx.open_window(
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
            kind: WindowKind::Floating,
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
                if let Err(error) = native::configure_panel(&card.marker) {
                    tracing::warn!(?error, "preparation panel configuration failed");
                    card.panel_fault = Some(PanelFault::Configuration);
                    card.tracking = Tracking::Disabled;
                }
                card.start(window, cx);
                card
            })
        },
    )
    .map_err(|error| OpenError::Window(error.to_string()))
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
        Self {
            controller: CardController::new(task, executable),
            return_handler,
            has_task: !task.is_empty(),
            client: None,
            pending: None,
            service_status: session::service_status(),
            adapter_error: None,
            external_dispatch: None,
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
        cx.on_release(|card, _| card.invalidate()).detach();
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = weak.update(cx, |card, _| card.invalidate());
            true
        });
        window.focus(&self.focus, cx);
        if let Some(error) = self.service_status.last_inspection_error {
            for effect in self.controller.act(CardAction::Recheck) {
                if let CardEffect::Inspect { ticket, .. } = effect {
                    self.controller.complete(ticket, Err(error));
                }
            }
        } else if self.service_status.cleanup == session::CleanupStatus::Clear
            && self.service_status.last_close_error.is_none()
            && self.service_status.failure.is_none()
        {
            self.dispatch(CardAction::Recheck, window, cx);
        }
        let cancel = self.cancel.clone();
        cx.spawn_in(window, async move |weak, cx| {
            while !cancel.is_cancelled() {
                cx.background_executor().timer(FOLLOW_INTERVAL).await;
                if cancel.is_cancelled() {
                    break;
                }
                if weak
                    .update_in(cx, |card, window, cx| card.tick(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn invalidate(&mut self) {
        self.controller.act(CardAction::Close);
        self.cancel.cancel();
        if let Some(client) = self.client.take() {
            client.cancel();
        }
        self.pending = None;
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
                CardEffect::Inspect {
                    ticket,
                    prompt,
                    probe_direct_capture,
                } => {
                    self.adapter_error = None;
                    let result = if self.client.is_none() {
                        ProbeClient::open().map(|client| self.client = Some(client))
                    } else {
                        Ok(())
                    }
                    .and_then(|()| {
                        self.client
                            .as_ref()
                            .ok_or(session::AdapterError::Unavailable)?
                            .inspect(ticket.clone(), prompt, probe_direct_capture)
                    });
                    match result {
                        Ok(()) => self.pending = Some(ticket),
                        Err(error) => {
                            self.adapter_error = Some(error.to_string());
                            self.controller
                                .complete(ticket, Err(ComputerUseError::Unavailable));
                            self.pending = None;
                            self.client = None;
                        }
                    }
                }
                CardEffect::OpenUrl(url) => {
                    cx.open_url(&url);
                    if !matches!(self.tracking, Tracking::Disabled) {
                        self.tracking = Tracking::Seeking(INITIAL_SETTINGS_ATTEMPTS);
                    }
                    self.external_dispatch =
                        Some("若系统设置未出现，请从 Apple 菜单手动打开“隐私与安全性”。".into());
                }
                CardEffect::RevealInFinder(path) => {
                    if path.exists() {
                        cx.reveal_path(&path);
                        self.external_dispatch =
                            Some("若 Finder 未出现，请手动前往当前 Vega 的位置。".into());
                    } else {
                        self.external_dispatch =
                            Some("当前 Vega 的位置已不存在。请重新打开应用后重试。".into());
                    }
                }
                CardEffect::CancelPending => {
                    self.cancel.cancel();
                    if let Some(client) = self.client.take() {
                        client.cancel();
                    }
                    self.pending = None;
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

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.controller.view().open {
            return;
        }
        for _ in 0..16 {
            let Some(client) = self.client.as_ref() else {
                break;
            };
            match client.try_recv() {
                Ok(Some(response)) => {
                    if self.pending.as_ref() == Some(&response.ticket) {
                        self.pending = None;
                    }
                    let effects = self.controller.complete(response.ticket, response.result);
                    self.apply_effects(effects, window, cx);
                    cx.notify();
                }
                Ok(None) => break,
                Err(error) => {
                    if let Some(ticket) = self.pending.take() {
                        self.controller
                            .complete(ticket, Err(ComputerUseError::Unavailable));
                    }
                    self.adapter_error = Some(error.to_string());
                    self.client = None;
                    cx.notify();
                    break;
                }
            }
        }
        let status = session::service_status();
        if status != self.service_status {
            self.service_status = status;
            cx.notify();
        }
        self.follow(cx);
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
