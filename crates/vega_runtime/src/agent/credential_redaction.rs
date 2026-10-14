use std::sync::Arc;

pub const PROVIDER_CREDENTIAL_REDACTION_MARKER: &str = "[REDACTED:provider_credential]";

pub type CredentialReader = Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>;

const COMMON_PREFIXES: [&str; 20] = [
    "github_pat_",
    "sk-svcacct-",
    "sk-ant-",
    "sk-proj-",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "xoxb-",
    "xoxp-",
    "xoxs-",
    "xoxa-",
    "xapp-",
    "AIza",
    "npm_",
    "pypi-",
    "glpat-",
    "AKIA",
    "sk-",
];

const ASSIGNMENT_KEYS: [&str; 15] = [
    "api_key",
    "api-key",
    "apikey",
    "access_token",
    "access-token",
    "refresh_token",
    "refresh-token",
    "client_secret",
    "client-secret",
    "private_key",
    "private-key",
    "auth_token",
    "auth-key",
    "authorization",
    "password",
];

pub fn redact_sensitive_credential_text(text: &str, credentials: &[String]) -> String {
    redact_sensitive_credential_text_with_offsets(text, credentials, &[]).0
}

pub fn redact_sensitive_credential_text_with_offsets(
    text: &str,
    credentials: &[String],
    offsets: &[usize],
) -> (String, Vec<usize>) {
    let mut redacted = text.to_string();
    let mut mapped_offsets = offsets.to_vec();
    let mut ordered = credentials
        .iter()
        .filter(|credential| !credential.is_empty())
        .collect::<Vec<_>>();
    ordered.sort_by_key(|credential| std::cmp::Reverse(credential.len()));
    ordered.dedup();
    for credential in ordered {
        (redacted, mapped_offsets) = replace_text_with_offsets(
            &redacted,
            credential,
            PROVIDER_CREDENTIAL_REDACTION_MARKER,
            &mapped_offsets,
        );
        let Ok(encoded) = serde_json::to_string(credential.as_str()) else {
            continue;
        };
        let Some(escaped) = encoded.get(1..encoded.len().saturating_sub(1)) else {
            continue;
        };
        (redacted, mapped_offsets) = replace_text_with_offsets(
            &redacted,
            escaped,
            PROVIDER_CREDENTIAL_REDACTION_MARKER,
            &mapped_offsets,
        );
    }
    redact_common_credential_formats_with_offsets(&redacted, &mapped_offsets)
}

pub fn contains_sensitive_credential(text: &str, credentials: &[String]) -> bool {
    redact_sensitive_credential_text(text, credentials) != text
}

fn redact_common_credential_formats_with_offsets(
    text: &str,
    offsets: &[usize],
) -> (String, Vec<usize>) {
    let lowercase = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let lowercase_bytes = lowercase.as_bytes();
    let mut cursor = 0usize;
    let mut ranges = Vec::new();
    while cursor < bytes.len() {
        if let Some((start, end)) = credential_range_at(bytes, lowercase_bytes, cursor) {
            ranges.push((start, end));
            cursor = end;
        } else {
            let next = text[cursor..]
                .chars()
                .next()
                .map_or(bytes.len(), |character| cursor + character.len_utf8());
            cursor = next;
        }
    }
    replace_ranges_with_offsets(text, &ranges, PROVIDER_CREDENTIAL_REDACTION_MARKER, offsets)
}

fn replace_text_with_offsets(
    text: &str,
    needle: &str,
    replacement: &str,
    offsets: &[usize],
) -> (String, Vec<usize>) {
    if needle.is_empty() {
        return (text.to_string(), offsets.to_vec());
    }
    let ranges = text
        .match_indices(needle)
        .map(|(start, matched)| (start, start + matched.len()))
        .collect::<Vec<_>>();
    replace_ranges_with_offsets(text, &ranges, replacement, offsets)
}

fn replace_ranges_with_offsets(
    text: &str,
    ranges: &[(usize, usize)],
    replacement: &str,
    offsets: &[usize],
) -> (String, Vec<usize>) {
    if ranges.is_empty() {
        return (text.to_string(), offsets.to_vec());
    }
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for (start, end) in ranges {
        result.push_str(&text[cursor..*start]);
        result.push_str(replacement);
        cursor = *end;
    }
    result.push_str(&text[cursor..]);

    let mut indexed_offsets = offsets.iter().copied().enumerate().collect::<Vec<_>>();
    indexed_offsets.sort_by_key(|(_, offset)| *offset);
    let mut mapped_offsets = vec![0; offsets.len()];
    let mut range_index = 0usize;
    let mut source_cursor = 0usize;
    let mut output_cursor = 0usize;
    for (offset_index, offset) in indexed_offsets {
        let mut mapped = None;
        while let Some((start, end)) = ranges.get(range_index).copied() {
            let output_start = output_cursor + (start - source_cursor);
            if offset < start {
                mapped = Some(output_cursor + (offset - source_cursor));
                break;
            }
            if offset == start {
                mapped = Some(output_start);
                break;
            }
            if offset <= end {
                mapped = Some(output_start + replacement.len());
                break;
            }
            source_cursor = end;
            output_cursor = output_start + replacement.len();
            range_index += 1;
        }
        mapped_offsets[offset_index] =
            mapped.unwrap_or_else(|| output_cursor + (offset - source_cursor));
    }
    (result, mapped_offsets)
}

fn credential_range_at(bytes: &[u8], lowercase: &[u8], start: usize) -> Option<(usize, usize)> {
    if let Some(range) = assignment_value_range(bytes, lowercase, start) {
        return Some(range);
    }
    if let Some(range) = bearer_value_range(bytes, lowercase, start) {
        return Some(range);
    }
    if let Some(range) = jwt_range(bytes, start) {
        return Some(range);
    }
    for prefix in COMMON_PREFIXES {
        if starts_with_ascii_case_insensitive(lowercase, start, prefix.as_bytes())
            && has_token_boundary_before(bytes, start)
        {
            let end = credential_token_end(bytes, start);
            if end.saturating_sub(start) >= 16 {
                return Some((start, end));
            }
        }
    }
    None
}

fn assignment_value_range(bytes: &[u8], lowercase: &[u8], start: usize) -> Option<(usize, usize)> {
    if !has_token_boundary_before(bytes, start) {
        return None;
    }
    for key in ASSIGNMENT_KEYS {
        let key_bytes = key.as_bytes();
        if !starts_with_ascii_case_insensitive(lowercase, start, key_bytes) {
            continue;
        }
        let mut cursor = start + key_bytes.len();
        if bytes.get(cursor) == Some(&b'"') || bytes.get(cursor) == Some(&b'\'') {
            cursor += 1;
        }
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if !matches!(bytes.get(cursor), Some(b'=' | b':')) {
            continue;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'"') || bytes.get(cursor) == Some(&b'\'') {
            cursor += 1;
        }
        let value_start = cursor;
        let end = assignment_value_end(bytes, cursor);
        if high_confidence_token(&bytes[value_start..end]) {
            return Some((value_start, end));
        }
    }
    None
}

fn bearer_value_range(bytes: &[u8], lowercase: &[u8], start: usize) -> Option<(usize, usize)> {
    if !has_token_boundary_before(bytes, start)
        || !starts_with_ascii_case_insensitive(lowercase, start, b"bearer")
    {
        return None;
    }
    let mut cursor = start + b"bearer".len();
    if !bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        return None;
    }
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    let value_start = cursor;
    let end = credential_token_end(bytes, cursor);
    high_confidence_token(&bytes[value_start..end]).then_some((value_start, end))
}

fn jwt_range(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    if !has_token_boundary_before(bytes, start) || !bytes[start..].starts_with(b"eyJ") {
        return None;
    }
    let token_end = credential_token_end(bytes, start);
    let end = if token_end > start && bytes[token_end - 1] == b'.' {
        token_end - 1
    } else {
        token_end
    };
    let token = &bytes[start..end];
    let mut segments = token.split(|byte| *byte == b'.');
    let first = segments.next()?;
    let second = segments.next()?;
    let third = segments.next()?;
    if segments.next().is_some()
        || first.len() < 8
        || second.len() < 8
        || third.len() < 8
        || !token
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    Some((start, end))
}

fn high_confidence_token(value: &[u8]) -> bool {
    if std::str::from_utf8(value).is_ok_and(vega_credentials::is_registered_alias) {
        return false;
    }
    if value.len() < 20
        || !value.iter().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(*byte, b'_' | b'-' | b'.' | b'+' | b'/' | b'=' | b'~')
        })
    {
        return false;
    }
    let mut counts = [0usize; 256];
    for byte in value {
        counts[*byte as usize] += 1;
    }
    let entropy: f64 = counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let probability = *count as f64 / value.len() as f64;
            -probability * probability.log2()
        })
        .sum();
    entropy >= 3.0
}

fn assignment_value_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while bytes.get(end).is_some_and(|byte| {
        !byte.is_ascii_whitespace()
            && !matches!(
                *byte,
                b'"' | b'\'' | b',' | b';' | b'&' | b'}' | b']' | b')'
            )
    }) {
        if bytes[end..].starts_with(PROVIDER_CREDENTIAL_REDACTION_MARKER.as_bytes()) {
            end += PROVIDER_CREDENTIAL_REDACTION_MARKER.len();
        } else {
            end += 1;
        }
    }
    end
}

fn credential_token_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while bytes.get(end).is_some_and(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(*byte, b'_' | b'-' | b'.' | b'+' | b'/' | b'=' | b'~')
    }) {
        end += 1;
    }
    end
}

fn has_token_boundary_before(bytes: &[u8], index: usize) -> bool {
    index == 0 || !bytes[index - 1].is_ascii_alphanumeric() && bytes[index - 1] != b'_'
}

fn starts_with_ascii_case_insensitive(haystack: &[u8], start: usize, needle: &[u8]) -> bool {
    haystack
        .get(start..start.saturating_add(needle.len()))
        .is_some_and(|value| value.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue378_source_code_and_low_confidence_literals_are_preserved() {
        for text in [
            "password: String::new()",
            "api_key: Option<String>",
            "password=abcdefgh",
            "Bearer documentation",
        ] {
            assert_eq!(redact_sensitive_credential_text(text, &[]), text);
            assert!(!contains_sensitive_credential(text, &[]));
            assert!(contains_sensitive_credential(text, &[text.to_string()]));
        }
    }

    const SECRET: &str = "canary-provider-credential-output-170-abcdefghijklmnopqrstuvwxyz";

    #[test]
    fn issue302_redacted_assignments_are_idempotent_without_hiding_following_secrets() {
        let marker = PROVIDER_CREDENTIAL_REDACTION_MARKER;
        for safe in [
            format!("api_key={marker}"),
            format!("{{\"password\":\"{marker}\"}}"),
            format!("authorization={marker}; ordinary text"),
        ] {
            assert_eq!(redact_sensitive_credential_text(&safe, &[]), safe);
            assert!(!contains_sensitive_credential(&safe, &[]));
        }
        for output in [
            format!("api_key={marker}{SECRET}"),
            format!("api_key={marker}; password=remaining-private-value"),
            format!("api_key={marker} {SECRET}"),
        ] {
            assert!(contains_sensitive_credential(
                &output,
                &[SECRET.to_string()]
            ));
            let redacted = redact_sensitive_credential_text(&output, &[SECRET.to_string()]);
            assert!(!redacted.contains(SECRET));
            assert!(!redacted.contains("remaining-private-value"));
            assert_eq!(redact_sensitive_credential_text(&redacted, &[]), redacted);
        }
    }

    #[test]
    fn issue170_exact_credentials_are_redacted_in_all_tool_output_shapes() {
        for output in [
            format!("stdout: {SECRET}"),
            format!("stderr: {SECRET}"),
            format!("{{\"structured\":\"{SECRET}\"}}"),
            format!("Tool error: failed after printing {SECRET}"),
        ] {
            let redacted = redact_sensitive_credential_text(&output, &[SECRET.to_string()]);
            assert!(!redacted.contains(SECRET));
            assert!(redacted.contains(PROVIDER_CREDENTIAL_REDACTION_MARKER));
        }
    }

    #[test]
    fn issue170_exact_redaction_maps_offsets_around_replaced_credentials() {
        let prefix = "前置 ";
        let suffix = " 后置";
        let text = format!("{prefix}{SECRET}{suffix}");
        let start = prefix.len();
        let end = start + SECRET.len();
        let offsets = [0, start, start + 7, end, text.len()];
        let (redacted, mapped) =
            redact_sensitive_credential_text_with_offsets(&text, &[SECRET.to_string()], &offsets);
        let marker_end = start + PROVIDER_CREDENTIAL_REDACTION_MARKER.len();

        assert_eq!(
            redacted,
            format!("{prefix}{PROVIDER_CREDENTIAL_REDACTION_MARKER}{suffix}")
        );
        assert_eq!(
            mapped,
            vec![0, start, marker_end, marker_end, redacted.len()]
        );
        assert!(
            mapped
                .iter()
                .all(|offset| redacted.is_char_boundary(*offset))
        );
    }

    #[test]
    fn issue170_json_escaped_held_credentials_are_redacted() {
        let secret = "fake-\"quoted\\credential-73";
        let output = serde_json::json!({"nested": {"credential": secret}}).to_string();
        let redacted = redact_sensitive_credential_text(&output, &[secret.to_string()]);
        assert!(!redacted.contains("fake-"));
        assert!(redacted.contains(PROVIDER_CREDENTIAL_REDACTION_MARKER));
    }

    #[test]
    fn issue170_exact_credentials_split_across_output_fragments_are_redacted() {
        let fragments = ["prefix ", &SECRET[..23], &SECRET[23..], " suffix"];
        let assembled = fragments.concat();
        let redacted = redact_sensitive_credential_text(&assembled, &[SECRET.to_string()]);
        assert!(!redacted.contains(SECRET));
        assert!(redacted.contains(PROVIDER_CREDENTIAL_REDACTION_MARKER));
    }

    #[test]
    fn issue170_large_outputs_are_redacted_without_dropping_safe_neighbors() {
        let output = format!(
            "{} {SECRET} {}",
            "before ".repeat(20_000),
            "after ".repeat(20_000)
        );
        let redacted = redact_sensitive_credential_text(&output, &[SECRET.to_string()]);
        assert!(!redacted.contains(SECRET));
        assert!(redacted.starts_with("before "));
        assert!(redacted.ends_with("after "));
        assert!(redacted.contains(PROVIDER_CREDENTIAL_REDACTION_MARKER));
    }

    #[test]
    fn issue170_common_provider_formats_are_redacted_without_owner_lookup() {
        for secret in [
            "sk-proj-canary123456789012345678901234567890",
            "ghp_canary1234567890123456789012345678901234",
            "AIzaCanary12345678901234567890123456789012",
            "Bearer canaryBearerToken12345678901234567890",
            "api_key=canary-assigned-secret-value-123456",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signatureCanary1234567890",
        ] {
            let redacted = redact_sensitive_credential_text(secret, &[]);
            assert!(!redacted.contains(secret));
            assert!(redacted.contains(PROVIDER_CREDENTIAL_REDACTION_MARKER));
        }
    }

    #[test]
    fn issue170_common_provider_formats_are_redacted_without_owner_lookup_jwt_period() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signatureCanary1234567890";
        assert_eq!(
            redact_sensitive_credential_text(&format!("{jwt}."), &[]),
            format!("{PROVIDER_CREDENTIAL_REDACTION_MARKER}.")
        );
    }

    #[test]
    fn issue170_safe_text_and_short_assignments_are_preserved() {
        assert_eq!(
            redact_sensitive_credential_text("ordinary text", &[]),
            "ordinary text"
        );
        assert_eq!(
            redact_sensitive_credential_text("token=short", &[]),
            "token=short"
        );
    }
}
