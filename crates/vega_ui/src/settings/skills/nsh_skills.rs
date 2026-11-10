use std::sync::Arc;

use gpui_kit::{App, Context, Window};
use nsh_core::Checked;
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{
    SkillRootPreview, SkillSettingsMutation, SkillSettingsProjection, SkillUiScope,
};
use vega_theme::{Layout, ThemeColors, Typography};

use super::{
    SkillAction, scope_label, skill_diagnostic_label, skill_shadow_label, skill_source_name,
};
use crate::nsh::{ProjectionId, Surface};
use crate::settings::SettingsView;

#[derive(Clone, PartialEq, Eq)]
struct Authority {
    generation: u64,
    projection: Option<SkillSettingsProjection>,
    preview: Option<SkillRootPreview>,
    busy: bool,
    available: bool,
    sources: bool,
    add: bool,
    message: Option<String>,
}

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    visible: bool,
    authority: Arc<Authority>,
    colors: ThemeColors,
}
struct Decision {
    observed: Snapshot,
    action: SkillAction,
    reply: oneshot::Sender<bool>,
}
enum Request {
    Decide(Box<Decision>),
    Layout(f32),
}
pub(super) struct Presentation {
    surface: Surface<Snapshot, Request>,
    id: ProjectionId,
    height: f32,
}

#[derive(Clone, PartialEq, Eq)]
struct Row {
    key: String,
    label: String,
    detail: Vec<String>,
    warning: bool,
    action: Option<(String, SkillAction, Option<bool>)>,
}

fn row(key: impl Into<String>, label: impl Into<String>) -> Row {
    Row {
        key: key.into(),
        label: label.into(),
        detail: Vec::new(),
        warning: false,
        action: None,
    }
}

fn toggle(
    key: impl Into<String>,
    label: impl Into<String>,
    value: bool,
    mutation: SkillSettingsMutation,
) -> Row {
    let mut row = row(key, label);
    row.action = Some((
        row.label.clone(),
        SkillAction::Mutate(mutation),
        Some(value),
    ));
    row
}

fn rows(snapshot: &Snapshot, query: &str) -> Vec<Row> {
    use vega_i18n::{t, t_args};
    let authority = &snapshot.authority;
    let mut rows = Vec::new();
    if let Some(projection) = &authority.projection {
        if authority.add {
            if projection.selected_project_id.is_some() {
                let mut item = row("inspect-project", "");
                item.action = Some((
                    t("settings.skills.inspect_project").into(),
                    SkillAction::PreviewProject,
                    None,
                ));
                rows.push(item);
            }
            let mut item = row("inspect-global", "");
            item.action = Some((
                t("settings.skills.inspect_global").into(),
                SkillAction::PreviewGlobal,
                None,
            ));
            rows.push(item);
            let mut item = row("import", "");
            item.action = Some((
                t("settings.skills.import").into(),
                SkillAction::ImportFolder,
                None,
            ));
            rows.push(item);
        }
        if authority.sources {
            rows.push(toggle(
                "global-enabled",
                t("settings.skills.use_global"),
                projection.global_enabled,
                SkillSettingsMutation::SetGlobal {
                    enabled: !projection.global_enabled,
                    automatic: projection.automatic_enabled,
                },
            ));
            rows.push(toggle(
                "global-auto",
                t("settings.skills.auto_global"),
                projection.automatic_enabled,
                SkillSettingsMutation::SetGlobal {
                    enabled: projection.global_enabled,
                    automatic: !projection.automatic_enabled,
                },
            ));
            if let Some(project_id) = &projection.selected_project_id {
                rows.push(toggle(
                    "project-enabled",
                    t("settings.skills.use_project"),
                    projection.project_enabled,
                    SkillSettingsMutation::SetProject {
                        project_id: project_id.clone(),
                        enabled: !projection.project_enabled,
                        automatic: projection.project_automatic,
                    },
                ));
                rows.push(toggle(
                    "project-auto",
                    t("settings.skills.auto_project"),
                    projection.project_automatic,
                    SkillSettingsMutation::SetProject {
                        project_id: project_id.clone(),
                        enabled: projection.project_enabled,
                        automatic: !projection.project_automatic,
                    },
                ));
            }
            for source in &projection.sources {
                let mut title = row(
                    format!("source-{}", source.id),
                    format!("{} · {}", scope_label(source.scope), source.source_label),
                );
                title.detail.push(t_args(
                    "settings.skills.directory",
                    &[("path", &source.configured_root.display().to_string())],
                ));
                title.detail.push(t_args(
                    "settings.skills.actual_target",
                    &[("path", &source.canonical_root.display().to_string())],
                ));
                if let Some(diagnostic) = &source.diagnostic {
                    title.warning = true;
                    title.detail.push(t_args(
                        "settings.skills.source_unavailable",
                        &[("diagnostic", skill_diagnostic_label(diagnostic))],
                    ));
                }
                rows.push(title);
                if !source.enabled
                    && source.diagnostic.as_deref() == Some("changed_review_required")
                {
                    let mut item = row(
                        format!("review-{}", source.id),
                        t("settings.skills.enable_directory"),
                    );
                    item.action = Some((
                        t("settings.skills.review_directory").into(),
                        SkillAction::PreviewSource {
                            source_id: source.id.clone(),
                        },
                        None,
                    ));
                    rows.push(item);
                } else {
                    rows.push(toggle(
                        format!("enable-{}", source.id),
                        format!(
                            "{}: {}",
                            t("settings.skills.enable_directory"),
                            source.source_label
                        ),
                        source.enabled,
                        SkillSettingsMutation::SetSource {
                            source_id: source.id.clone(),
                            enabled: !source.enabled,
                            automatic: source.automatic,
                        },
                    ));
                }
                rows.push(toggle(
                    format!("auto-{}", source.id),
                    format!(
                        "{}: {}",
                        t("settings.skills.auto_directory"),
                        source.source_label
                    ),
                    source.automatic,
                    SkillSettingsMutation::SetSource {
                        source_id: source.id.clone(),
                        enabled: source.enabled,
                        automatic: !source.automatic,
                    },
                ));
                let mut unlink = row(format!("unlink-{}", source.id), "");
                unlink.action = Some((
                    t("settings.skills.unlink").into(),
                    SkillAction::Mutate(SkillSettingsMutation::UnlinkSource {
                        source_id: source.id.clone(),
                    }),
                    None,
                ));
                rows.push(unlink);
            }
        }
        if projection.sources.is_empty() {
            rows.push(row("no-sources", t("settings.skills.no_sources")));
        }
        let mut matched = 0;
        for source in &projection.sources {
            if source.candidates.is_empty()
                && let Some(diagnostic) = &source.diagnostic
            {
                let mut item = row(
                    format!("diagnostic-{}", source.id),
                    format!(
                        "{} · {}",
                        scope_label(source.scope),
                        skill_diagnostic_label(diagnostic)
                    ),
                );
                item.warning = true;
                rows.push(item);
            }
            for candidate in &source.candidates {
                let searchable = format!(
                    "{} {} {} {}",
                    candidate.name,
                    candidate.description.as_deref().unwrap_or_default(),
                    scope_label(source.scope),
                    source.configured_root.display()
                )
                .to_lowercase();
                if !query.is_empty() && !searchable.contains(query) {
                    continue;
                }
                matched += 1;
                let mut item = row(
                    format!("candidate-{}-{}", source.id, candidate.name),
                    &candidate.name,
                );
                if let Some(description) = &candidate.description {
                    item.detail.push(description.clone());
                }
                item.detail.push(skill_source_name(source, projection));
                if let Some(diagnostic) = candidate
                    .diagnostic
                    .as_deref()
                    .or(source.diagnostic.as_deref())
                {
                    item.warning = true;
                    item.detail.push(skill_diagnostic_label(diagnostic).into());
                } else if !source.enabled
                    || !(if source.scope == SkillUiScope::Project {
                        projection.project_enabled
                    } else {
                        projection.global_enabled
                    })
                {
                    item.detail
                        .push(t("settings.skills.source_disabled").into());
                } else if let Some(shadow) = skill_shadow_label(projection, candidate) {
                    item.detail.push(shadow);
                }
                if candidate.content_sha256.is_some() {
                    item.action = Some((
                        t_args(
                            "settings.skills.toggle_label",
                            &[
                                (
                                    "label",
                                    &format!(
                                        "{} · {}",
                                        candidate.name,
                                        source.configured_root.display()
                                    ),
                                ),
                                (
                                    "state",
                                    t(if candidate.enabled {
                                        "settings.state.on"
                                    } else {
                                        "settings.state.off"
                                    }),
                                ),
                            ],
                        ),
                        SkillAction::Mutate(SkillSettingsMutation::SetSkill {
                            source_id: source.id.clone(),
                            name: candidate.name.clone(),
                            enabled: !candidate.enabled,
                            automatic: !candidate.enabled,
                        }),
                        Some(candidate.enabled),
                    ));
                }
                rows.push(item);
            }
        }
        if matched == 0 && !query.is_empty() {
            rows.push(row("no-matches", t("settings.skills.no_matches")));
        }
    }
    if let Some(preview) = &authority.preview {
        let mut item = row("preview", t("settings.skills.confirm_directory"));
        item.warning = true;
        item.detail.push(t_args(
            "settings.skills.selected_directory",
            &[("path", &preview.configured_root.display().to_string())],
        ));
        item.detail.push(t_args(
            "settings.skills.actual_target",
            &[("path", &preview.canonical_root.display().to_string())],
        ));
        item.detail.push(t_args(
            "settings.skills.source_label",
            &[("label", &preview.source_label)],
        ));
        item.detail.push(t_args(
            "settings.skills.candidate_count",
            &[("count", &preview.candidates.len().to_string())],
        ));
        item.detail
            .extend(preview.candidates.iter().map(|candidate| {
                candidate.diagnostic.as_deref().map_or_else(
                    || candidate.name.clone(),
                    |diagnostic| {
                        format!(
                            "{} · {}",
                            candidate.name,
                            skill_diagnostic_label(diagnostic)
                        )
                    },
                )
            }));
        item.detail.push(t("settings.skills.link_warning").into());
        rows.push(item);
        let mut item = row("link", "");
        item.action = Some((
            t("settings.skills.link_exact").into(),
            SkillAction::Mutate(SkillSettingsMutation::LinkRoot {
                preview_token: preview.token.clone(),
            }),
            None,
        ));
        rows.push(item);
        let mut item = row("cancel", "");
        item.action = Some((
            t("settings.action.cancel").into(),
            SkillAction::DismissPreview,
            None,
        ));
        rows.push(item);
    }
    rows
}

fn allowed(snapshot: &Snapshot, action: &SkillAction) -> bool {
    match action {
        SkillAction::ToggleSources | SkillAction::ToggleAdd => true,
        SkillAction::Reload => snapshot.authority.available && !snapshot.authority.busy,
        _ if snapshot.authority.busy || !snapshot.authority.available => false,
        SkillAction::DismissPreview => snapshot.authority.preview.is_some(),
        SkillAction::Mutate(SkillSettingsMutation::LinkRoot { preview_token }) => snapshot
            .authority
            .preview
            .as_ref()
            .is_some_and(|preview| preview.token == *preview_token),
        _ => snapshot
            .authority
            .projection
            .as_ref()
            .is_some_and(|projection| match action {
                SkillAction::PreviewProject => {
                    snapshot.authority.add && projection.selected_project_id.is_some()
                }
                SkillAction::PreviewGlobal | SkillAction::ImportFolder => snapshot.authority.add,
                SkillAction::PreviewSource { source_id } => {
                    snapshot.authority.sources
                        && projection.sources.iter().any(|source| {
                            source.id == *source_id
                                && !source.enabled
                                && source.diagnostic.as_deref() == Some("changed_review_required")
                        })
                }
                SkillAction::Mutate(SkillSettingsMutation::SetGlobal { enabled, automatic }) => {
                    snapshot.authority.sources
                        && ((*enabled != projection.global_enabled
                            && *automatic == projection.automatic_enabled)
                            || (*enabled == projection.global_enabled
                                && *automatic != projection.automatic_enabled))
                }
                SkillAction::Mutate(SkillSettingsMutation::SetProject {
                    project_id,
                    enabled,
                    automatic,
                }) => {
                    snapshot.authority.sources
                        && projection.selected_project_id.as_ref() == Some(project_id)
                        && ((*enabled != projection.project_enabled
                            && *automatic == projection.project_automatic)
                            || (*enabled == projection.project_enabled
                                && *automatic != projection.project_automatic))
                }
                SkillAction::Mutate(SkillSettingsMutation::SetSource {
                    source_id,
                    enabled,
                    automatic,
                }) => {
                    snapshot.authority.sources
                        && projection.sources.iter().any(|source| {
                            source.id == *source_id
                                && ((*enabled != source.enabled
                                    && *automatic == source.automatic
                                    && (source.enabled
                                        || source.diagnostic.as_deref()
                                            != Some("changed_review_required")))
                                    || (*enabled == source.enabled
                                        && *automatic != source.automatic))
                        })
                }
                SkillAction::Mutate(SkillSettingsMutation::UnlinkSource { source_id }) => {
                    snapshot.authority.sources
                        && projection
                            .sources
                            .iter()
                            .any(|source| source.id == *source_id)
                }
                SkillAction::Mutate(SkillSettingsMutation::SetSkill {
                    source_id,
                    name,
                    enabled,
                    automatic,
                }) => projection.sources.iter().any(|source| {
                    source.id == *source_id
                        && source.candidates.iter().any(|candidate| {
                            candidate.name == *name
                                && candidate.content_sha256.is_some()
                                && *enabled != candidate.enabled
                                && *automatic != candidate.enabled
                        })
                }),
                _ => false,
            }),
    }
}

fn accepts(current: &Snapshot, observed: &Snapshot, action: &SkillAction) -> bool {
    current.id.matches(&observed.id)
        && current.visible
        && current.authority == observed.authority
        && allowed(current, action)
}

impl SettingsView {
    fn skills_snapshot(&self, id: ProjectionId, cx: &App) -> Snapshot {
        Snapshot {
            id,
            visible: self.section == 6
                && cx
                    .try_global::<crate::settings::SettingsOpen>()
                    .is_some_and(|open| open.0),
            authority: Arc::new(Authority {
                generation: self.skills.request_generation,
                projection: self.skills.projection.clone(),
                preview: self.skills.root_preview.clone(),
                busy: self.skills.busy,
                available: self.skills.service.is_some(),
                sources: self.skills.show_sources,
                add: self.skills.show_add,
                message: self.skills.message.as_ref().map(|message| message.render()),
            }),
            colors: vega_theme::theme(cx).colors,
        }
    }

    pub(in crate::settings) fn sync_skills_presentation(&self, cx: &App) {
        if let Some(page) = &self.skills.presentation {
            page.surface
                .publish(self.skills_snapshot(page.id.clone(), cx));
        }
    }

    pub(crate) fn render_skills(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.skills.presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new_transparent(self.skills_snapshot(id.clone(), cx), render, window, cx);
            self.skills.presentation = Some(Presentation {
                surface,
                id,
                height: Layout::SETTINGS_NOTES_MAX_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted =
                                        this.skills.presentation.as_ref().is_some_and(|page| {
                                            accepts(
                                                &this.skills_snapshot(page.id.clone(), cx),
                                                &command.observed,
                                                &command.action,
                                            )
                                        });
                                    if accepted {
                                        this.handle_skills_action(command.action, cx);
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.skills.presentation
                                        && (height - page.height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            this.sync_skills_presentation(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.skills.presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.skills_snapshot(page.id.clone(), cx);
        let Some(page) = &mut self.skills.presentation else {
            return gpui_kit::div().into_any_element();
        };
        page.surface.sync_environment(cx);
        page.surface.publish(snapshot);
        gpui_kit::div()
            .w_full()
            .flex_shrink_0()
            .h(gpui_kit::px(page.height))
            .child(page.surface.entity())
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
struct Actions {
    state: Signal<Snapshot>,
    pending: Signal<bool>,
    failed: Signal<bool>,
}
impl Actions {
    fn send(self, commands: mpsc::Sender<Request>, action: SkillAction, authority: Arc<Authority>) {
        let mut observed = self.state.get();
        observed.authority = authority;
        if self.pending.get_untracked() || !observed.visible || !allowed(&observed, &action) {
            return;
        }
        self.pending.set(true);
        self.failed.set(false);
        spawn(async move {
            let (reply, response) = oneshot::channel();
            let accepted = commands
                .send(Request::Decide(Box::new(Decision {
                    observed,
                    action,
                    reply,
                })))
                .await
                .is_ok()
                && matches!(response.await, Ok(true));
            self.failed.set(!accepted);
            self.pending.set(false);
        });
    }
}

fn control(
    actions: Actions,
    commands: mpsc::Sender<Request>,
    item: impl Fn() -> (String, SkillAction, Option<bool>) + Copy + 'static,
    authority: impl Fn() -> Arc<Authority> + Copy + 'static,
) -> AnyElement {
    let target = element_ref();
    let focused = signal(false);
    target.on_focus_in(move || focused.set(true));
    target.on_focus_out(move || focused.set(false));
    view()
        .element_ref(&target)
        .role(if item().2.is_some() {
            AccessibilityRole::Switch
        } else {
            AccessibilityRole::Button
        })
        .focusable(true)
        .label(move || item().0)
        .accessibility_state(move || AccessibilityState {
            checked: item()
                .2
                .map(|value| if value { Checked::True } else { Checked::False }),
            ..Default::default()
        })
        .disabled(move || {
            let current = actions.state.get();
            actions.pending.get()
                || current.authority != authority()
                || !allowed(&current, &item().1)
        })
        .border(px(1.))
        .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
        .px_3()
        .py_2()
        .border_color(move || {
            crate::nsh::color(if focused.get() {
                actions.state.get().colors.accent
            } else {
                actions.state.get().colors.border_subtle
            })
        })
        .bg(move || {
            crate::nsh::color(if item().2 == Some(true) {
                actions.state.get().colors.bg_active
            } else {
                actions.state.get().colors.bg_elevated
            })
        })
        .on_press(move |_: &PressEvent| actions.send(commands.clone(), item().1, authority()))
        .child(text(move || {
            if let Some(value) = item().2 {
                actions.state.with(|_| {
                    vega_i18n::t(if value {
                        "settings.state.on"
                    } else {
                        "settings.state.off"
                    })
                    .to_owned()
                })
            } else {
                item().0
            }
        }))
        .into_element()
}

fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let actions = Actions {
        state,
        pending: signal(false),
        failed: signal(false),
    };
    let query = signal(String::new());
    let search_target = element_ref();
    let search_focused = signal(false);
    search_target.on_focus_in(move || search_focused.set(true));
    search_target.on_focus_out(move || search_focused.set(false));
    let layout = commands.clone();
    let measured = signal(0.0f32);
    let rows_commands = commands.clone();
    view()
        .w_full()
        .flex_shrink_0()
        .gap(px(Layout::SETTINGS_SECTION_GAP))
        .text_size(px(Typography::BODY))
        .text_color(move || crate::nsh::color(state.get().colors.text_primary))
        .on_layout(move |bounds: &Bounds<Points>| {
            let height = bounds.size.height.0;
            if height.is_finite() && height > 0. && (height - measured.get_untracked()).abs() > 0.5
            {
                measured.set(height);
                let commands = layout.clone();
                spawn(async move {
                    if commands.send(Request::Layout(height)).await.is_err() {
                        actions.failed.set(true);
                    }
                });
            }
        })
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .justify_between()
                .gap_2()
                .child(
                    text(move || state.with(|_| vega_i18n::t("settings.skills.title")))
                        .text_size(px(Typography::HEADING_BLOCK))
                        .font_weight(FontWeight(Typography::HEADING_BLOCK_WEIGHT.0 as u16)),
                )
                .child(
                    view()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .children(
                            [
                                (SkillAction::Reload, "settings.action.refresh"),
                                (SkillAction::ToggleSources, "settings.skills.sources"),
                                (SkillAction::ToggleAdd, "settings.skills.add"),
                            ]
                            .into_iter()
                            .map(|(action, key)| {
                                let item = stored((key, action));
                                control(
                                    actions,
                                    commands.clone(),
                                    move || {
                                        item.with(|(key, action)| {
                                            (
                                                state.with(|_| vega_i18n::t(key).to_owned()),
                                                action.clone(),
                                                None,
                                            )
                                        })
                                    },
                                    move || state.get().authority,
                                )
                            }),
                        ),
                ),
        )
        .child(
            nsh_base::input()
                .element_ref(&search_target)
                .label(move || state.with(|_| vega_i18n::t("settings.skills.search")))
                .placeholder(move || {
                    state.with(|_| vega_i18n::t("settings.skills.search").to_owned())
                })
                .value(move || query.get())
                .on_change(move |text: &str| query.set(text.into()))
                .border(px(1.))
                .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                .px_3()
                .py_2()
                .w(px(Layout::SETTINGS_LABEL_WIDTH))
                .max_w(relative(1.))
                .self_end()
                .border_color(move || {
                    crate::nsh::color(if search_focused.get() {
                        state.get().colors.accent
                    } else {
                        state.get().colors.border_subtle
                    })
                }),
        )
        .child(when(
            move || state.get().authority.busy,
            move || text(move || state.with(|_| vega_i18n::t("settings.skills.checking"))),
        ))
        .child(each(
            move || {
                state
                    .get()
                    .authority
                    .message
                    .clone()
                    .into_iter()
                    .collect::<Vec<_>>()
            },
            |message| message.clone(),
            move |message, _| {
                text(move || message.get())
                    .text_color(move || crate::nsh::color(state.get().colors.warning))
            },
        ))
        .child(each(
            move || {
                let snapshot = state.get();
                rows(&snapshot, &query.get().trim().to_lowercase())
                    .into_iter()
                    .map(|row| (row, snapshot.authority.clone()))
                    .collect::<Vec<_>>()
            },
            |(row, _)| row.key.clone(),
            move |entry, _| {
                let row = computed(move || entry.get().0);
                if matches!(
                    row.get_untracked().key.as_str(),
                    "no-sources" | "no-matches"
                ) {
                    return view().child(super::super::nsh_form::status(
                        move || state.get().colors,
                        move || state.get().colors.text_secondary,
                        text(move || row.get().label),
                    ));
                }
                let commands = rows_commands.clone();
                view()
                    .w_full()
                    .gap_2()
                    .py_2()
                    .child(text(move || row.get().label).text_color(move || {
                        crate::nsh::color(if row.get().warning {
                            state.get().colors.warning
                        } else {
                            state.get().colors.text_primary
                        })
                    }))
                    .child(each(
                        move || row.get().detail,
                        |detail| detail.clone(),
                        move |detail, _| {
                            text(move || detail.get())
                                .text_size(px(Typography::METADATA))
                                .text_color(move || {
                                    crate::nsh::color(if row.get().warning {
                                        state.get().colors.warning
                                    } else {
                                        state.get().colors.text_secondary
                                    })
                                })
                        },
                    ))
                    .child(each(
                        move || {
                            let (row, authority) = entry.get();
                            row.action
                                .into_iter()
                                .map(|item| (item, authority.clone()))
                                .collect::<Vec<_>>()
                        },
                        |((_, _, checked), _)| checked.is_some(),
                        move |item, _| {
                            control(
                                actions,
                                commands.clone(),
                                move || item.get().0,
                                move || item.get().1,
                            )
                        },
                    ))
            },
        ))
        .child(when(
            move || actions.failed.get(),
            move || {
                text(move || state.with(|_| vega_i18n::t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}
