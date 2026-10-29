use super::*;

struct AlternatingRefillHook {
    recording: Arc<RecordingHook>,
    requests: Mutex<Vec<ContextCompactionRequest>>,
}

impl ContextCompactionHook for AlternatingRefillHook {
    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'a, Result<ContextCompactionResult, ContextCompactionFailure>> {
        Box::pin(async move {
            let installed = {
                let mut requests = self.requests.lock().unwrap();
                requests.push(request.clone());
                requests.len()
            };
            assert!(installed <= 3, "third rapid refill must not reach the hook");
            let mut result = self.recording.compact(request, cancel).await?;
            for message in &mut result.messages {
                if message.role == ChatRole::Assistant {
                    message.content = "Retained response".into();
                }
            }
            if installed == 2 {
                result.messages[1].content = "a".repeat(120_000);
            }
            result.source_fingerprint = Some(format!("shared-refill-{installed}"));
            Ok(result)
        })
    }
}

fn skill_refill_response(
    name: &str,
    id: &str,
    refill: bool,
) -> Vec<Result<ProviderEvent, VegaError>> {
    vec![
        Ok(ProviderEvent::TextDelta(if refill {
            "a".repeat(330_000)
        } else {
            String::new()
        })),
        Ok(ProviderEvent::ToolUse {
            id: id.into(),
            name: vega_runtime::skills::LOAD_SKILL_TOOL_NAME.into(),
            input_json: serde_json::json!({"name": name}).to_string(),
        }),
        Ok(ProviderEvent::Done {
            stop_reason: StopReason::ToolUse,
        }),
    ]
}

#[tokio::test]
async fn explicit_and_model_skill_refills_share_the_ordinary_rapid_refill_limit() -> TestResult {
    use vega_runtime::ContextAccountingStage::{PrimaryPreflight, SkillProspect};

    for explicit in [false, true] {
        let directory = SkillDirectory(
            std::env::temp_dir().join(format!("vega-skill-refill-{}", ulid::Ulid::generate())),
        );
        for name in ["bounded", "second"] {
            let path = directory.0.join(".agents/skills").join(name);
            std::fs::create_dir_all(&path)?;
            std::fs::write(
                path.join("SKILL.md"),
                format!(
                    "---\nname: {name}\ndescription: Synthetic bounded skill\n---\n{}",
                    "skill detail ".repeat(9000)
                ),
            )?;
        }
        let source = SkillSource::project_approved(&directory.0)
            .map_err(|error| format!("{error:?}"))?
            .ok_or("source")?;
        let discovery = source.discover().map_err(|error| format!("{error:?}"))?;
        assert!(discovery.diagnostics.is_empty());
        assert_eq!(discovery.candidates.len(), 2);
        let selection = SkillSelection::from_candidate(
            discovery
                .candidates
                .iter()
                .find(|candidate| candidate.name == "bounded")
                .ok_or("bounded skill")?,
        );
        let approvals = discovery
            .candidates
            .iter()
            .map(|candidate| SkillApproval::reviewed(candidate, "development", true, true))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{error:?}"))?;
        let catalog = SkillCatalog::freeze(discovery.candidates, &approvals, true)
            .map_err(|error| format!("{error:?}"))?;
        let trace = Trace::default();
        let recording = RecordingHook::new(trace.clone(), false, false);
        let hook = Arc::new(AlternatingRefillHook {
            recording: recording.clone(),
            requests: Mutex::new(Vec::new()),
        });
        let mut request = request(recording.clone());
        request.context_compaction_hook = Some(hook.clone());
        request.history.extend([
            ChatMessage::new(ChatRole::Assistant, "a".repeat(220_000)),
            ChatMessage::new(ChatRole::User, "Continue the task"),
        ]);
        request.tool_config = request.tool_config.with_skill_run(
            SkillRun::new(catalog, true),
            if explicit {
                vec![selection]
            } else {
                Vec::new()
            },
        );
        let mut replies = Vec::new();
        if !explicit {
            replies.push(skill_refill_response("bounded", "initial-load", false));
        }
        replies.extend([
            skill_refill_response("bounded", "ordinary-refill", true),
            skill_refill_response("second", "skill-refill", false),
            skill_refill_response("second", "rejected-refill", true),
        ]);
        let provider = ScriptedProvider::new(trace.clone(), replies);
        let tools = vega_tools::Tools::new(&directory.0)?;
        let mut events = Vec::new();
        let result = run_agent_with_sink(
            &provider,
            &tools,
            request,
            CancellationToken::new(),
            |event| {
                if let RuntimeEvent::ContextAccountingUpdated(decision) = &event {
                    let boundary = match decision.stage {
                        SkillProspect => Some("skill-refill"),
                        PrimaryPreflight if decision.predicted_input >= decision.trigger_tokens => {
                            Some("ordinary-refill")
                        }
                        _ => None,
                    };
                    if let Some(boundary) = boundary {
                        trace.lock().unwrap().push(boundary);
                    }
                }
                events.push(event);
                std::future::ready(Ok(()))
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(VegaError::Context(ContextRuntimeError::OverLimit { .. }))
        ));
        let requests = hook.requests.lock().unwrap();
        assert_eq!(requests.len(), 3, "explicit={explicit}");
        for (index, request) in requests.iter().enumerate() {
            assert_eq!(request.source_owner_id.as_deref(), Some("owner"));
            assert_eq!(request.source_version, index as u64 + 1);
        }
        assert_eq!(recording.compacted.lock().unwrap().len(), 3);
        assert!(provider.replies.lock().unwrap().is_empty());
        let trace = trace.lock().unwrap();
        assert_eq!(
            trace.iter().filter(|entry| **entry == "primary").count(),
            if explicit { 3 } else { 4 },
            "explicit={explicit}"
        );
        let boundaries = trace
            .iter()
            .copied()
            .filter(|entry| *entry != "primary")
            .collect::<Vec<_>>();
        assert_eq!(
            boundaries,
            [
                "skill-refill",
                "compact",
                "ordinary-refill",
                "compact",
                "skill-refill",
                "compact",
                "ordinary-refill",
            ],
            "explicit={explicit}"
        );
        let prospects = events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::ContextAccountingUpdated(decision)
                    if decision.stage == SkillProspect =>
                {
                    Some(decision)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(prospects.len(), 2);
        assert!(
            prospects
                .iter()
                .all(|decision| decision.predicted_input > decision.input_budget)
        );
        let loaded = events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::SkillActivation { audit, .. } if audit.status == "loaded" => {
                    Some(audit.name.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(loaded, ["bounded", "second"]);
    }
    Ok(())
}
