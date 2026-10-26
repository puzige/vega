use std::sync::Arc;

pub const PROVIDER_CREDENTIAL_REDACTION_MARKER: &str = "[REDACTED:provider_credential]";

pub type CredentialReader = Arc<dyn Fn() -> Result<Vec<String>, ()> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CredentialConfidence {
    Low,
    High,
    KnownExact,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialFinding {
    pub rule_id: &'static str,
    pub start: usize,
    pub end: usize,
    pub confidence: CredentialConfidence,
}

pub type CredentialExceptionFilter = Arc<dyn Fn(&str, &CredentialFinding) -> bool + Send + Sync>;

pub fn credential_stream_safe_end(text: &str, known: &[String]) -> usize {
    let mut safe_end = text.len();
    for secret in known.iter().filter(|secret| !secret.is_empty()) {
        let mut forms = vec![secret.clone()];
        if let Ok(encoded) = serde_json::to_string(secret) {
            forms.push(encoded[1..encoded.len() - 1].into());
        }
        for form in forms {
            let lower_bound = text.len().saturating_sub(form.len());
            for (start, _) in text
                .char_indices()
                .filter(|(start, _)| *start >= lower_bound)
            {
                if form.starts_with(&text[start..]) {
                    safe_end = safe_end.min(start);
                    break;
                }
            }
        }
    }
    let lowercase = text.to_ascii_lowercase();
    let lower_bound = text.len().saturating_sub(256);
    for (start, _) in text
        .char_indices()
        .filter(|(start, _)| *start >= lower_bound)
    {
        if !has_token_boundary_before(text.as_bytes(), start) {
            continue;
        }
        let suffix = &lowercase[start..];
        let potential = COMMON_PREFIXES
            .iter()
            .chain(ASSIGNMENT_KEYS.iter())
            .copied()
            .chain(["bearer", "eyj"])
            .any(|prefix| {
                let prefix = prefix.to_ascii_lowercase();
                prefix.starts_with(suffix)
                    || (suffix.starts_with(&prefix)
                        && !suffix[prefix.len()..].contains(['\n', '\r', ';', ',', '}', ']']))
            });
        if potential {
            safe_end = safe_end.min(start);
        }
    }
    safe_end
}

pub fn inspect_credential_text(text: &str, known: &[String]) -> Vec<CredentialFinding> {
    let mut findings = Vec::new();
    for secret in known.iter().filter(|secret| !secret.is_empty()) {
        let mut forms = vec![secret.clone()];
        if let Ok(encoded) = serde_json::to_string(secret) {
            forms.push(encoded[1..encoded.len() - 1].into());
        }
        for form in forms {
            for (start, _) in text.match_indices(&form) {
                findings.push(CredentialFinding {
                    rule_id: "owner_exact",
                    start,
                    end: start + form.len(),
                    confidence: CredentialConfidence::KnownExact,
                });
            }
        }
    }
    let lowercase = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let candidate = if let Some((start, end)) =
            credential_range_at(bytes, lowercase.as_bytes(), cursor)
        {
            Some((start, end, CredentialConfidence::High, "credential_format"))
        } else {
            assignment_candidate_range(bytes, lowercase.as_bytes(), cursor).map(|(start, end)| {
                (
                    start,
                    end,
                    CredentialConfidence::Low,
                    "credential_assignment",
                )
            })
        };
        if let Some((start, end, confidence, rule_id)) = candidate {
            let value = &text[start..end];
            if value != PROVIDER_CREDENTIAL_REDACTION_MARKER
                && !vega_credentials::is_registered_alias(value)
            {
                findings.push(CredentialFinding {
                    rule_id,
                    start,
                    end,
                    confidence,
                });
            }
            cursor = if confidence == CredentialConfidence::Low {
                cursor + text[cursor..].chars().next().map_or(1, char::len_utf8)
            } else {
                end
            };
        } else {
            cursor += text[cursor..].chars().next().map_or(1, char::len_utf8);
        }
    }
    findings.sort_by_key(|finding| (finding.start, std::cmp::Reverse(finding.end)));
    let mut merged: Vec<CredentialFinding> = Vec::new();
    for finding in findings {
        if let Some(last) = merged.last_mut()
            && finding.start < last.end
        {
            last.end = last.end.max(finding.end);
            if finding.confidence > last.confidence {
                last.confidence = finding.confidence;
                last.rule_id = finding.rule_id;
            }
        } else {
            merged.push(finding);
        }
    }
    merged
}

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
    redact_sensitive_credential_text_with_offsets_and_policy(text, credentials, offsets, None)
}

pub fn redact_sensitive_credential_text_with_policy(
    text: &str,
    credentials: &[String],
    exceptions: Option<&CredentialExceptionFilter>,
) -> String {
    redact_sensitive_credential_text_with_offsets_and_policy(text, credentials, &[], exceptions).0
}

pub fn redact_sensitive_credential_text_with_offsets_and_policy(
    text: &str,
    credentials: &[String],
    offsets: &[usize],
    exceptions: Option<&CredentialExceptionFilter>,
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
    redact_common_credential_formats_with_offsets(&redacted, &mapped_offsets, exceptions)
}

pub fn contains_sensitive_credential(text: &str, credentials: &[String]) -> bool {
    redact_sensitive_credential_text(text, credentials) != text
}

fn redact_common_credential_formats_with_offsets(
    text: &str,
    offsets: &[usize],
    exceptions: Option<&CredentialExceptionFilter>,
) -> (String, Vec<usize>) {
    let ranges = inspect_credential_text(text, &[])
        .into_iter()
        .filter(|finding| {
            finding.confidence != CredentialConfidence::Low
                && !exceptions.is_some_and(|filter| filter(text, finding))
        })
        .map(|finding| (finding.start, finding.end))
        .collect::<Vec<_>>();
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
    assignment_candidate_range(bytes, lowercase, start)
        .filter(|(start, end)| high_confidence_token(&bytes[*start..*end]))
}

fn assignment_candidate_range(
    bytes: &[u8],
    lowercase: &[u8],
    start: usize,
) -> Option<(usize, usize)> {
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
