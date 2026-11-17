use std::collections::HashMap;

use gpui_kit::{App, Context, Task, Window};
use nsh_core::prelude::*;
use tokio::sync::mpsc;

use super::{CloseSettings, SettingsView};

#[derive(Clone, PartialEq)]
pub(super) enum Kind {
    Group,
    Text,
    Button,
    Input { masked: bool, multiline: bool },
}

#[derive(Clone, PartialEq)]
pub(super) struct Node {
    pub id: String,
    pub label: String,
    pub value: String,
    pub enabled: bool,
    pub selected: bool,
    pub kind: Kind,
    pub children: Vec<Node>,
}

impl Node {
    pub fn text(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value: String::new(),
            enabled: true,
            selected: false,
            kind: Kind::Text,
            children: Vec::new(),
        }
    }

    pub fn group(id: impl Into<String>, children: Vec<Self>) -> Self {
        Self { kind: Kind::Group, children, ..Self::text(id, "") }
    }
}

type Handler = Box<dyn Fn(&mut SettingsView, String, &mut Window, &mut Context<SettingsView>)>;

#[derive(Default)]
pub(super) struct Page {
    pub handlers: HashMap<String, Handler>,
}

impl Page {
    pub fn button(
        &mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        enabled: bool,
        selected: bool,
        handler: impl Fn(&mut SettingsView, &mut Window, &mut Context<SettingsView>) + 'static,
    ) -> Node {
        let id = id.into();
        self.handlers.insert(id.clone(), Box::new(move |this, _, window, cx| handler(this, window, cx)));
        Node { kind: Kind::Button, enabled, selected, ..Node::text(id, label) }
    }

    pub fn input(
        &mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        input: gpui_kit::Entity<super::TextInput>,
        masked: bool,
        multiline: bool,
        enabled: bool,
        cx: &App,
    ) -> Node {
        let id = id.into();
        let value = if masked { String::new() } else { input.read(cx).text().to_owned() };
        self.handlers.insert(id.clone(), Box::new(move |_, value, _, cx| {
            input.update(cx, |input, cx| input.set_text(&value, cx));
            cx.notify();
        }));
        Node { kind: Kind::Input { masked, multiline }, value, enabled, ..Node::text(id, label) }
    }
}

#[derive(Clone)]
pub(super) struct Projection {
    route: String,
    navigation: Vec<Node>,
    content: Vec<Node>,
    background: Rgba,
    foreground: Rgba,
    elevated: Rgba,
    accent: Rgba,
}

pub(super) struct Command {
    route: String,
    id: String,
    value: String,
}

pub(super) struct Presentation {
    surface: crate::nsh::Surface<Projection, Command>,
    _commands: Task<()>,
    _errors: Task<()>,
}

fn send(commands: &mpsc::Sender<Command>, route: String, id: String, value: String) {
    let commands = commands.clone();
    spawn(async move { let _ = commands.send(Command { route, id, value }).await; });
}

fn nodes(
    items: impl IntoReactive<Vec<Node>>,
    state: Signal<Projection>,
    commands: mpsc::Sender<Command>,
) -> AnyElement {
    each(items, |node| node.id.clone(), move |node, _| {
        let initial = node.get();
        let id = initial.id.clone();
        match initial.kind {
            Kind::Group => view().key(id).flex_col().gap_3().child(nodes(move || node.get().children, state, commands.clone())).into_element(),
            Kind::Text => text(move || node.get().label).into_element(),
            Kind::Button => {
                let commands = commands.clone();
                view().key(id.clone()).role(AccessibilityRole::Button).focusable(true)
                    .disabled(move || !node.get().enabled)
                    .label(move || node.get().label)
                    .accessibility_state(move || AccessibilityState { selected: node.get().selected, ..Default::default() })
                    .px_3().py_2().rounded(6.)
                    .bg(move || if node.get().selected { state.get().accent } else { state.get().elevated })
                    .on_press(move |_: &PressEvent| send(&commands, state.get().route, id.clone(), String::new()))
                    .child(text(move || node.get().label)).into_element()
            }
            Kind::Input { masked, multiline } => {
                let commands = commands.clone();
                let field = input().key(id.clone()).label(move || node.get().label)
                    .masked(masked).multiline(multiline).readonly(move || !node.get().enabled)
                    .px_3().py_2().rounded(6.).bg(move || state.get().elevated)
                    .on_change(move |value: &str| send(&commands, state.get().route, id.clone(), value.to_owned()));
                let field = if masked { field } else { field.value(move || node.get().value) };
                view().flex_col().gap_1().child(text(move || node.get().label)).child(field).into_element()
            }
        }
    })
}

fn render(state: Signal<Projection>, commands: mpsc::Sender<Command>) -> AnyElement {
    view().size_full().flex_row().gap_4()
        .bg(move || state.get().background).text_color(move || state.get().foreground)
        .child(scroll().w(190.).h_full().child(view().flex_col().p_3().gap_2()
            .child(nodes(move || state.get().navigation, state, commands.clone()))))
        .child(scroll().flex_1().h_full().child(view().flex_col().p_4().gap_4()
            .child(nodes(move || state.get().content, state, commands))))
        .into_element()
}

impl SettingsView {
    fn nsh_route(&self) -> String {
        format!("{}:{:?}:{:?}", self.section, self.provider_management.selected, self.model_settings)
    }

    pub(super) fn render_nsh(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let mut page = Page::default();
        let navigation = [(1, "settings.nav.general"), (0, "settings.nav.providers"), (5, "settings.nav.mcp"), (7, "settings.nav.agents"), (6, "settings.nav.skills"), (4, "settings.nav.usage")]
            .into_iter().map(|(section, key)| page.button(format!("nav-{section}"), vega_i18n::t(key), true, self.navigation_section() == section, move |this, _, cx| this.request_settings_section(section, cx))).collect::<Vec<_>>();
        let mut content = Vec::new();
        if let Some(error) = &self.error { content.push(Node::text("error", error.render())); }
        if self.pending_section.is_some() { content.extend(self.nsh_navigation_guard(&mut page, cx)); }
        content.extend(match self.section {
            0 => self.nsh_providers(&mut page, cx),
            1 => self.nsh_general(&mut page, cx),
            2 => self.nsh_reasoning(&mut page, cx),
            3 => self.nsh_pricing(&mut page, cx),
            4 => self.nsh_usage(&mut page, cx),
            5 => self.nsh_mcp(&mut page, cx),
            6 => self.nsh_skills(&mut page, cx),
            _ => self.nsh_agents(&mut page, cx),
        });
        let mut navigation = navigation;
        navigation.insert(0, page.button("close", vega_i18n::t("settings.navigation.back_app"), true, false, |this, window, cx| {
            if this.settings_have_draft(cx) {
                this.pending_settings_close = true;
                this.pending_section = Some(super::SettingsDestination::ModelSection(this.section));
                cx.notify();
            } else if !this.guard_default_permission_close(window, cx) {
                window.dispatch_action(Box::new(CloseSettings), cx);
            }
        }));
        let colors = vega_theme::theme(cx).colors;
        let projection = Projection {
            route: self.nsh_route(), navigation, content,
            background: crate::nsh::color(colors.bg_base), foreground: crate::nsh::color(colors.text_primary),
            elevated: crate::nsh::color(colors.bg_elevated), accent: crate::nsh::color(colors.bg_active),
        };
        self.nsh_page = page;
        if self.nsh_presentation.is_none() {
            let (surface, mut commands) = crate::nsh::Surface::new(projection.clone(), render, window, cx);
            let mut errors = surface.errors();
            let command_task = cx.spawn_in(window, async move |this, cx| {
                while let Some(command) = commands.recv().await {
                    if this.update_in(cx, |this, window, cx| {
                        if command.route != this.nsh_route() { return; }
                        if let Some(handler) = this.nsh_page.handlers.remove(&command.id) {
                            handler(this, command.value, window, cx);
                            this.nsh_page.handlers.insert(command.id, handler);
                            cx.notify();
                        }
                    }).is_err() { break; }
                }
            });
            let error_task = cx.spawn(async move |this, cx| {
                while errors.changed().await.is_ok() {
                    if errors.borrow_and_update().is_some() {
                        let _ = this.update(cx, |this, cx| {
                            this.error = Some(super::SettingsText::key("settings.config.load_failed"));
                            cx.notify();
                        });
                    }
                }
            });
            self.nsh_presentation = Some(Presentation { surface, _commands: command_task, _errors: error_task });
        }
        let Some(presentation) = self.nsh_presentation.as_mut() else { return gpui_kit::div().into_any_element(); };
        presentation.surface.sync_environment(cx);
        presentation.surface.publish(projection);
        gpui_kit::IntoElement::into_any_element(presentation.surface.entity())
    }
}
