use std::collections::HashMap;

use gpui_kit::*;
use vega_conversation::types::{NavigationRoute, UserQuestionId};

use crate::app_agent::RunCompletionOutcome;
use crate::window::VegaWindow;

#[derive(Default)]
struct NotificationTargets {
    routes: HashMap<SharedString, NavigationRoute>,
    identity_initialized: bool,
}

impl Global for NotificationTargets {}

pub(crate) fn init(cx: &mut App) {
    if cx
        .try_global::<NotificationTargets>()
        .is_some_and(|state| state.identity_initialized)
    {
        return;
    }
    cx.set_app_identity("ai.vega", "Vega");
    if cx.try_global::<NotificationTargets>().is_none() {
        cx.set_global(NotificationTargets::default());
    }
    cx.global_mut::<NotificationTargets>().identity_initialized = true;
}

pub(crate) fn bind(window: WindowHandle<VegaWindow>, root: WeakEntity<VegaWindow>, cx: &mut App) {
    init(cx);
    cx.on_system_notification_response(move |response, cx| {
        let Some(route) = cx
            .try_global::<NotificationTargets>()
            .and_then(|targets| targets.routes.get(&response.tag))
            .cloned()
        else {
            return;
        };
        let root = root.clone();
        cx.defer(move |cx| {
            if root.upgrade().is_none() {
                return;
            }
            let _ = window.update(cx, |owner, window, cx| {
                owner.open_completion_notification(&response.tag, &route, window, cx);
            });
        });
    });
}

pub(crate) fn dismiss(tag: &str, cx: &mut App) {
    if cx.try_global::<NotificationTargets>().is_some() {
        cx.global_mut::<NotificationTargets>().routes.remove(tag);
    }
    cx.dismiss_system_notification(tag);
}

pub(crate) fn show_question(
    route: NavigationRoute,
    title: &str,
    generation: u64,
    queue_generation: &str,
    request_id: &UserQuestionId,
    cx: &mut App,
) -> Option<SharedString> {
    init(cx);
    if !cx
        .try_global::<NotificationTargets>()
        .is_some_and(|targets| targets.identity_initialized)
    {
        return None;
    }
    let NavigationRoute::Task { task, .. } = &route else {
        return None;
    };
    let tag: SharedString =
        format!("vega-question-{task}-{generation}-{queue_generation}-{request_id}").into();
    cx.global_mut::<NotificationTargets>()
        .routes
        .insert(tag.clone(), route);
    let title = title.trim();
    cx.show_system_notification(SystemNotification {
        tag: tag.clone(),
        title: if title.is_empty() { "新任务" } else { title }
            .to_owned()
            .into(),
        body: "Waiting for your answer".into(),
        actions: Vec::new(),
    });
    Some(tag)
}

pub(crate) fn show(
    route: NavigationRoute,
    title: &str,
    generation: u64,
    outcome: RunCompletionOutcome,
    output: &str,
    cx: &mut App,
) {
    let clean = output
        .chars()
        .filter(|character| !character.is_control() || character.is_whitespace())
        .collect::<String>();
    let summary = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.is_empty() {
        return;
    }
    let body = match outcome {
        RunCompletionOutcome::Completed => summary,
        RunCompletionOutcome::Failed => format!("任务失败：{summary}"),
        RunCompletionOutcome::Interrupted => format!("任务已中断：{summary}"),
    };
    let mut characters = body.chars();
    let mut body = characters.by_ref().take(160).collect::<String>();
    if characters.next().is_some() {
        body.pop();
        body.push('…');
    }
    let NavigationRoute::Task { task, .. } = &route else {
        return;
    };
    let tag: SharedString = format!("vega-run-{task}-{generation}").into();
    if cx.try_global::<NotificationTargets>().is_none() {
        cx.set_global(NotificationTargets::default());
    }
    cx.global_mut::<NotificationTargets>()
        .routes
        .insert(tag.clone(), route);
    let title = title.trim();
    cx.show_system_notification(SystemNotification {
        tag,
        title: if title.is_empty() { "新任务" } else { title }
            .to_owned()
            .into(),
        body: body.into(),
        actions: Vec::new(),
    });
}
