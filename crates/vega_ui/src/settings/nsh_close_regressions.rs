use super::*;
use crate::settings::{SettingsOpen, SettingsSaved, nsh_appearance::close_support as appearance};
use gpui_kit::{
    Entity, FocusHandle, IntoElement, ParentElement, Render, Styled, Subscription, TestAppContext,
    VisualTestContext, div, prelude::*, px, size,
};
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use vega_store::config::{self, AppConfig};

static QUEUES: Mutex<Vec<Queue>> = Mutex::new(Vec::new());

#[derive(Clone)]
struct Queue {
    snapshot: Snapshot,
    commands: mpsc::Sender<Request>,
}

impl Queue {
    fn send(&self, action: Action) -> oneshot::Receiver<bool> {
        let (reply, response) = oneshot::channel();
        assert!(
            self.commands
                .try_send(Request::Decide(Box::new(Decision {
                    observed: self.snapshot.clone(),
                    action,
                    reply,
                })))
                .is_ok(),
            "actual input-preference receiver queue must remain available"
        );
        response
    }
}

pub(super) fn observe(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) {
    effect(move || {
        let snapshot = state.get();
        let mut queues = QUEUES.lock().unwrap();
        queues.retain(|queue| !queue.commands.same_channel(&commands));
        queues.push(Queue {
            snapshot,
            commands: commands.clone(),
        });
    });
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

struct Root {
    settings: Entity<SettingsView>,
    retained_preview: bool,
    sidebar_actions: usize,
    saved: usize,
    focus: FocusHandle,
    _saved: Subscription,
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.retained_preview {
            div().into_any_element()
        } else {
            self.settings.update(cx, |settings, cx| {
                div()
                    .flex()
                    .flex_col()
                    .child(settings.render_input_preferences(window, cx))
                    .child(settings.render_appearance(window, cx))
                    .into_any_element()
            })
        };
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &crate::sidebar::ToggleSidebar, _, cx| {
                    this.sidebar_actions += 1;
                    let collapsed = cx.global::<crate::sidebar::SidebarCollapsed>().0;
                    cx.set_global(crate::sidebar::SidebarCollapsed(!collapsed));
                }),
            )
            .child(content)
    }
}

struct Fixture {
    visual: VisualTestContext,
    root: Entity<Root>,
    settings: Entity<SettingsView>,
    scratch: Scratch,
    path: PathBuf,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "vega-settings-close-{}-{}",
            std::process::id(),
            vega_conversation::threads::new_thread_id(),
        )));
        std::fs::create_dir(&scratch.0).unwrap();
        let path = scratch.0.join("config.toml");
        let mut config = AppConfig::default();
        config.ui.theme = "light".into();
        config.ui.language = "en".into();
        config.agent.running_message_behavior = "queue".into();
        config.save_to(&path).unwrap();
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(SettingsOpen(true));
            cx.set_global(crate::sidebar::SidebarCollapsed(false));
            cx.set_global(crate::conversation_stream::RunningMessageDefault(
                PendingMessageBehavior::Queue,
            ));
            cx.set_global(crate::conversation_stream::ComposerPredictionsEnabled(
                config.agent.composer_predictions_enabled,
            ));
            vega_i18n::set_language("en");
        });
        let config_path = path.clone();
        let window = cx.open_window(size(px(740.), px(800.)), move |window, cx| {
            let settings = cx.new(|cx| {
                let mut settings = SettingsView::from_config(config, None, cx);
                settings.section = 1;
                settings.config_path = Some(config_path);
                settings
            });
            let saved = cx.subscribe(&settings, |this: &mut Root, _, _: &SettingsSaved, _| {
                this.saved += 1;
            });
            let focus = cx.focus_handle();
            focus.focus(window, cx);
            Root {
                settings,
                retained_preview: false,
                sidebar_actions: 0,
                saved: 0,
                focus,
                _saved: saved,
            }
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let (root, settings) = visual.update(|window, cx| {
            window.activate_window();
            let root = window.root::<Root>().flatten().unwrap();
            let settings = root.read(cx).settings.clone();
            (root, settings)
        });
        Self {
            visual,
            root,
            settings,
            scratch,
            path,
        }
    }

    fn frame(&mut self) {
        let start = Instant::now();
        for _ in 0..512 {
            assert!(start.elapsed() < Duration::from_secs(2));
            if !self.visual.cx.background_executor.tick() {
                break;
            }
        }
        self.visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        std::thread::sleep(Duration::from_millis(10));
        self.visual
            .cx
            .background_executor
            .advance_clock(Duration::from_millis(10));
    }

    fn ready(&mut self) -> (Queue, appearance::Queue) {
        let start = Instant::now();
        loop {
            self.frame();
            let settings = self.settings.clone();
            let queues = self.visual.update(|_, cx| {
                let settings = settings.read(cx);
                let page = settings.input_preferences_presentation.as_ref()?;
                let queue = QUEUES
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|queue| {
                        queue.snapshot.id.matches(&page.id)
                            && queue.snapshot.predictions
                                == settings.config.agent.composer_predictions_enabled
                            && queue.snapshot.running
                                == settings.config.agent.running_message_behavior
                            && queue.snapshot.enabled
                                == (!settings.provider_management.saving
                                    && !settings.default_permission.io_pending())
                    })
                    .cloned()?;
                Some((queue, appearance::current(settings, cx)?))
            });
            if let Some(queues) = queues {
                return queues;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "settings current queues deadline"
            );
        }
    }

    fn reply(&mut self, mut response: oneshot::Receiver<bool>, expected: bool) {
        let start = Instant::now();
        loop {
            match response.try_recv() {
                Ok(value) => {
                    assert_eq!(value, expected);
                    return;
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    panic!("settings receiver dropped reply")
                }
                Err(oneshot::error::TryRecvError::Empty) => self.frame(),
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "settings reply deadline"
            );
        }
    }

    fn settled(&mut self) {
        let start = Instant::now();
        loop {
            self.frame();
            if self
                .visual
                .update(|_, cx| !self.settings.read(cx).provider_management.saving)
            {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "settings save deadline"
            );
        }
    }

    fn unchanged(&mut self, config: &AppConfig, disk: &[u8], saved: usize, sidebar: usize) {
        self.settled();
        assert_eq!(std::fs::read(&self.path).unwrap(), disk);
        self.visual.update(|_, cx| {
            let settings = self.settings.read(cx);
            assert_eq!(&settings.config, config);
            assert!(settings.error.is_none());
            assert_eq!(self.root.read(cx).saved, saved);
            assert_eq!(self.root.read(cx).sidebar_actions, sidebar);
            assert_eq!(vega_i18n::language(), config.ui.language);
            assert_eq!(
                vega_theme::theme(cx).appearance,
                vega_theme::Appearance::Light
            );
            assert!(!cx.global::<crate::sidebar::SidebarCollapsed>().0);
            assert_eq!(
                cx.global::<crate::conversation_stream::ComposerPredictionsEnabled>()
                    .0,
                config.agent.composer_predictions_enabled,
            );
            assert_eq!(
                cx.global::<crate::conversation_stream::RunningMessageDefault>()
                    .0,
                PendingMessageBehavior::Queue,
            );
        });
    }
}

impl SettingsView {
    pub fn queued_general_commands_are_revoked_on_close(cx: &mut TestAppContext) {
        for preview in [false, true] {
            let mut fixture = Fixture::new(cx);
            assert!(fixture.scratch.0.is_dir());
            let (input, appearance) = fixture.ready();
            let baseline = fixture
                .visual
                .update(|_, cx| fixture.settings.read(cx).config.clone());
            let disk = std::fs::read(&fixture.path).unwrap();
            let responses = fixture.visual.update(|_, cx| {
                let responses = vec![
                    input.send(Action::Predictions),
                    input.send(Action::Running(PendingMessageBehavior::Steer)),
                    appearance.send(appearance::Choice::Theme),
                    appearance.send(appearance::Choice::Language),
                    appearance.send(appearance::Choice::Sidebar),
                ];
                cx.set_global(SettingsOpen(false));
                fixture.root.update(cx, |root, cx| {
                    root.retained_preview = preview;
                    cx.notify();
                });
                responses
            });
            for response in responses {
                fixture.reply(response, false);
            }
            fixture.unchanged(&baseline, &disk, 0, 0);
            fixture.visual.update(|_, cx| {
                let settings = fixture.settings.read(cx);
                assert_eq!(fixture.root.read(cx).settings, fixture.settings);
                assert_eq!(settings.section, 1);
                assert!(
                    !settings
                        .input_preferences_presentation
                        .as_ref()
                        .unwrap()
                        .id
                        .matches(&input.snapshot.id)
                );
                cx.set_global(SettingsOpen(true));
                fixture.root.update(cx, |root, cx| {
                    root.retained_preview = false;
                    cx.notify();
                });
            });
            fixture.ready();
            for response in [
                input.send(Action::Predictions),
                input.send(Action::Running(PendingMessageBehavior::Steer)),
                appearance.send(appearance::Choice::Theme),
                appearance.send(appearance::Choice::Language),
                appearance.send(appearance::Choice::Sidebar),
            ] {
                fixture.reply(response, false);
            }
            fixture.unchanged(&baseline, &disk, 0, 0);
            let (current_input, current_appearance) = fixture.ready();
            fixture.visual.update(|_, cx| {
                fixture
                    .settings
                    .update(cx, |settings, _| settings.provider_management.saving = true)
            });
            for response in [
                current_input.send(Action::Predictions),
                current_input.send(Action::Running(PendingMessageBehavior::Steer)),
                current_appearance.send(appearance::Choice::Theme),
                current_appearance.send(appearance::Choice::Language),
            ] {
                fixture.reply(response, false);
            }
            fixture.visual.update(|_, cx| {
                fixture.settings.update(cx, |settings, _| {
                    settings.provider_management.saving = false
                })
            });
            fixture.unchanged(&baseline, &disk, 0, 0);
            fixture.visual.update(|_, cx| {
                fixture
                    .settings
                    .update(cx, |settings, _| settings.section = 2)
            });
            for response in [
                current_input.send(Action::Predictions),
                current_appearance.send(appearance::Choice::Sidebar),
            ] {
                fixture.reply(response, false);
            }
            fixture.visual.update(|_, cx| {
                fixture
                    .settings
                    .update(cx, |settings, _| settings.section = 1)
            });
            let (current_input, _) = fixture.ready();
            fixture.reply(current_input.send(Action::Predictions), true);
            fixture.settled();
            assert_eq!(
                config::load_from(&fixture.path)
                    .unwrap()
                    .agent
                    .composer_predictions_enabled,
                !baseline.agent.composer_predictions_enabled
            );
            fixture.reply(current_input.send(Action::Predictions), false);
            let (current_input, _) = fixture.ready();
            let response = current_input.send(Action::Running(PendingMessageBehavior::Steer));
            fixture.reply(response, true);
            fixture
                .visual
                .update(|_, cx| cx.set_global(SettingsOpen(false)));
            fixture.settled();
            assert_eq!(
                config::load_from(&fixture.path)
                    .unwrap()
                    .agent
                    .running_message_behavior,
                "steer"
            );
            fixture
                .visual
                .update(|_, cx| cx.set_global(SettingsOpen(true)));
            let (_, current_appearance) = fixture.ready();
            fixture.reply(current_appearance.send(appearance::Choice::Theme), true);
            fixture.settled();
            assert_eq!(config::load_from(&fixture.path).unwrap().ui.theme, "dark");
            fixture.reply(current_appearance.send(appearance::Choice::Theme), false);
            let (_, current_appearance) = fixture.ready();
            fixture.reply(current_appearance.send(appearance::Choice::Language), true);
            fixture.settled();
            assert_eq!(
                config::load_from(&fixture.path).unwrap().ui.language,
                "zh-CN"
            );
            fixture.reply(current_appearance.send(appearance::Choice::Language), false);
            let (_, current_appearance) = fixture.ready();
            fixture.visual.update(|_, cx| {
                fixture
                    .settings
                    .update(cx, |settings, _| settings.provider_management.saving = true)
            });
            fixture.reply(current_appearance.send(appearance::Choice::Sidebar), true);
            fixture.frame();
            fixture.visual.update(|_, cx| {
                assert_eq!(fixture.root.read(cx).sidebar_actions, 1);
                assert!(cx.global::<crate::sidebar::SidebarCollapsed>().0);
                fixture.settings.update(cx, |settings, _| {
                    settings.provider_management.saving = false
                });
            });
            fixture.reply(current_appearance.send(appearance::Choice::Sidebar), false);
            fixture.visual.update(|_, cx| {
                assert_eq!(fixture.root.read(cx).saved, 4);
                let settings = fixture.settings.read(cx);
                assert!(settings.error.is_none());
                assert_eq!(settings.config, config::load_from(&fixture.path).unwrap());
                assert_eq!(
                    vega_theme::theme(cx).appearance,
                    vega_theme::Appearance::Dark
                );
                assert_eq!(vega_i18n::language(), "zh-CN");
                assert_eq!(
                    cx.global::<crate::conversation_stream::ComposerPredictionsEnabled>()
                        .0,
                    !baseline.agent.composer_predictions_enabled
                );
                assert_eq!(
                    cx.global::<crate::conversation_stream::RunningMessageDefault>()
                        .0,
                    PendingMessageBehavior::Steer
                );
            });
        }
    }
}
