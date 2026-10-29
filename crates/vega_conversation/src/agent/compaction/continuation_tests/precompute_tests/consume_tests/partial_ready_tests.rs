use super::*;
use crate::types::{PartialCompactionDirection as Direction, PartialCompactionSelection};
use vega_store::context_projection::ContinuationMode;

type UsageSnapshot = Vec<(i64, i64, i64, i64)>;

fn text(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ordered(messages: &[ChatMessage], markers: &[&str]) {
    let history = text(messages);
    let positions = markers
        .iter()
        .map(|marker| {
            assert_eq!(history.matches(*marker).count(), 1, "{marker}: {history}");
            history.find(*marker).expect("ordered marker absent")
        })
        .collect::<Vec<_>>();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "unordered history: {history}"
    );
}

#[tokio::test]
async fn partial_summaries_feed_ready_in_order_and_consumption_preserves_task_without_rebilling()
-> TestResult {
    for mode in ["success", "unknown"] {
        let (_directory, path, store) = source_fixture()?;
        let mut request = input(&store)?;
        let mut row = messages::find(store.conn(), "task")?.ok_or("task missing")?;
        for (id, seq, marker) in [("middle", 3, "SOURCE_MIDDLE"), ("tail", 4, "SOURCE_TAIL")] {
            row.id = id.into();
            row.seq = seq;
            row.role = "assistant".into();
            row.content = marker.into();
            messages::insert(store.conn(), &row)?;
        }
        let original = load_source(store.conn(), "thread")?;
        request = current_input(&store, request)?;
        request.trigger = ContextCompactionTrigger::Manual;
        let partial_provider = OrderedSummaryProvider {
            steps: Mutex::new(std::collections::VecDeque::from([
                (vec!["SOURCE_TAIL"], "SUMMARY_TAIL"),
                (
                    vec!["Continue implementation; no local tests."],
                    "SUMMARY_TASK",
                ),
            ])),
        };
        for (id, direction) in [("tail", Direction::From), ("owner", Direction::UpTo)] {
            let result = ConversationCompactionHook::new(
                &partial_provider,
                path.clone(),
                "thread",
                "model",
                None,
                None,
            )
            .with_partial_selection(PartialCompactionSelection {
                message_id: id.into(),
                source_fingerprint: original.fingerprint.clone(),
                direction,
            })
            .compact(request.clone(), CancellationToken::new())
            .await
            .map_err(|failure| *failure.error)?;
            request.messages = result.messages;
            request.estimate =
                estimate_chat_context(&request.system_prompt, &request.messages, &request.tools)?;
            assert_eq!(load_source(store.conn(), "thread")?, original);
        }
        assert!(partial_provider.steps.lock().unwrap().is_empty());
        ordered(
            &request.messages,
            &["SUMMARY_TASK", "result-2", "SOURCE_MIDDLE", "SUMMARY_TAIL"],
        );
        let previous = latest_checkpoint(store.conn(), "thread", "model")?
            .ok_or("partial checkpoint missing")?;
        let partial_plan = previous
            .projection_plan
            .as_ref()
            .ok_or("partial plan missing")?;
        assert_eq!(partial_plan.strategy, ProjectionStrategy::PartialUpTo);
        assert_eq!(partial_plan.task_message_id.as_deref(), Some("task"));
        row.id = "retained".into();
        row.seq = 5;
        row.content = "SOURCE_RETAINED".into();
        messages::insert(store.conn(), &row)?;
        let captured = load_source(store.conn(), "thread")?;
        let mut expected_messages = original.messages.clone();
        expected_messages.push(row.clone());
        assert_eq!(captured.messages, expected_messages);
        assert_eq!(captured.tool_calls, original.tool_calls);
        assert_eq!(captured.images, original.images);
        request = current_input(&store, request)?;
        request.messages = crate::agent::pipeline::history_from_context_source_with_checkpoint(
            &captured,
            Some(&previous),
            "__none__",
        )?;
        request.estimate =
            estimate_chat_context(&request.system_prompt, &request.messages, &request.tools)?;
        request.trigger = ContextCompactionTrigger::Threshold;
        let provider = BackgroundProvider::new(mode, false);
        let hook = ConversationCompactionHook::new(&provider, path, "thread", "model", None, None);
        publish(&hook, request.clone()).await?;
        {
            let requests = provider.0.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            let background = &requests[0];
            let history = &background.messages[1..background.messages.len() - 1];
            ordered(
                history,
                &["SUMMARY_TASK", "result-2", "SOURCE_MIDDLE", "SUMMARY_TAIL"],
            );
            let history = text(history);
            for absent in [
                "SOURCE_TAIL",
                "SOURCE_RETAINED",
                "Continue implementation; no local tests.",
            ] {
                assert!(
                    !history.contains(absent),
                    "unexpected summary input: {absent}"
                );
            }
        }
        let candidate = ready::recover(store.conn(), "thread", now_ms())?.ok_or("ready missing")?;
        assert_eq!(
            candidate
                .checkpoint
                .projection_plan
                .as_ref()
                .ok_or("ready plan missing")?
                .strategy,
            ProjectionStrategy::Precompute
        );
        assert_eq!(count(&store, "context_checkpoints")?, 2);
        assert_eq!(load_source(store.conn(), "thread")?, captured);
        assert_eq!(count(&store, "token_usage")?, i64::from(mode == "success"));
        assert_eq!(count(&store, "context_precompute_stages")?, 1);
        let billing = || -> Result<UsageSnapshot, Box<dyn std::error::Error>> {
            let mut statement = store.conn().prepare(
                "SELECT id,input_tokens,output_tokens,cost_microcents FROM token_usage ORDER BY id",
            )?;
            Ok(statement
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?
                .collect::<Result<UsageSnapshot, _>>()?)
        };
        let settled = billing()?;
        if mode == "success" {
            assert_eq!((settled[0].1, settled[0].2), (100, 10));
        }
        row.id = "continued-owner".into();
        row.seq = 6;
        row.content = "SOURCE_APPENDED".into();
        messages::insert(store.conn(), &row)?;
        let before = load_source(store.conn(), "thread")?;
        expected_messages.push(row);
        assert_eq!(before.messages, expected_messages);
        assert_eq!(before.tool_calls, original.tool_calls);
        assert_eq!(before.images, original.images);
        request = current_input(&store, request)?;
        request.messages = crate::agent::pipeline::history_from_context_source_with_checkpoint(
            &before,
            Some(&previous),
            "__none__",
        )?;
        request.estimate =
            estimate_chat_context(&request.system_prompt, &request.messages, &request.tools)?;
        request.source_owner_id = Some("continued-owner".into());
        let frame = ReplayFrame::new(request.messages.len() + 1, ReasoningReplayPolicy::Unknown);
        let result = hook
            .compact_with_replay(
                request.clone(),
                Some(frame.clone()),
                CancellationToken::new(),
            )
            .await
            .map_err(|failure| *failure.error)?;
        ordered(
            &result.messages,
            &[
                "Continue the implementation.",
                "SOURCE_RETAINED",
                "SOURCE_APPENDED",
            ],
        );
        let final_text = text(&result.messages);
        for absent in [
            "SUMMARY_TASK",
            "SUMMARY_TAIL",
            "SOURCE_MIDDLE",
            "SOURCE_TAIL",
            "result-2",
        ] {
            assert!(
                !final_text.contains(absent),
                "covered source leaked: {absent}"
            );
        }
        let checkpoint = latest_checkpoint(store.conn(), "thread", "model")?
            .ok_or("consumed checkpoint missing")?;
        let plan = checkpoint
            .projection_plan
            .as_ref()
            .ok_or("consumed plan missing")?;
        assert_eq!(plan.strategy, ProjectionStrategy::Precompute);
        assert_eq!(plan.task_message_id.as_deref(), Some("task"));
        assert_eq!(plan.source_owner_id.as_deref(), Some("continued-owner"));
        assert_eq!(plan.continuation, ContinuationMode::ContinueExistingRun);
        let reloaded = crate::agent::pipeline::history_from_context_source_with_checkpoint(
            &before,
            Some(&checkpoint),
            "__none__",
        )?;
        assert_eq!(result.messages, reloaded);
        assert_eq!(load_source(store.conn(), "thread")?, before);
        assert!(result.usages.is_empty());
        let usage_state = if mode == "success" {
            ContextCompactionUsageState::Known { priced: false }
        } else {
            ContextCompactionUsageState::Unknown
        };
        assert_eq!(result.usage_state(), usage_state);
        assert_eq!(result.settled_usage, Some(usage_state));
        assert_eq!(billing()?, settled);
        assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
        assert_eq!(count(&store, "context_checkpoints")?, 3);
        assert!(ready::recover(store.conn(), "thread", now_ms())?.is_none());
        assert!(
            hook.consume_precompute(&request, Some(&frame), &CancellationToken::new())
                .await?
                .is_none()
        );
        assert_eq!(billing()?, settled);
        assert_eq!(count(&store, "context_precompute_stages")?, 1);
        assert_eq!(provider.0.requests.lock().unwrap().len(), 1);
        assert_eq!(count(&store, "context_checkpoints")?, 3);
        assert_eq!(load_source(store.conn(), "thread")?, before);
    }
    Ok(())
}
