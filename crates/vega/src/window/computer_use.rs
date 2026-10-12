use super::*;
use vega_ui::interaction::InteractionExt;

struct ComputerUsePreview {
    id: String,
    image: Arc<Image>,
    rendered: Option<Arc<RenderImage>>,
}

pub(crate) struct ComputerUsePreviews {
    epoch: Option<u64>,
    entries: Vec<ComputerUsePreview>,
}

impl ComputerUsePreviews {
    pub fn new(cx: &mut App) -> Entity<Self> {
        let previews = cx.new(|_| Self {
            epoch: None,
            entries: Vec::new(),
        });
        cx.observe_release(&previews, |previews, cx| previews.clear(None, cx))
            .detach();
        previews
    }

    fn discard(preview: ComputerUsePreview, window: Option<&mut Window>, cx: &mut App) {
        preview.image.remove_asset(cx);
        if let Some(rendered) = preview.rendered {
            cx.drop_image(rendered, window);
        }
    }

    fn clear(&mut self, mut window: Option<&mut Window>, cx: &mut App) {
        for preview in self.entries.drain(..) {
            Self::discard(preview, window.as_deref_mut(), cx);
        }
        self.epoch = None;
    }

    fn sync(
        &mut self,
        epoch: u64,
        observations: &[(String, ImageAttachment)],
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.epoch != Some(epoch) {
            self.clear(Some(window), cx);
        }
        self.epoch = Some(epoch);
        let observations = observations.iter().take(2).collect::<Vec<_>>();
        let previous = std::mem::take(&mut self.entries);
        for preview in previous {
            if observations.iter().any(|item| item.0 == preview.id) {
                self.entries.push(preview);
            } else {
                Self::discard(preview, Some(window), cx);
            }
        }
        for observation in observations {
            if self
                .entries
                .iter()
                .any(|preview| preview.id == observation.0)
            {
                continue;
            }
            let image = &observation.1;
            let format = match image.mime_type() {
                "image/jpeg" => ImageFormat::Jpeg,
                "image/webp" => ImageFormat::Webp,
                _ => ImageFormat::Png,
            };
            self.entries.push(ComputerUsePreview {
                id: observation.0.clone(),
                image: Arc::new(Image::from_bytes(format, image.bytes().to_vec())),
                rendered: None,
            });
        }
        for preview in &mut self.entries {
            preview.rendered = preview.image.clone().use_render_image(window, cx);
        }
    }

    fn image(&self, id: &str) -> Option<Arc<Image>> {
        self.entries
            .iter()
            .find(|preview| preview.id == id)
            .map(|preview| preview.image.clone())
    }
}

#[derive(Clone)]
pub(crate) enum ComputerUseAction {
    Pause,
    Resume,
    Stop,
    Browser(Target),
    Details,
}

#[derive(Clone)]
pub(crate) struct ComputerUseOwner {
    pub thread_id: String,
    pub generation: u64,
    pub epoch: u64,
    pub phase: ControlPhase,
    pub capability: Option<Capability>,
}

impl VegaWindow {
    pub(crate) fn computer_use_action(
        &mut self,
        owner: &ComputerUseOwner,
        action: &ComputerUseAction,
        cx: &mut Context<Self>,
    ) {
        let Some(active) = self
            .agent_controller
            .active
            .get_mut(&owner.thread_id)
            .filter(|active| active.generation == owner.generation)
        else {
            return;
        };
        let Some(control) = &active.computer_use else {
            return;
        };
        let status = control.status();
        if status.epoch != owner.epoch
            || status.phase != owner.phase
            || status.capability != owner.capability
        {
            return;
        }
        match action {
            ComputerUseAction::Pause
                if matches!(
                    status.phase,
                    ControlPhase::Connecting
                        | ControlPhase::AwaitingApproval
                        | ControlPhase::Running
                ) =>
            {
                control.pause()
            }
            ComputerUseAction::Resume if status.phase == ControlPhase::Paused => {
                let _ = control.resume(owner.epoch);
            }
            ComputerUseAction::Stop => {
                control.stop();
                self.cancel_agent_for_thread(&owner.thread_id, cx);
            }
            ComputerUseAction::Browser(target)
                if status.phase == ControlPhase::AwaitingApproval =>
            {
                let _ = control.approve_browser(owner.epoch, target);
            }
            ComputerUseAction::Details => {
                active.computer_use_details_open = !active.computer_use_details_open
            }
            _ => return,
        }
        cx.notify();
    }

    fn computer_use_button(
        &self,
        id: impl Into<SharedString>,
        label: &str,
        owner: &ComputerUseOwner,
        action: ComputerUseAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let id = id.into();
        let selector = id.clone();
        let mouse_owner = owner.clone();
        let key_owner = owner.clone();
        let key_action = action.clone();
        div()
            .id(id)
            .debug_selector(move || selector.to_string())
            .flex_shrink_0()
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .text_size(px(Typography::METADATA))
            .text_color(colors.text_primary)
            .interaction_surface(colors.bg_elevated, colors.bg_hover, colors.bg_hover, true)
            .focusable()
            .tab_stop(true)
            .focus_visible(move |style| style.border_color(colors.accent))
            .cursor_pointer()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.computer_use_action(&mouse_owner, &action, cx)
                }),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.computer_use_action(&key_owner, &key_action, cx);
                    cx.stop_propagation();
                }
            }))
            .child(label.to_string())
            .into_any_element()
    }

    pub(crate) fn render_computer_use_bar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let selected = self
            .agent_controller
            .active
            .values()
            .filter_map(|active| {
                active
                    .computer_use
                    .as_ref()
                    .map(|control| (active, control.status()))
            })
            .filter(|(_, status)| {
                !matches!(
                    status.phase,
                    ControlPhase::Disconnected | ControlPhase::Stopped
                )
            })
            .min_by_key(|(active, status)| {
                (
                    if status.phase == ControlPhase::Failed {
                        1
                    } else {
                        0
                    },
                    active.generation,
                )
            });
        let Some((active, status)) = selected else {
            self.computer_use_previews
                .update(cx, |previews, cx| previews.clear(Some(window), cx));
            return None;
        };
        if !active.computer_use_details_open || status.phase != ControlPhase::Running {
            self.computer_use_previews
                .update(cx, |previews, cx| previews.clear(Some(window), cx));
        }
        let colors = theme(cx).colors;
        let owner = ComputerUseOwner {
            thread_id: active.thread_id.clone(),
            generation: active.generation,
            epoch: status.epoch,
            phase: status.phase,
            capability: status.capability,
        };
        let phase = match status.phase {
            ControlPhase::Disconnected => "待连接",
            ControlPhase::Connecting => "正在连接",
            ControlPhase::AwaitingApproval => "等待标签页授权",
            ControlPhase::Running => "正在操控",
            ControlPhase::Pausing => "正在交还控制，请稍候…",
            ControlPhase::Paused => "已接管",
            ControlPhase::Stopping => "正在停止…",
            ControlPhase::Stopped => "已停止",
            ControlPhase::Failed => "操控不可用",
        };
        let mut actions = div().flex().flex_wrap().gap_2();
        if matches!(
            status.phase,
            ControlPhase::Connecting | ControlPhase::AwaitingApproval | ControlPhase::Running
        ) {
            actions = actions.child(self.computer_use_button(
                "computer-use-pause",
                "接管",
                &owner,
                ComputerUseAction::Pause,
                cx,
            ));
        }
        if status.phase == ControlPhase::Paused {
            actions = actions.child(self.computer_use_button(
                "computer-use-resume",
                "继续",
                &owner,
                ComputerUseAction::Resume,
                cx,
            ));
        }
        if status.phase != ControlPhase::Stopping {
            actions = actions.child(self.computer_use_button(
                "computer-use-stop",
                "停止任务",
                &owner,
                ComputerUseAction::Stop,
                cx,
            ));
        }
        if matches!(status.phase, ControlPhase::Running | ControlPhase::Paused) {
            actions = actions.child(self.computer_use_button(
                "computer-use-details",
                if active.computer_use_details_open {
                    "收起观察"
                } else {
                    "查看观察"
                },
                &owner,
                ComputerUseAction::Details,
                cx,
            ));
        }
        let mut bar = div()
            .id("computer-use-control-bar")
            .debug_selector(|| "computer-use-control-bar".into())
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_elevated)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(Typography::BODY))
                                    .text_color(colors.text_primary)
                                    .text_ellipsis()
                                    .child(format!("{} · {}", active.computer_use_owner, phase)),
                            )
                            .child(
                                div()
                                    .text_size(px(Typography::METADATA))
                                    .text_color(colors.text_secondary)
                                    .text_ellipsis()
                                    .child(status.target_label.clone()),
                            ),
                    )
                    .child(actions),
            );
        if let Some(error) = status.error {
            bar = bar.child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.danger)
                    .child(error.to_string()),
            );
        }
        if status.phase == ControlPhase::AwaitingApproval {
            bar = bar.child(
                div()
                    .text_size(px(Typography::METADATA))
                    .text_color(colors.text_secondary)
                    .child("本任务可观察并操作你选择的标签页，可随时接管或停止。"),
            );
            let mut choices = div()
                .id("computer-use-targets")
                .debug_selector(|| "computer-use-targets".into())
                .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2();
            if let Some(target) = status.browser_target {
                choices = choices.child(self.computer_use_button(
                    "computer-use-browser-approve",
                    &format!("授权标签页：{}", target.label()),
                    &owner,
                    ComputerUseAction::Browser(target),
                    cx,
                ));
            }
            bar = bar.child(choices);
        }
        if active.computer_use_details_open
            && status.phase == ControlPhase::Running
            && let Some(control) = &active.computer_use
        {
            let observations = control.snapshot().observations;
            self.computer_use_previews.update(cx, |previews, cx| {
                previews.sync(
                    owner.epoch,
                    &observations
                        .iter()
                        .filter(|item| item.epoch == owner.epoch)
                        .filter_map(|item| item.image.clone().map(|image| (item.id.clone(), image)))
                        .take(2)
                        .collect::<Vec<_>>(),
                    window,
                    cx,
                )
            });
            let mut details = div()
                .id("computer-use-observations")
                .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2();
            if observations.is_empty() {
                details = details.child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .child("暂无本轮观察"),
                );
            }
            for observation in observations
                .into_iter()
                .filter(|observation| observation.epoch == owner.epoch)
            {
                details = details.child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(format!(
                            "{} · {} · 当前运行观察，不保存",
                            observation.capability.label(),
                            observation.target.label()
                        )),
                );
                if let Some(image) = self.computer_use_previews.read(cx).image(&observation.id) {
                    details = details.child(
                        img(image)
                            .w_full()
                            .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                            .object_fit(ObjectFit::Contain),
                    );
                }
                details = details.child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(colors.text_secondary)
                        .child(observation.text),
                );
            }
            bar = bar.child(details);
        }
        Some(bar.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::ComputerUsePreviews;
    use base64::Engine;
    use gpui_kit::prelude::*;
    use gpui_kit::{Context, Entity, ImageSource, TestAppContext, Window, div, img, px};
    use vega_conversation::types::ImageAttachment;

    struct PreviewHarness {
        previews: Option<Entity<ComputerUsePreviews>>,
        images: Vec<(String, ImageAttachment)>,
        epoch: u64,
    }

    impl Render for PreviewHarness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let Some(previews) = &self.previews else {
                return div();
            };
            previews.update(cx, |previews, cx| {
                previews.sync(self.epoch, &self.images, window, cx)
            });
            div().children(
                self.images
                    .iter()
                    .filter_map(|(id, _)| previews.read(cx).image(id))
                    .map(|image| img(image).size(px(16.))),
            )
        }
    }

    fn fixture_image(encoded: &str) -> ImageAttachment {
        ImageAttachment::from_bytes(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
        )
        .unwrap()
    }

    #[gpui_kit::test]
    async fn issue75_observation_previews_evict_app_assets_on_replace_clear_and_release(
        cx: &mut TestAppContext,
    ) {
        let red = fixture_image(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==",
        );
        let blue = fixture_image(
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYPj/HwADAgH/5ncLrgAAAABJRU5ErkJggg==",
        );
        let previews = cx.update(ComputerUsePreviews::new);
        let held = previews.clone();
        let initial = red.clone();
        let window = cx
            .update(|cx| {
                cx.open_window(Default::default(), move |_, cx| {
                    cx.new(|_| PreviewHarness {
                        previews: Some(held),
                        images: vec![("one".into(), initial)],
                        epoch: 1,
                    })
                })
            })
            .unwrap();
        cx.run_until_parked();
        let first = previews.read_with(cx, |previews, _| previews.image("one").unwrap());
        assert!(cx.update(|cx| ImageSource::from(first.clone()).is_asset_cached(cx)));
        window
            .update(cx, |view, _, cx| {
                view.images = vec![("two".into(), blue)];
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        let second = previews.read_with(cx, |previews, _| previews.image("two").unwrap());
        assert!(!cx.update(|cx| ImageSource::from(first.clone()).is_asset_cached(cx)));
        assert!(cx.update(|cx| ImageSource::from(second.clone()).is_asset_cached(cx)));
        window
            .update(cx, |view, window, cx| {
                view.images.clear();
                previews.update(cx, |previews, cx| previews.clear(Some(window), cx));
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert!(!cx.update(|cx| ImageSource::from(second).is_asset_cached(cx)));
        window
            .update(cx, |view, _, cx| {
                view.images = vec![("again".into(), red)];
                view.epoch = 2;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert!(cx.update(|cx| ImageSource::from(first.clone()).is_asset_cached(cx)));
        window
            .update(cx, |view, _, cx| {
                view.previews = None;
                cx.notify();
            })
            .unwrap();
        drop(previews);
        cx.update(|_| ());
        cx.run_until_parked();
        assert!(!cx.update(|cx| ImageSource::from(first).is_asset_cached(cx)));
    }
}
