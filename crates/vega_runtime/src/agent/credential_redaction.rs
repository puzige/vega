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
    let mut redacted = text.to_string();
    let mut ordered = credentials
        .iter()
        .filter(|credential| !credential.is_empty())
        .collect::<Vec<_>>();
    ordered.sort_by_key(|credential| std::cmp::Reverse(credential.len()));
    ordered.dedup();
    for credential in ordered {
        redacted = redacted.replace(credential.as_str(), PROVIDER_CREDENTIAL_REDACTION_MARKER);
    }
    redact_common_credential_formats(&redacted)
}

pub fn contains_sensitive_credential(text: &str, credentials: &[String]) -> bool {
    redact_sensitive_credential_text(text, credentials) != text
}

fn redact_common_credential_formats(text: &str) -> String {
    let lowercase = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let lowercase_bytes = lowercase.as_bytes();
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if let Some((start, end)) = credential_range_at(bytes, lowercase_bytes, cursor) {
            result.push_str(&text[cursor..start]);
            result.push_str(PROVIDER_CREDENTIAL_REDACTION_MARKER);
            cursor = end;
        } else {
            let next = text[cursor..]
                .chars()
                .next()
                .map_or(bytes.len(), |character| cursor + character.len_utf8());
            result.push_str(&text[cursor..next]);
            cursor = next;
        }
    }
    result
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
        if end.saturating_sub(value_start) >= 8 {
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
    (end.saturating_sub(value_start) >= 8).then_some((value_start, end))
}

fn jwt_range(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    if !has_token_boundary_before(bytes, start) || !bytes[start..].starts_with(b"eyJ") {
        return None;
    }
    let end = credential_token_end(bytes, start);
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

fn assignment_value_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while bytes.get(end).is_some_and(|byte| {
        !byte.is_ascii_whitespace()
            && !matches!(
                *byte,
                b'"' | b'\'' | b',' | b';' | b'&' | b'}' | b']' | b')'
            )
    }) {
        end += 1;
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

    const SECRET: &str = "canary-provider-credential-output-170-abcdefghijklmnopqrstuvwxyz";

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
