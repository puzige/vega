use super::*;
use vega_ui::localized_copy::LocalizedCopy;

impl VegaWindow {
    pub(crate) fn request_privacy(
        &mut self,
        stream: Entity<ConversationStream>,
        request: &PrivacyRequested,
        cx: &mut Context<Self>,
    ) {
        if self
            .agent_controller
            .active
            .contains_key(&request.thread_id)
        {
            stream.update(cx, |stream, cx| {
                stream.apply_privacy_result(Err(LocalizedCopy::new("app.privacy.wait")), cx)
            });
            return;
        }
        let Some(database) = self.context_database(cx) else {
            stream.update(cx, |stream, cx| {
                stream.apply_privacy_result(Err(LocalizedCopy::new("app.privacy.read_retry")), cx)
            });
            return;
        };
        let request = request.clone();
        let thread_id = request.thread_id.clone();
        let generation = self.agent_controller.next_generation;
        let replay = cx
            .try_global::<crate::app_agent::AppReplaySessions>()
            .map(|sessions| sessions.0.clone());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = vega_store::Store::open(database)
                        .map_err(|_| vega_i18n::t("app.privacy.read_failed").to_string())?;
                    let changed = !matches!(&request.action, PrivacyAction::Review);
                    match request.action {
                        PrivacyAction::Review => {}
                        PrivacyAction::Exception {
                            snapshot,
                            index,
                            allow,
                        } => vega_conversation::privacy::set_exception(
                            &store,
                            &request.thread_id,
                            &snapshot,
                            index,
                            allow,
                        )
                        .map_err(|error| error.to_string())?,
                        PrivacyAction::Clean { snapshot } => {
                            vega_conversation::privacy::permanently_clean_with_replay(
                                &store,
                                &request.thread_id,
                                &snapshot,
                                replay.as_ref(),
                            )
                            .map_err(|error| error.to_string())?
                        }
                    }
                    let review = vega_conversation::privacy::review(&store, &request.thread_id)
                        .map_err(|error| error.to_string())?;
                    let page = if changed {
                        Some(
                            vega_conversation::history::latest_history_page(
                                &store,
                                &request.thread_id,
                                vega_store::messages::PAGE_LIMIT,
                            )
                            .map_err(|error| error.to_string())?,
                        )
                    } else {
                        None
                    };
                    Ok::<_, String>((review, page))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let stale = this.agent_controller.next_generation != generation
                    || this.agent_controller.active.contains_key(&thread_id);
                if !stale && result.as_ref().is_ok_and(|(_, page)| page.is_some()) {
                    this.refresh_subagents_after_privacy(&thread_id, cx);
                }
                stream.update(cx, |stream, cx| {
                    if stale {
                        stream.apply_privacy_result(
                            Err(LocalizedCopy::new("app.privacy.changed")),
                            cx,
                        );
                        return;
                    }
                    match result {
                        Ok((review, page)) => {
                            if let Some(page) = page {
                                let anchor =
                                    page.entries.iter().rev().find_map(|entry| {
                                        match entry {
                            vega_conversation::history::HistoryEntry::UserText {
                                message_id,
                                ..
                            }
                            | vega_conversation::history::HistoryEntry::AssistantText {
                                message_id,
                                ..
                            } => Some(message_id.clone()),
                            _ => None,
                        }
                                    });
                                if let Some(anchor) = anchor {
                                    stream.replace_history_window(page, &anchor, cx);
                                }
                            }
                            stream.apply_privacy_result(Ok(review), cx);
                        }
                        Err(error) => stream.apply_privacy_result(
                            Err(crate::app_localization::localized_error(&error)),
                            cx,
                        ),
                    }
                });
            });
        })
        .detach();
    }
}
