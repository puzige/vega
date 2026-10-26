use crate::types::*;

pub fn parse_command(text: &str) -> Result<Option<AutomationCommand>, String> {
    let text = text.trim();
    let (name, mut rest) = token(text);
    let kind = match name {
        "/goal" => AutomationKind::Goal,
        "/loop" => AutomationKind::Loop,
        _ => return Ok(None),
    };
    if rest.is_empty() {
        return Ok(Some(AutomationCommand::Status(kind)));
    }
    let (first, tail) = token(rest);
    if tail.is_empty()
        && let Some(control) = match first {
            "pause" => Some(AutomationControl::Pause),
            "resume" => Some(AutomationControl::Resume),
            "clear" => Some(AutomationControl::Clear),
            _ => None,
        }
    {
        return Ok(Some(AutomationCommand::Control(kind, control)));
    }
    let mut token_budget = None;
    let mut interval_seconds = None;
    if first == "--" {
        rest = tail;
    } else if kind == AutomationKind::Goal && first == "--budget" {
        let (budget, instruction) = token(tail);
        token_budget = Some(
            budget
                .chars()
                .all(|c| c.is_ascii_digit())
                .then(|| budget.parse::<u64>().ok())
                .flatten()
                .filter(|value| *value > 0 && *value <= i64::MAX as u64)
                .ok_or(vega_i18n::t("domain.automation.parse.budget"))?,
        );
        rest = instruction.strip_prefix("-- ").unwrap_or(instruction);
    } else if kind == AutomationKind::Loop
        && first.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+')
        && !first.chars().all(|c| c.is_ascii_digit())
    {
        let (number, multiplier) = [("s", 1), ("m", 60), ("h", 3600), ("d", 86400)]
            .into_iter()
            .find_map(|(suffix, multiplier)| {
                first
                    .strip_suffix(suffix)
                    .map(|number| (number, multiplier))
            })
            .ok_or(vega_i18n::t("domain.automation.parse.interval"))?;
        interval_seconds = Some(
            number
                .chars()
                .all(|c| c.is_ascii_digit())
                .then(|| number.parse::<u64>().ok())
                .flatten()
                .and_then(|value| value.checked_mul(multiplier))
                .filter(|value| (60..=86400).contains(value))
                .ok_or(vega_i18n::t("domain.automation.parse.interval_range"))?,
        );
        rest = tail.strip_prefix("-- ").unwrap_or(tail);
    } else if first.starts_with("--") {
        return Err(vega_i18n::t("domain.automation.parse.unknown_option").into());
    }
    let instruction = rest.trim();
    if instruction.is_empty() {
        return Err(vega_i18n::t("domain.automation.parse.instruction").into());
    }
    if instruction.len() > 16 * 1024 {
        return Err(vega_i18n::t("domain.automation.parse.instruction_size").into());
    }
    Ok(Some(AutomationCommand::Create {
        kind,
        instruction: instruction.to_string(),
        token_budget,
        interval_seconds,
    }))
}

fn token(text: &str) -> (&str, &str) {
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(first, rest)| (first, rest.trim_start()))
}
