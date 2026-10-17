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
                .ok_or("预算须为正整数且不超过 9223372036854775807")?,
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
            .ok_or("间隔须为正整数加 s/m/h/d，例如 5m")?;
        interval_seconds = Some(
            number
                .chars()
                .all(|c| c.is_ascii_digit())
                .then(|| number.parse::<u64>().ok())
                .flatten()
                .and_then(|value| value.checked_mul(multiplier))
                .filter(|value| (60..=86400).contains(value))
                .ok_or("间隔须在 60 秒至 24 小时之间")?,
        );
        rest = tail.strip_prefix("-- ").unwrap_or(tail);
    } else if first.starts_with("--") {
        return Err("未知选项；要将控制词用作任务内容，请加 --".into());
    }
    let instruction = rest.trim();
    if instruction.is_empty() {
        return Err("请输入任务内容".into());
    }
    if instruction.len() > 16 * 1024 {
        return Err("任务内容不能超过 16 KiB".into());
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_preserve_data_and_reject_invalid_limits() {
        assert_eq!(parse_command("/goals ordinary").unwrap(), None);
        assert!(matches!(
            parse_command("/loop 2026 年计划").unwrap(),
            Some(AutomationCommand::Create {
                interval_seconds: None,
                ..
            })
        ));
        assert!(
            matches!(parse_command("/goal -- pause").unwrap(),Some(AutomationCommand::Create {instruction,..}) if instruction == "pause")
        );
        for text in [
            "/goal --budget 0 x",
            "/goal --budget -1 x",
            "/goal --budget 18446744073709551616 x",
            "/goal --",
            "/loop 1s x",
            "/loop 25h x",
            "/loop 1.5m x",
            "/loop +1m x",
            "/loop -1m x",
            "/goal --budget +10 x",
            "/loop 5分钟 检查",
            "/loop 1💡 检查",
        ] {
            assert!(parse_command(text).is_err(), "{text}");
        }
        assert!(parse_command(&format!("/goal {}", "a".repeat(16385))).is_err());
        assert!(matches!(
            parse_command("/loop 5m check").unwrap(),
            Some(AutomationCommand::Create {
                interval_seconds: Some(300),
                ..
            })
        ));
        assert!(matches!(
            parse_command("/loop check").unwrap(),
            Some(AutomationCommand::Create {
                interval_seconds: None,
                ..
            })
        ));
    }
}
