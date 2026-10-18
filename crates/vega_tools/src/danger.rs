//! Centralized UX-only dangerous-command matcher.
//!
//! These rules force permission confirmation. They are intentionally not a
//! security boundary; every bash execution still passes through Seatbelt.

use std::sync::OnceLock;

use regex::Regex;

/// A stable dangerous-command match consumed by the permission engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DangerMatch {
    /// Stable identifier persisted in permission audit data.
    pub rule_id: &'static str,
    /// Stable user-facing reason.
    pub reason: &'static str,
}

/// Fail-closed matcher initialization failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("danger matcher unavailable")]
pub struct DangerMatcherError;

struct Rule {
    id: &'static str,
    reason: &'static str,
    regex: Regex,
}

const RULE_SPECS: [(&str, &str, &str); 4] = [
    (
        "git-force-push",
        "forced git push can rewrite remote history",
        r"(?m)(?:^|[;&|][ \t]*)(?:/[^\s;&|]+/)?git[ \t]+push(?:[ \t]+[^\s;&|]+)*[ \t]+(?:-f|--force(?:-with-lease)?)(?:=[^\s;&|]+)?(?:[ \t]|$|[;&|])",
    ),
    (
        "raw-device-write",
        "raw device writes can destroy disks",
        r"(?m)(?:^|[;&|][ \t]*)(?:/[^\s;&|]+/)?dd(?:[ \t]+[^\s;&|]+)*[ \t]+of=/dev/[^\s;&|]+",
    ),
    (
        "filesystem-format",
        "filesystem formatting destroys existing data",
        r"(?m)(?:^|[;&|][ \t]*)(?:/[^\s;&|]+/)?mkfs[^\s;&|]*(?:[ \t]|$|[;&|])",
    ),
    (
        "diskutil-destructive",
        "diskutil erase and partition operations destroy disk data",
        r"(?m)(?:^|[;&|][ \t]*)(?:/usr/sbin/)?diskutil[ \t]+(?:eraseDisk|partitionDisk|secureErase)(?:[ \t]|$|[;&|])",
    ),
];

static RULES: OnceLock<Result<Vec<Rule>, DangerMatcherError>> = OnceLock::new();

/// Return the first stable danger match, if any.
pub fn detect_danger(command: &str) -> Result<Option<DangerMatch>, DangerMatcherError> {
    if root_recursive_delete(command) {
        return Ok(Some(DangerMatch {
            rule_id: "rm-root-recursive-force",
            reason: "recursive forced deletion of the filesystem root is destructive",
        }));
    }

    let rules = RULES.get_or_init(|| {
        RULE_SPECS
            .iter()
            .map(|(id, reason, pattern)| {
                Regex::new(pattern)
                    .map(|regex| Rule { id, reason, regex })
                    .map_err(|_| DangerMatcherError)
            })
            .collect()
    });
    let rules = rules.as_ref().map_err(|_| DangerMatcherError)?;
    Ok(rules.iter().find_map(|rule| {
        rule.regex.is_match(command).then_some(DangerMatch {
            rule_id: rule.id,
            reason: rule.reason,
        })
    }))
}

fn root_recursive_delete(command: &str) -> bool {
    for segment in command.split([';', '|', '&', '\n']) {
        let mut tokens = segment.split_whitespace();
        let Some(program) = tokens.next() else {
            continue;
        };
        if program.rsplit('/').next() != Some("rm") {
            continue;
        }

        let mut recursive = false;
        let mut force = false;
        let mut root = false;
        for token in tokens {
            if token == "/" {
                root = true;
                continue;
            }
            if token == "--recursive" {
                recursive = true;
                continue;
            }
            if token == "--force" {
                force = true;
                continue;
            }
            if let Some(short) = token
                .strip_prefix('-')
                .filter(|value| !value.starts_with('-'))
            {
                recursive |= short.chars().any(|value| matches!(value, 'r' | 'R'));
                force |= short.chars().any(|value| value == 'f');
            }
        }
        if recursive && force && root {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::detect_danger;

    fn rule(command: &str) -> Option<&'static str> {
        detect_danger(command)
            .unwrap()
            .map(|matched| matched.rule_id)
    }

    #[test]
    fn danger_rm_root_variants_and_negatives() {
        for command in [
            "rm -rf /",
            "rm -fr /",
            "rm -r -f /",
            "rm -f -R /",
            "rm --recursive --force /",
            "rm --force --recursive /",
            "\trm   -r   -f   /  ",
            "/bin/rm -Rf /",
        ] {
            assert_eq!(rule(command), Some("rm-root-recursive-force"), "{command}");
        }
        for command in [
            "rm -rf ./",
            "rm -r /",
            "rm -f /",
            "echo rm -rf /",
            "rmdir /",
        ] {
            assert_eq!(rule(command), None, "{command}");
        }
    }

    #[test]
    fn danger_git_force_push_variants_and_negatives() {
        for command in [
            "git push -f",
            "git push --force origin main",
            "git push origin main --force-with-lease",
            "git push --force-with-lease=refs/heads/main",
            "/usr/bin/git push --force origin main",
            "echo ok; git push -f origin main",
        ] {
            assert_eq!(rule(command), Some("git-force-push"), "{command}");
        }
        for command in [
            "git push",
            "git push origin main",
            "git fetch --force",
            "echo git push -f",
        ] {
            assert_eq!(rule(command), None, "{command}");
        }
    }

    #[test]
    fn danger_device_format_and_diskutil_rules_have_negatives() {
        for (command, expected) in [
            ("dd if=image of=/dev/disk4", "raw-device-write"),
            ("dd bs=1m of=/dev/rdisk2 if=image", "raw-device-write"),
            ("/bin/dd if=image of=/dev/disk4", "raw-device-write"),
            ("mkfs /dev/disk4", "filesystem-format"),
            ("/sbin/mkfs.ext4 /dev/disk4", "filesystem-format"),
            ("mkfs_apfs /dev/disk4", "filesystem-format"),
            ("/sbin/mkfs-ext4 /dev/disk4", "filesystem-format"),
            ("mkfs-not-a-command file", "filesystem-format"),
            (
                "diskutil eraseDisk APFS Empty /dev/disk4",
                "diskutil-destructive",
            ),
            (
                "diskutil partitionDisk /dev/disk4 1 GPT APFS X R",
                "diskutil-destructive",
            ),
            (
                "/usr/sbin/diskutil secureErase 0 /dev/disk4",
                "diskutil-destructive",
            ),
        ] {
            assert_eq!(rule(command), Some(expected), "{command}");
        }
        for command in [
            "dd if=/dev/zero of=./image",
            "echo dd of=/dev/disk4",
            "diskutil list",
            "diskutil info /dev/disk4",
        ] {
            assert_eq!(rule(command), None, "{command}");
        }
    }
}
