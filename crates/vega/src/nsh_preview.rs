use gpui_kit::{prelude::*, *};
use nsh_platform_gpui::{App as NshApp, AppOptions, AppView, CacheOptions, Locale, Root};
use tokio::sync::{mpsc, oneshot, watch};
use vega_theme::{Layout, Theme, theme};

mod page;

const MAX_TEXT: usize = 256;

actions!(vega_nsh, [OpenNshPreview, CloseNshPreview]);

#[derive(Default)]
struct PreviewWindow(Option<WindowHandle<gpui_kit::base::Root>>);

impl Global for PreviewWindow {}

struct MenuLanguage(&'static str);

impl Global for MenuLanguage {}

pub(crate) fn sync_menus(cx: &mut App) {
    if !cx.has_global::<MenuLanguage>() || cx.global::<MenuLanguage>().0 == vega_i18n::language() {
        return;
    }
    cx.global_mut::<MenuLanguage>().0 = vega_i18n::language();
    cx.set_menus([Menu {
        name: "Vega".into(),
        items: vec![
            MenuItem::action(vega_i18n::t("app.nsh.title"), OpenNshPreview),
            MenuItem::separator(),
            MenuItem::action(vega_i18n::t("app.startup.quit"), crate::Quit),
        ],
        disabled: false,
    }]);
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    background: nsh_core::Rgba,
    foreground: nsh_core::Rgba,
    muted: nsh_core::Rgba,
    surface: nsh_core::Rgba,
    border: nsh_core::Rgba,
    accent: nsh_core::Rgba,
    on_accent: nsh_core::Rgba,
    title: String,
    input: String,
    send: String,
    result: String,
    session_only: String,
    failed: String,
}

impl Snapshot {
    fn current(cx: &App) -> Self {
        let colors = theme(cx).colors;
        let color = |value: gpui_kit::Rgba| nsh_core::Rgba {
            r: value.r,
            g: value.g,
            b: value.b,
            a: value.a,
        };
        Self {
            background: color(colors.bg_base),
            foreground: color(colors.text_primary),
            muted: color(colors.text_secondary),
            surface: color(colors.bg_elevated),
            border: color(colors.border_subtle),
            accent: color(colors.accent),
            on_accent: color(colors.brand_on_accent),
            title: vega_i18n::t("app.nsh.title").to_owned(),
            input: vega_i18n::t("app.nsh.input").to_owned(),
            send: vega_i18n::t("app.nsh.send").to_owned(),
            result: vega_i18n::t("app.nsh.result").to_owned(),
            session_only: vega_i18n::t("app.nsh.session_only").to_owned(),
            failed: vega_i18n::t("app.nsh.failed").to_owned(),
        }
    }
}

struct Bridge {
    state: watch::Receiver<Snapshot>,
    commands: mpsc::Sender<Command>,
}

enum Command {
    Echo(String, oneshot::Sender<String>),
    Failed,
}

struct Preview {
    composer: Entity<vega_ui::text_input::TextInput>,
    view: Entity<AppView>,
    state: watch::Sender<Snapshot>,
    worker: Task<()>,
    failed: bool,
    language: &'static str,
    appearance: vega_theme::Appearance,
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(PreviewWindow::default());
    cx.set_global(MenuLanguage(""));
    sync_menus(cx);
    cx.bind_keys([
        KeyBinding::new("cmd-alt-n", OpenNshPreview, None),
        KeyBinding::new("cmd-w", CloseNshPreview, Some("NshPreview")),
    ]);
    cx.on_action(|_: &OpenNshPreview, cx| {
        if let Some(handle) = cx.global::<PreviewWindow>().0
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let bounds = Bounds::centered(
            None,
            size(
                px(Layout::SETTINGS_CONTENT_MAX_WIDTH),
                px(Layout::PREPARATION_OVERVIEW_HEIGHT),
            ),
            cx,
        );
        match cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(vega_i18n::t("app.nsh.title").into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                let preview = cx.new(|cx| Preview::new(window, cx));
                cx.new(|cx| gpui_kit::base::Root::new(preview, window, cx))
            },
        ) {
            Ok(handle) => cx.global_mut::<PreviewWindow>().0 = Some(handle),
            Err(error) => tracing::error!(%error, "could not open NSH preview"),
        }
    });
}

impl Preview {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (view, state, worker) = Self::load(window, cx);
        let composer = cx.new(|cx| {
            vega_ui::text_input::TextInput::new_multiline(cx, "", 2)
                .with_placeholder_key("app.nsh.input")
                .with_image_paste()
                .with_edit_menu()
        });
        cx.observe(&composer, |_, _, cx| cx.notify()).detach();
        cx.observe_global::<Theme>(|_, cx| cx.notify()).detach();
        Self {
            composer,
            view,
            state,
            worker,
            failed: false,
            language: vega_i18n::language(),
            appearance: theme(cx).appearance,
        }
    }

    fn load(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<AppView>, watch::Sender<Snapshot>, Task<()>) {
        let (state, receiver) = watch::channel(Snapshot::current(cx));
        let (commands, mut requests) = mpsc::channel(8);
        let bridge = Bridge {
            state: receiver,
            commands: commands.clone(),
        };
        let view = cx.new(|cx| {
            let view = AppView::new(
                AppOptions {
                    app_id: "ai.vega.preview".into(),
                    cache: CacheOptions::off(),
                    context: nsh_core::element::EngineContext::default().with(bridge),
                    ..Default::default()
                },
                window,
                cx,
            );
            view.on_app_error(move |_| {
                let _ = commands.try_send(Command::Failed);
            });
            view.set_appearance(nsh_appearance(theme(cx).appearance), cx);
            view.set_locale(Locale::parse(vega_i18n::language()), cx);
            view.load(NshApp::Rust(Root::of::<page::PreviewPage>()));
            view
        });
        window.focus(&view.read(cx).focus_handle(cx), cx);
        let worker = cx.spawn(async move |this, cx| {
            while let Some(request) = requests.recv().await {
                let failed = matches!(request, Command::Failed);
                if this
                    .update(cx, |this, cx| {
                        if let Command::Echo(text, reply) = request {
                            let _ = reply.send(text.chars().take(MAX_TEXT).collect());
                        } else {
                            this.failed = true;
                            cx.notify();
                        }
                    })
                    .is_err()
                    || failed
                {
                    break;
                }
            }
        });
        (view, state, worker)
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view.read(cx).stop();
        let (view, state, worker) = Self::load(window, cx);
        self.view = view;
        self.state = state;
        self.worker = worker;
        self.failed = false;
        self.language = vega_i18n::language();
        self.appearance = theme(cx).appearance;
        cx.notify();
    }
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        sync_menus(cx);
        let snapshot = Snapshot::current(cx);
        if *self.state.borrow() != snapshot {
            self.state.send_replace(snapshot);
        }
        if self.language != vega_i18n::language() || self.appearance != theme(cx).appearance {
            self.language = vega_i18n::language();
            self.appearance = theme(cx).appearance;
            self.view.update(cx, |view, cx| {
                view.set_locale(Locale::parse(self.language), cx);
                view.set_appearance(nsh_appearance(self.appearance), cx);
            });
            window.set_window_title(vega_i18n::t("app.nsh.title"));
        }
        let colors = theme(cx).colors;
        div()
            .key_context("NshPreview")
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .on_action(|_: &CloseNshPreview, window, _| window.remove_window())
            .child(
                div()
                    .flex()
                    .justify_end()
                    .p(px(Layout::SETTINGS_GROUP_PADDING))
                    .child(
                        gpui_kit::component::button::Button::new("nsh-reload")
                            .label(vega_i18n::t("app.nsh.reload"))
                            .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
                    ),
            )
            .when(self.failed, |root| {
                root.child(
                    div()
                        .p(px(Layout::SETTINGS_GROUP_PADDING))
                        .text_color(colors.danger)
                        .child(vega_i18n::t("app.nsh.failed")),
                )
            })
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().p(px(Layout::SETTINGS_GROUP_PADDING)).child(
                vega_ui::text_input::TextInput::render_nsh_preview(
                    self.composer.clone(),
                    window,
                    cx,
                ),
            ))
    }
}

fn nsh_appearance(value: vega_theme::Appearance) -> nsh_platform_gpui::Appearance {
    match value {
        vega_theme::Appearance::Light => nsh_platform_gpui::Appearance::Light,
        vega_theme::Appearance::Dark => nsh_platform_gpui::Appearance::Dark,
    }
}
