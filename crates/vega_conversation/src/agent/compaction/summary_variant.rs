#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SummaryVariant {
    Control,
    Lean,
    Short,
    Capped,
}

impl SummaryVariant {
    pub(super) fn from_environment() -> Self {
        Self::parse(
            std::env::var("VEGA_COMPACTION_SUMMARY_VARIANT")
                .ok()
                .as_deref(),
        )
    }

    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("lean") => Self::Lean,
            Some("short") => Self::Short,
            Some("capped") => Self::Capped,
            _ => Self::Control,
        }
    }

    pub(super) fn instruction(self) -> &'static str {
        match self {
            Self::Control => "",
            Self::Lean => {
                "\nUse a lean, actionable checkpoint. Deduplicate repeated background and resolved exploration; preserve exact identifiers and decisions needed for the next action. All required safety and task-continuity information must remain."
            }
            Self::Short => {
                "\nUse a short, compact handoff focused on the active task, user corrections, completed work, pending work and the next concrete action. Omit redundant exposition, not constraints or unresolved risks."
            }
            Self::Capped => {
                "\nKeep the summary within 2000 words. Prioritize the active task, user corrections, exact continuation details, constraints, stop and permission boundaries, and unresolved side effects over background detail."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_requests_preserve_task_safety_and_budget_across_variants()
    -> Result<(), Box<dyn std::error::Error>> {
        use super::super::*;

        let history = vec![ChatMessage::new(
            ChatRole::User,
            "Continue the pending repair.",
        )];
        let budget = ContextBudget::new(200_000, 32_000, true)?;
        let request = ContextCompactionRequest {
            trigger: ContextCompactionTrigger::Threshold,
            system_prompt: "system".into(),
            messages: history.clone(),
            tools: Vec::new(),
            budget,
            estimate: estimate_chat_context("system", &history, &[])?,
            target_tokens: budget.target_tokens()?,
            source_version: 1,
            source_fingerprint: None,
            require_source_fence: false,
            source_owner_id: None,
        };
        let groups = vec![history.clone()];
        for variant in [
            SummaryVariant::Control,
            SummaryVariant::Lean,
            SummaryVariant::Short,
            SummaryVariant::Capped,
        ] {
            let outgoing =
                direct_summary_request(&request, "model", None, &groups, history.first(), variant);
            assert_eq!(outgoing.messages[0].content, SUMMARY_SYSTEM_PROMPT);
            assert_eq!(outgoing.messages[1], history[0]);
            assert!(outgoing.tools.is_empty());
            assert_eq!(outgoing.max_tokens, Some(SUMMARY_MAX_TOKENS.min(20_000)));
            let instruction = &outgoing
                .messages
                .last()
                .ok_or("missing instruction")?
                .content;
            assert!(instruction.starts_with(SUMMARY_INSTRUCTION));
            assert!(instruction.ends_with("Continue the pending repair."));
            assert!(instruction.contains("not a new instruction or permission"));
            assert!(instruction.contains(variant.instruction()));
            assert_eq!(
                instruction.contains("2000 words"),
                variant == SummaryVariant::Capped
            );
        }
        Ok(())
    }

    #[test]
    fn invalid_or_missing_selection_falls_back_to_control() {
        for value in [
            None,
            Some(""),
            Some("unknown"),
            Some("SHORT"),
            Some(" short "),
        ] {
            assert_eq!(SummaryVariant::parse(value), SummaryVariant::Control);
        }
        for (value, expected) in [
            ("control", SummaryVariant::Control),
            ("lean", SummaryVariant::Lean),
            ("short", SummaryVariant::Short),
            ("capped", SummaryVariant::Capped),
        ] {
            assert_eq!(SummaryVariant::parse(Some(value)), expected);
        }
    }
}
