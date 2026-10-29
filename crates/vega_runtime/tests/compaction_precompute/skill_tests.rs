use super::*;
use vega_runtime::skills::{SkillApproval, SkillCatalog, SkillRun, SkillSelection, SkillSource};

#[path = "skill_refill_tests.rs"]
mod skill_refill_tests;

struct SkillDirectory(std::path::PathBuf);

impl Drop for SkillDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn explicit_and_model_skill_compaction_suppress_same_owner_precomputation() -> TestResult {
    for explicit in [false, true] {
        let directory = SkillDirectory(
            std::env::temp_dir().join(format!("vega-precompute-skill-{}", ulid::Ulid::generate())),
        );
        let skill_path = directory.0.join(".agents/skills/bounded");
        std::fs::create_dir_all(&skill_path)?;
        std::fs::write(
            skill_path.join("SKILL.md"),
            format!(
                "---\nname: bounded\ndescription: Synthetic bounded skill\n---\n{}",
                "skill detail ".repeat(9000)
            ),
        )?;
        let source = SkillSource::project_approved(&directory.0)
            .map_err(|error| format!("{error:?}"))?
            .ok_or("source")?;
        let discovery = source.discover().map_err(|error| format!("{error:?}"))?;
        assert!(discovery.diagnostics.is_empty());
        assert_eq!(discovery.candidates.len(), 1);
        let candidate = &discovery.candidates[0];
        let selection = SkillSelection::from_candidate(candidate);
        let approval = SkillApproval::reviewed(candidate, "development", true, true)
            .map_err(|error| format!("{error:?}"))?;
        let catalog = SkillCatalog::freeze(discovery.candidates, &[approval], true)
            .map_err(|error| format!("{error:?}"))?;
        let run = SkillRun::new(catalog, true);
        let trace = Trace::default();
        let hook = RecordingHook::new(trace.clone(), false, false);
        let mut request = request(hook.clone());
        request.history.extend([
            ChatMessage::new(ChatRole::Assistant, "a".repeat(220_000)),
            ChatMessage::new(ChatRole::User, "Continue the task"),
        ]);
        request.tool_config = request.tool_config.with_skill_run(
            run,
            if explicit {
                vec![selection]
            } else {
                Vec::new()
            },
        );
        let mut replies = Vec::new();
        if !explicit {
            replies.push(vec![
                Ok(ProviderEvent::ToolUse {
                    id: "load".into(),
                    name: vega_runtime::skills::LOAD_SKILL_TOOL_NAME.into(),
                    input_json: "{\"name\":\"bounded\"}".into(),
                }),
                Ok(ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                }),
            ]);
        }
        replies.push(response(81_000, StopReason::End));
        let provider = ScriptedProvider::new(trace.clone(), replies);
        let tools = vega_tools::Tools::new(&directory.0)?;
        let outcome = run_agent_with_sink(
            &provider,
            &tools,
            request,
            CancellationToken::new(),
            |event| {
                sink_event(&trace, &event);
                std::future::ready(Ok(()))
            },
        )
        .await?;
        assert!(!outcome.failed);
        assert!(outcome.events.iter().any(|event| matches!(event,
            RuntimeEvent::ContextAccountingUpdated(decision)
                if decision.stage == vega_runtime::ContextAccountingStage::SkillProspect
                    && decision.predicted_input > decision.input_budget
        )));
        if !explicit {
            let decision = outcome
                .events
                .iter()
                .find_map(|event| match event {
                    RuntimeEvent::ContextAccountingUpdated(decision)
                        if decision.stage
                            == vega_runtime::ContextAccountingStage::PrimaryPreflight =>
                    {
                        Some(decision)
                    }
                    _ => None,
                })
                .ok_or("primary preflight")?;
            assert!(decision.predicted_input < 80_000);
        }
        assert_eq!(hook.compacted.lock().unwrap().len(), 1);
        assert!(hook.armed.lock().unwrap().is_empty());
        assert!(outcome.events.iter().any(|event| matches!(event, RuntimeEvent::SkillActivation { audit, .. } if audit.status == "loaded")));
        let trace = trace.lock().unwrap();
        let compact = trace.iter().position(|value| *value == "compact").unwrap();
        let primary = trace.iter().position(|value| *value == "primary").unwrap();
        assert_eq!(compact < primary, explicit);
        assert!(provider.replies.lock().unwrap().is_empty());
    }
    Ok(())
}
