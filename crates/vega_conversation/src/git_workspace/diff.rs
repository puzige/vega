use super::*;

pub(crate) fn parse_patch(
    layer: DiffLayer,
    bytes: &[u8],
    remaining_rows: &mut usize,
) -> Result<DiffSection, GitWorkspaceError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?;
    if text.contains("Binary files ") || text.contains("GIT binary patch") {
        return Err(error(GitWorkspaceErrorCode::MetadataOnly));
    }
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0_u32;
    let mut new_line = 0_u32;
    for line in logical_lines(text) {
        if line.starts_with("@@ ") {
            if line.len() > PATCH_LINE_LIMIT {
                return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
            }
            if let Some(hunk) = current.take() {
                validate_hunk(&hunk)?;
                hunks.push(hunk);
            }
            let (old_start, old_count, new_start, new_count, heading_suffix) =
                parse_hunk_header(line)?;
            old_line = old_start;
            new_line = new_start;
            current = Some(DiffHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                heading_suffix,
                missing_trailing_newline: false,
                rows: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = current.as_mut() else {
            continue;
        };
        let Some((&prefix, body)) = line.as_bytes().split_first() else {
            return Err(error(GitWorkspaceErrorCode::MalformedOutput));
        };
        if body.len() > PATCH_LINE_LIMIT {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        if prefix == b'\\' {
            if line != "\\ No newline at end of file" {
                return Err(error(GitWorkspaceErrorCode::MalformedOutput));
            }
            hunk.missing_trailing_newline = true;
            continue;
        }
        if *remaining_rows == 0 {
            return Err(error(GitWorkspaceErrorCode::OutputTooLarge));
        }
        *remaining_rows -= 1;
        let body = std::str::from_utf8(body)
            .map_err(|_| error(GitWorkspaceErrorCode::MetadataOnly))?
            .to_owned();
        let row = match prefix {
            b' ' => {
                let row = DiffRow {
                    kind: DiffRowKind::Context,
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                    text: body,
                };
                old_line = old_line
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                new_line = new_line
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                row
            }
            b'-' => {
                let row = DiffRow {
                    kind: DiffRowKind::Deletion,
                    old_line: Some(old_line),
                    new_line: None,
                    text: body,
                };
                old_line = old_line
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                row
            }
            b'+' => {
                let row = DiffRow {
                    kind: DiffRowKind::Addition,
                    old_line: None,
                    new_line: Some(new_line),
                    text: body,
                };
                new_line = new_line
                    .checked_add(1)
                    .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
                row
            }
            _ => return Err(error(GitWorkspaceErrorCode::MalformedOutput)),
        };
        hunk.rows.push(row);
    }
    if let Some(hunk) = current {
        validate_hunk(&hunk)?;
        hunks.push(hunk);
    }
    Ok(DiffSection { layer, hunks })
}

pub(crate) fn parse_hunk_header(
    line: &str,
) -> Result<(u32, u32, u32, u32, Option<String>), GitWorkspaceError> {
    let end = line[3..]
        .find(" @@")
        .map(|index| index + 3)
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let mut ranges = line[3..end].split(' ');
    let old = ranges
        .next()
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let new = ranges
        .next()
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    if ranges.next().is_some() {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    let (old_start, old_count) = parse_range(old, '-')?;
    let (new_start, new_count) = parse_range(new, '+')?;
    let suffix = line[end + 3..]
        .strip_prefix(' ')
        .unwrap_or(&line[end + 3..]);
    let suffix = (!suffix.is_empty()).then(|| suffix.to_owned());
    Ok((old_start, old_count, new_start, new_count, suffix))
}

pub(crate) fn parse_range(value: &str, prefix: char) -> Result<(u32, u32), GitWorkspaceError> {
    let value = value
        .strip_prefix(prefix)
        .ok_or_else(|| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    let start = start
        .parse()
        .map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    let count = count
        .parse()
        .map_err(|_| error(GitWorkspaceErrorCode::MalformedOutput))?;
    Ok((start, count))
}

pub(crate) fn validate_hunk(hunk: &DiffHunk) -> Result<(), GitWorkspaceError> {
    let old = hunk
        .rows
        .iter()
        .filter(|row| row.kind != DiffRowKind::Addition)
        .count();
    let new = hunk
        .rows
        .iter()
        .filter(|row| row.kind != DiffRowKind::Deletion)
        .count();
    if old != hunk.old_count as usize || new != hunk.new_count as usize {
        return Err(error(GitWorkspaceErrorCode::MalformedOutput));
    }
    Ok(())
}

pub(crate) fn logical_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_terminator('\n')
}

pub(crate) fn language_for(path: &[u8]) -> DiffLanguage {
    let name = path.rsplit(|byte| *byte == b'/').next().unwrap_or(path);
    let extension = name
        .iter()
        .rposition(|byte| *byte == b'.')
        .map(|position| &name[position + 1..]);
    match extension {
        Some(b"rs") => DiffLanguage::Rust,
        Some(b"ts") => DiffLanguage::TypeScript,
        Some(b"tsx") => DiffLanguage::Tsx,
        Some(b"js" | b"jsx" | b"mjs" | b"cjs") => DiffLanguage::JavaScript,
        Some(b"py") => DiffLanguage::Python,
        _ => DiffLanguage::Plain,
    }
}

pub(crate) fn escape_path(path: &[u8]) -> String {
    escape_bytes(path)
}

pub(crate) fn escape_ref(reference: &[u8]) -> String {
    escape_bytes(reference)
}

pub(crate) fn escape_bytes(bytes: &[u8]) -> String {
    if let Ok(value) = std::str::from_utf8(bytes) {
        let mut escaped = String::new();
        for character in value.chars() {
            if character.is_control() || is_bidi_control(character) {
                for byte in character.to_string().bytes() {
                    escaped.push_str(&format!("\\x{byte:02x}"));
                }
            } else if character == '\\' {
                escaped.push_str("\\\\");
            } else {
                escaped.push(character);
            }
        }
        return escaped;
    }
    let mut escaped = String::new();
    for byte in bytes {
        if (0x20..=0x7e).contains(byte) && *byte != b'\\' {
            escaped.push(char::from(*byte));
        } else if *byte == b'\\' {
            escaped.push_str("\\\\");
        } else {
            escaped.push_str(&format!("\\x{byte:02x}"));
        }
    }
    escaped
}

pub(crate) fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

pub(crate) fn seal(
    identity: RootIdentity,
    instance_nonce: u64,
    generation: u64,
    slot: u32,
    path: &[u8],
) -> u64 {
    let mut value = identity.dev
        ^ identity.ino.rotate_left(17)
        ^ instance_nonce.rotate_left(23)
        ^ generation.rotate_left(31);
    value ^= u64::from(slot).rotate_left(7);
    for byte in path {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    value
}

pub(crate) fn trim_one_newline(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}
