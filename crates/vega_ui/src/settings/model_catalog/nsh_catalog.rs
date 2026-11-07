use std::sync::Arc;

use gpui_kit::{App, Context, Window};
use nsh_core::prelude::*;
use tokio::sync::{mpsc, oneshot};
use vega_conversation::types::{
    CatalogAction, CatalogIdentity, CatalogReasoningOption, ModelCatalogProjection,
};
use vega_i18n::{t, t_args};
use vega_theme::{Layout, ThemeColors, Typography};

use super::simple_prices;
use crate::nsh::{ProjectionId, Surface};
use crate::settings::{PricingEntryKind, PricingSettingsProjection, SettingsView};

#[derive(Clone)]
struct Snapshot {
    id: ProjectionId,
    version: ProjectionId,
    visible: bool,
    owner: Option<CatalogIdentity>,
    generation: u64,
    projection: ModelCatalogProjection,
    open: bool,
    mapping: bool,
    provider: Option<String>,
    candidate: Option<CatalogIdentity>,
    capacity: Option<(u64, String, String, String, bool, bool)>,
    capacity_draft: Option<crate::settings::provider_management::ProviderDraftStamp>,
    prices: bool,
    help: bool,
    message: Option<String>,
    error: Option<String>,
    colors: ThemeColors,
}
impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.id.matches(&other.id)
            && self.version.matches(&other.version)
            && self.visible == other.visible
            && self.owner == other.owner
            && self.generation == other.generation
            && self.open == other.open
            && self.mapping == other.mapping
            && self.provider == other.provider
            && self.candidate == other.candidate
            && self.capacity == other.capacity
            && self.capacity_draft == other.capacity_draft
            && self.prices == other.prices
            && self.help == other.help
            && self.message == other.message
            && self.error == other.error
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Action {
    Toggle,
    Refresh,
    Help,
    Mapping,
    Unmap,
    Provider(Option<String>),
    Candidate(CatalogIdentity),
    Confirm,
    Capacity(bool),
    Prices,
}
struct Decision {
    observed: Snapshot,
    action: Action,
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

fn allowed(snapshot: &Snapshot, action: &Action) -> bool {
    if snapshot.owner.is_none() {
        return false;
    }
    match action {
        Action::Toggle | Action::Help => true,
        Action::Refresh => !snapshot.projection.refreshing,
        _ if !snapshot.open || snapshot.projection.refreshing => false,
        Action::Mapping => true,
        Action::Unmap => snapshot.projection.mapped.is_some(),
        Action::Provider(provider) => {
            snapshot.mapping
                && provider.as_ref().is_none_or(|id| {
                    snapshot
                        .projection
                        .providers
                        .iter()
                        .any(|provider| provider.id == *id)
                })
        }
        Action::Candidate(target) => {
            snapshot.mapping
                && snapshot.provider.as_ref() == Some(&target.provider)
                && snapshot.projection.providers.iter().any(|provider| {
                    provider.id == target.provider
                        && provider.models.iter().any(|model| model.id == target.model)
                })
        }
        Action::Confirm => {
            snapshot.mapping
                && snapshot
                    .candidate
                    .as_ref()
                    .is_some_and(|target| allowed(snapshot, &Action::Candidate(target.clone())))
        }
        Action::Capacity(input) => {
            snapshot.capacity.is_some()
                && snapshot
                    .capacity_draft
                    .as_ref()
                    .is_some_and(|stamp| !stamp.composing())
                && snapshot.projection.matched_model().is_some_and(|model| {
                    if *input {
                        model.input.is_some_and(|value| value > 0)
                    } else {
                        model.output > 0
                    }
                })
        }
        Action::Prices => {
            snapshot.prices
                && snapshot
                    .projection
                    .matched_model()
                    .and_then(simple_prices)
                    .is_some()
        }
    }
}
fn accepts(current: &Snapshot, observed: &Snapshot, action: &Action) -> bool {
    current.visible
        && current.id.matches(&observed.id)
        && current.version.matches(&observed.version)
        && current.owner == observed.owner
        && current.generation == observed.generation
        && current.open == observed.open
        && current.mapping == observed.mapping
        && current.provider == observed.provider
        && current.candidate == observed.candidate
        && current.capacity == observed.capacity
        && current.capacity_draft == observed.capacity_draft
        && current.prices == observed.prices
        && current.help == observed.help
        && allowed(current, action)
}
impl SettingsView {
    fn catalog_snapshot(&self, id: ProjectionId, cx: &App) -> Snapshot {
        let price_target = match (&self.pricing, &self.model_settings) {
            (PricingSettingsProjection::Ready { entries, .. }, Some((_, model))) => entries
                .iter()
                .find(|entry| entry.model == *model)
                .is_none_or(|entry| entry.kind == PricingEntryKind::CustomStatic),
            _ => false,
        };
        Snapshot {
            id,
            version: self.catalog.version.clone(),
            visible: self.section == 0
                && self.model_settings.is_some()
                && cx
                    .try_global::<crate::settings::SettingsOpen>()
                    .is_some_and(|open| open.0),
            owner: self.catalog.owner.clone(),
            generation: self.catalog.generation,
            projection: self.catalog.projection.clone(),
            open: self.catalog.open,
            mapping: self.catalog.mapping,
            provider: self.catalog.selected_provider.clone(),
            candidate: self.catalog.candidate.clone(),
            capacity: self.catalog_capacity_fence(cx),
            capacity_draft: self.provider_nsh_stamp(),
            prices: price_target && self.pricing_allows_editing() && !self.settings_have_draft(cx),
            help: self.catalog.help,
            message: self
                .catalog
                .message
                .as_ref()
                .map(|message| message.render()),
            error: self.catalog.error.as_ref().map(|error| error.render()),
            colors: vega_theme::theme(cx).colors,
        }
    }
    pub(in crate::settings) fn sync_catalog_presentation(&self, cx: &App) {
        if let Some(page) = &self.catalog.presentation {
            page.surface
                .publish(self.catalog_snapshot(page.id.clone(), cx));
        }
    }
    pub(crate) fn render_model_catalog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        use gpui_kit::{IntoElement, ParentElement, Styled};
        if self.catalog.presentation.is_none() {
            let id = ProjectionId::new();
            let (surface, mut requests) =
                Surface::new(self.catalog_snapshot(id.clone(), cx), render, window, cx);
            self.catalog.presentation = Some(Presentation {
                surface,
                id,
                height: Layout::SETTINGS_CONTROL_MIN_HEIGHT,
            });
            cx.spawn_in(window, async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, _, cx| {
                            match request {
                                Request::Decide(command) => {
                                    let accepted =
                                        this.catalog.presentation.as_ref().is_some_and(|page| {
                                            accepts(
                                                &this.catalog_snapshot(page.id.clone(), cx),
                                                &command.observed,
                                                &command.action,
                                            )
                                        });
                                    if accepted {
                                        match command.action {
                                            Action::Toggle => {
                                                this.catalog.open = !this.catalog.open
                                            }
                                            Action::Help => this.catalog.help = !this.catalog.help,
                                            Action::Refresh => this
                                                .request_model_catalog(CatalogAction::Refresh, cx),
                                            Action::Mapping => {
                                                this.catalog.mapping = !this.catalog.mapping;
                                                this.catalog.selected_provider = None;
                                                this.catalog.candidate = None;
                                            }
                                            Action::Unmap => {
                                                this.request_model_catalog(CatalogAction::Unmap, cx)
                                            }
                                            Action::Provider(provider) => {
                                                this.catalog.selected_provider = provider;
                                                this.catalog.candidate = None;
                                            }
                                            Action::Candidate(target) => {
                                                this.catalog.candidate = Some(target)
                                            }
                                            Action::Confirm => {
                                                if let Some(target) = this.catalog.candidate.clone()
                                                {
                                                    this.request_model_catalog(
                                                        CatalogAction::Map(target),
                                                        cx,
                                                    );
                                                }
                                            }
                                            Action::Capacity(input) => this.catalog_apply_capacity(
                                                command.observed.capacity_draft.as_ref(),
                                                input,
                                                cx,
                                            ),
                                            Action::Prices => this.catalog_apply_prices(cx),
                                        }
                                        cx.notify();
                                    }
                                    let _ = command.reply.send(accepted);
                                }
                                Request::Layout(height) if height.is_finite() && height > 0. => {
                                    if let Some(page) = &mut this.catalog.presentation
                                        && (height - page.height).abs() > 0.5
                                    {
                                        page.height = height;
                                        cx.notify();
                                    }
                                }
                                Request::Layout(_) => {}
                            }
                            this.sync_catalog_presentation(cx);
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        let Some(page) = &self.catalog.presentation else {
            return gpui_kit::div().into_any_element();
        };
        let snapshot = self.catalog_snapshot(page.id.clone(), cx);
        let Some(page) = &mut self.catalog.presentation else {
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
#[derive(Clone, PartialEq, Eq)]
struct Control {
    key: String,
    label: String,
    action: Action,
    selected: bool,
}
#[derive(Clone, PartialEq, Eq)]
struct Content {
    lines: Vec<(String, bool)>,
    controls: Vec<Control>,
}
fn content(snapshot: &Snapshot, query: &str) -> Content {
    let mut result = Content {
        lines: Vec::new(),
        controls: Vec::new(),
    };
    let mut add = |key: &str, label: String, action: Action, selected| {
        result.controls.push(Control {
            key: key.into(),
            label,
            action,
            selected,
        })
    };
    add(
        "toggle",
        t(if snapshot.open {
            "settings.catalog.collapse"
        } else {
            "settings.catalog.title"
        })
        .into(),
        Action::Toggle,
        snapshot.open,
    );
    add(
        "refresh",
        t(if snapshot.projection.refreshing {
            "settings.catalog.reading"
        } else {
            "settings.catalog.refresh"
        })
        .into(),
        Action::Refresh,
        false,
    );
    add(
        "help",
        t("settings.catalog.help").into(),
        Action::Help,
        snapshot.help,
    );
    if let Some(error) = &snapshot.error {
        result.lines.push((error.clone(), true));
    }
    if snapshot.help {
        result.lines.push((
            t_args(
                "settings.catalog.help_detail",
                &[
                    ("version", &snapshot.projection.schema_version.to_string()),
                    (
                        "digest",
                        &snapshot
                            .projection
                            .digest
                            .as_deref()
                            .map(|value| value.chars().take(12).collect::<String>())
                            .unwrap_or_else(|| t("settings.catalog.not_fetched").into()),
                    ),
                ],
            ),
            false,
        ));
    }
    if !snapshot.open {
        return result;
    }
    if let Some(key) = if snapshot.projection.refreshing {
        Some("settings.catalog.loading")
    } else if snapshot.projection.stale {
        Some("settings.catalog.stale")
    } else if snapshot.projection.fetched_at_ms.is_none() {
        Some("settings.catalog.empty")
    } else {
        None
    } {
        result.lines.push((t(key).into(), false));
    }
    if let Some(timestamp) = snapshot.projection.fetched_at_ms {
        let minutes = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| (duration.as_millis() as u64).saturating_sub(timestamp) / 60000)
            .unwrap_or_default();
        result.lines.push((
            t_args(
                "settings.catalog.fetched_ago",
                &[("minutes", &minutes.to_string())],
            ),
            false,
        ));
    }
    add(
        "mapping",
        t("settings.catalog.map").into(),
        Action::Mapping,
        snapshot.mapping,
    );
    if snapshot.projection.mapped.is_some() {
        add(
            "unmap",
            t("settings.catalog.unmap").into(),
            Action::Unmap,
            false,
        );
    }
    if snapshot.mapping {
        if let Some(provider) = &snapshot.provider {
            add(
                "back",
                t("settings.catalog.choose_provider").into(),
                Action::Provider(None),
                false,
            );
            if let Some(provider) = snapshot
                .projection
                .providers
                .iter()
                .find(|candidate| candidate.id == *provider)
            {
                for model in provider
                    .models
                    .iter()
                    .filter(|model| {
                        model.id.to_lowercase().contains(query)
                            || model.name.to_lowercase().contains(query)
                    })
                    .take(20)
                {
                    let target = CatalogIdentity {
                        provider: provider.id.clone(),
                        model: model.id.clone(),
                    };
                    add(
                        &format!("model-{}", model.id),
                        model.id.clone(),
                        Action::Candidate(target.clone()),
                        snapshot.candidate.as_ref() == Some(&target),
                    );
                }
            }
            add(
                "confirm",
                t("settings.catalog.confirm_map").into(),
                Action::Confirm,
                false,
            );
        } else {
            for provider in snapshot
                .projection
                .providers
                .iter()
                .filter(|provider| {
                    provider.id.to_lowercase().contains(query)
                        || provider.name.to_lowercase().contains(query)
                })
                .take(20)
            {
                add(
                    &format!("provider-{}", provider.id),
                    format!("{} ({})", provider.name, provider.id),
                    Action::Provider(Some(provider.id.clone())),
                    false,
                );
            }
        }
        result
            .lines
            .push((t("settings.catalog.results_limit").into(), false));
    }
    if let Some(target) = &snapshot.projection.mapped {
        result.lines.push((
            t_args(
                "settings.catalog.identity",
                &[("provider", &target.provider), ("model", &target.model)],
            ),
            false,
        ));
    }
    if let Some(model) = snapshot.projection.matched_model() {
        let missing = t("settings.catalog.not_provided");
        result.lines.push((
            t_args(
                "settings.catalog.capacity",
                &[
                    ("context", &model.context.to_string()),
                    (
                        "input",
                        &model
                            .input
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| missing.into()),
                    ),
                    ("output", &model.output.to_string()),
                ],
            ),
            false,
        ));
        let support = |value| match value {
            Some(true) => t("settings.catalog.supported"),
            Some(false) => t("settings.catalog.unsupported"),
            None => missing,
        };
        result.lines.push((
            t_args(
                "settings.catalog.capabilities",
                &[
                    ("tools", support(model.tool_call)),
                    ("reasoning", support(model.reasoning)),
                ],
            ),
            false,
        ));
        if let Some(canonical) = &model.canonical_id {
            result.lines.push((
                t_args("settings.catalog.canonical", &[("canonical", canonical)]),
                false,
            ));
        }
        if let Some(status) = &model.status {
            result.lines.push((
                t_args("settings.catalog.status", &[("status", status)]),
                true,
            ));
        }
        add(
            "input",
            t("settings.catalog.apply_input").into(),
            Action::Capacity(true),
            false,
        );
        add(
            "output",
            t("settings.catalog.apply_output").into(),
            Action::Capacity(false),
            false,
        );
        if !model.reasoning_options.is_empty() {
            let options = model
                .reasoning_options
                .iter()
                .map(|option| match option {
                    CatalogReasoningOption::Toggle => t("settings.catalog.toggle").into(),
                    CatalogReasoningOption::Effort(values) => values
                        .iter()
                        .map(|value| value.as_deref().unwrap_or("null"))
                        .collect::<Vec<_>>()
                        .join(" / "),
                    CatalogReasoningOption::Budget { min, max } => t_args(
                        "settings.catalog.budget",
                        &[
                            (
                                "min",
                                &min.map(|value| value.to_string())
                                    .unwrap_or_else(|| missing.into()),
                            ),
                            (
                                "max",
                                &max.map(|value| value.to_string())
                                    .unwrap_or_else(|| missing.into()),
                            ),
                        ],
                    ),
                    CatalogReasoningOption::Unknown(value) => value.clone(),
                })
                .collect::<Vec<_>>()
                .join(t("settings.detail_separator"));
            result.lines.push((
                t_args("settings.catalog.reasoning", &[("text", &options)]),
                false,
            ));
        }
        if let Some(cost) = &model.cost {
            result.lines.push((
                t_args(
                    "settings.catalog.prices",
                    &[
                        ("input", cost.input.as_deref().unwrap_or(missing)),
                        ("output", cost.output.as_deref().unwrap_or(missing)),
                        ("cache_read", cost.cache_read.as_deref().unwrap_or(missing)),
                        (
                            "cache_write",
                            cost.cache_write.as_deref().unwrap_or(missing),
                        ),
                    ],
                ),
                false,
            ));
            if cost.complex {
                result
                    .lines
                    .push((t("settings.catalog.complex_pricing").into(), false));
            }
        }
        add(
            "prices",
            t("settings.catalog.apply_prices").into(),
            Action::Prices,
            false,
        );
        result
            .lines
            .push((t("settings.catalog.shared_prices_hint").into(), false));
    } else if snapshot.projection.mapped.is_some() {
        result
            .lines
            .push((t("settings.catalog.mapped_missing").into(), true));
    }
    if let Some(message) = &snapshot.message {
        result.lines.push((message.clone(), false));
    }
    result
}
fn render(state: Signal<Snapshot>, commands: mpsc::Sender<Request>) -> AnyElement {
    let pending = signal(false);
    let failed = signal(false);
    let query = signal(String::new());
    let measured = signal(0.0f32);
    let layout = commands.clone();
    let buttons = commands.clone();
    let search = element_ref();
    let focused = signal(false);
    search.on_focus_in(move || focused.set(true));
    search.on_focus_out(move || focused.set(false));
    let query_owner = signal_with(None::<CatalogIdentity>, SignalEquals::Never);
    let query_provider = signal(None::<String>);
    let mapping = signal(false);
    effect(move || {
        let snapshot = state.get();
        if query_owner.get_untracked() != snapshot.owner
            || query_provider.get_untracked() != snapshot.provider
            || mapping.get_untracked() != snapshot.mapping
        {
            query.set(String::new());
            query_owner.set(snapshot.owner);
            query_provider.set(snapshot.provider);
            mapping.set(snapshot.mapping);
        }
    });
    view()
        .w_full()
        .flex_shrink_0()
        .gap_2()
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
                        failed.set(true);
                    }
                });
            }
        })
        .child(when(
            move || state.get().open && state.get().mapping,
            move || {
                nsh_base::input()
                    .element_ref(&search)
                    .label(move || state.with(|_| t("settings.catalog.search_placeholder")))
                    .value(move || query.get())
                    .on_change(move |text: &str| query.set(text.into()))
                    .border(px(1.))
                    .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                    .px_3()
                    .py_2()
                    .w_full()
                    .border_color(move || {
                        crate::nsh::color(if focused.get() {
                            state.get().colors.accent
                        } else {
                            state.get().colors.border_subtle
                        })
                    })
            },
        ))
        .child(each(
            move || {
                content(&state.get(), &query.get().to_lowercase())
                    .lines
                    .into_iter()
                    .enumerate()
                    .collect::<Vec<_>>()
            },
            |(index, _)| *index,
            move |line, _| {
                text(move || line.get().1.0).text_color(move || {
                    crate::nsh::color(if line.get().1.1 {
                        state.get().colors.warning
                    } else {
                        state.get().colors.text_secondary
                    })
                })
            },
        ))
        .child(each(
            move || {
                let snapshot = state.get();
                content(&snapshot, &query.get().to_lowercase())
                    .controls
                    .into_iter()
                    .map(|control| (control, Arc::new(snapshot.clone())))
                    .collect::<Vec<_>>()
            },
            |(control, _)| control.key.clone(),
            move |item, _| {
                let target = element_ref();
                let focused = signal(false);
                target.on_focus_in(move || focused.set(true));
                target.on_focus_out(move || focused.set(false));
                let commands = buttons.clone();
                view()
                    .element_ref(&target)
                    .role(AccessibilityRole::Button)
                    .focusable(true)
                    .label(move || item.get().0.label)
                    .accessibility_state(move || AccessibilityState {
                        selected: item.get().0.selected,
                        ..Default::default()
                    })
                    .disabled(move || {
                        pending.get() || !accepts(&state.get(), &item.get().1, &item.get().0.action)
                    })
                    .border(px(1.))
                    .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                    .px_3()
                    .py_2()
                    .border_color(move || {
                        crate::nsh::color(if focused.get() {
                            state.get().colors.accent
                        } else {
                            state.get().colors.border_subtle
                        })
                    })
                    .bg(move || {
                        crate::nsh::color(if item.get().0.selected {
                            state.get().colors.bg_active
                        } else {
                            state.get().colors.bg_elevated
                        })
                    })
                    .on_press(move |_: &PressEvent| {
                        let (control, observed) = item.get_untracked();
                        if pending.get_untracked()
                            || !accepts(&state.get_untracked(), &observed, &control.action)
                        {
                            return;
                        }
                        pending.set(true);
                        let commands = commands.clone();
                        spawn(async move {
                            let (reply, response) = oneshot::channel();
                            let accepted = commands
                                .send(Request::Decide(Box::new(Decision {
                                    observed: (*observed).clone(),
                                    action: control.action,
                                    reply,
                                })))
                                .await
                                .is_ok()
                                && matches!(response.await, Ok(true));
                            failed.set(!accepted);
                            pending.set(false);
                        });
                    })
                    .child(text(move || item.get().0.label))
            },
        ))
        .child(when(
            move || failed.get(),
            move || {
                text(move || state.with(|_| t("ui.command_failed")))
                    .text_color(move || crate::nsh::color(state.get().colors.danger))
            },
        ))
        .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vega_conversation::types::{CatalogCost, CatalogModel, CatalogProvider};

    fn snapshot() -> Snapshot {
        let target = CatalogIdentity {
            provider: "catalog".into(),
            model: "model".into(),
        };
        Snapshot {
            id: ProjectionId::new(),
            version: ProjectionId::new(),
            visible: true,
            owner: Some(CatalogIdentity {
                provider: "configured".into(),
                model: "actual-model".into(),
            }),
            generation: 4,
            projection: ModelCatalogProjection {
                providers: Arc::new(vec![CatalogProvider {
                    id: "catalog".into(),
                    name: "Catalog".into(),
                    models: vec![CatalogModel {
                        id: "model".into(),
                        name: "Model".into(),
                        canonical_id: None,
                        context: 100,
                        input: Some(80),
                        output: 20,
                        reasoning: None,
                        tool_call: None,
                        reasoning_options: vec![],
                        text_pricing: true,
                        status: None,
                        cost: Some(CatalogCost {
                            input: Some("0001.234567".into()),
                            output: Some("0.000001".into()),
                            cache_read: Some("0".into()),
                            cache_write: Some("2".into()),
                            complex: false,
                        }),
                    }],
                }]),
                mapped: Some(target.clone()),
                ..Default::default()
            },
            open: true,
            mapping: true,
            provider: Some("catalog".into()),
            candidate: Some(target),
            capacity: Some((3, "model".into(), "80".into(), "20".into(), true, false)),
            capacity_draft: Some(
                crate::settings::provider_management::ProviderDraftStamp::development_fixture(),
            ),
            prices: true,
            help: false,
            message: None,
            error: None,
            colors: vega_theme::Theme::light().colors,
        }
    }
    #[test]
    fn catalog_mapping_rejects_replaced_directory_route_and_candidate() {
        let observed = snapshot();
        assert!(accepts(&observed, &observed, &Action::Confirm));
        let mut current = observed.clone();
        current.version = ProjectionId::new();
        assert!(!accepts(&current, &observed, &Action::Confirm));
        current = observed.clone();
        current.owner.as_mut().unwrap().model = "another-model".into();
        assert!(!accepts(&current, &observed, &Action::Confirm));
        current = observed.clone();
        current.candidate.as_mut().unwrap().model = "removed".into();
        assert!(!allowed(&current, &Action::Confirm));
        current = observed.clone();
        current.visible = false;
        assert!(!accepts(&current, &observed, &Action::Confirm));
    }
    #[test]
    fn catalog_prefills_require_current_editable_draft_and_preserve_exact_rates() {
        let observed = snapshot();
        assert!(accepts(&observed, &observed, &Action::Capacity(true)));
        assert!(accepts(&observed, &observed, &Action::Prices));
        assert_eq!(
            simple_prices(observed.projection.matched_model().unwrap()).unwrap()[0],
            "0001.234567"
        );
        let mut current = observed.clone();
        current.capacity.as_mut().unwrap().2 = "new edit".into();
        assert!(!accepts(&current, &observed, &Action::Capacity(true)));
        current = observed.clone();
        current
            .capacity_draft
            .as_mut()
            .unwrap()
            .development_change(false);
        assert!(!accepts(&current, &observed, &Action::Capacity(true)));
        current
            .capacity_draft
            .as_mut()
            .unwrap()
            .development_change(true);
        assert!(!allowed(&current, &Action::Capacity(true)));
        current = observed.clone();
        current.projection.refreshing = true;
        assert!(!allowed(&current, &Action::Prices));
        assert!(!allowed(&current, &Action::Confirm));
        assert!(allowed(&current, &Action::Toggle));
        current = observed.clone();
        current.prices = false;
        current.capacity = None;
        assert!(!allowed(&current, &Action::Prices));
        assert!(!allowed(&current, &Action::Capacity(false)));
    }
}
