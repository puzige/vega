use serde_json::Value;

pub fn split_tool_description(raw: &str) -> Result<(String, Option<String>), serde_json::Error> {
    let mut input = crate::json::parse_unique_value(raw)?;
    let Some(description) = input
        .as_object_mut()
        .and_then(|object| object.remove("description"))
    else {
        return Ok((raw.to_string(), None));
    };
    let description = description.as_str().map(str::to_owned);
    Ok((input.to_string(), description))
}

pub fn tool_inputs_equal_ignoring_description(left: &str, right: &str) -> bool {
    let without_description = |raw: &str| {
        let mut input = crate::json::parse_unique_value(raw).ok()?;
        input.as_object_mut()?.remove("description");
        Some(input)
    };
    match (without_description(left), without_description(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

pub fn tool_description_title(text: &str) -> Option<String> {
    let text: String = text
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                character
            }
        })
        .collect();
    let limited: String = text.trim().chars().take(120).collect();
    let mut title = limited.trim_end();
    loop {
        let trimmed = title.trim_end_matches('。').trim_end();
        let trimmed = match trimmed.strip_suffix('.') {
            Some(prefix)
                if prefix.chars().next_back().is_some_and(|character| {
                    !character.is_whitespace() && !matches!(character, '.' | '/' | '\\')
                }) =>
            {
                prefix
            }
            _ => trimmed,
        };
        if title == trimmed {
            break;
        }
        title = trimmed;
    }
    (!title.is_empty()).then(|| title.to_owned())
}

pub fn description_field_schema() -> Value {
    serde_json::json!({
        "type": ["string", "null"],
        "maxLength": 200,
        "description": "Write a concise action title using active voice and the user's language, without sentence-ending punctuation; use 5–10 words for simple actions. For read, use 'Read project instructions'. Display only; never substitutes for actual arguments or approval."
    })
}
