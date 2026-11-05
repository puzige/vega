use super::*;
use crate::skills::SkillSource;

fn parse(fields: &str) -> Result<SkillDocument, SkillError> {
    let text =
        format!("---\nname: example-skill\ndescription: Inspect files.\n{fields}---\nBody\n");
    parse_skill_md(text.as_bytes(), "example-skill")
}

#[test]
fn string_and_list_declarations_preserve_order_and_body() {
    let string = parse("allowed-tools: 'Read Bash(git:*) Grep'\n").unwrap();
    let list = parse("allowed-tools: [Read, 'Bash(git:*)', Grep]\n").unwrap();
    assert_eq!(string, list);
    assert_eq!(
        list.metadata.allowed_tools.as_deref(),
        Some("Read Bash(git:*) Grep")
    );
    assert_eq!(list.body, "Body\n");
    assert_eq!(
        parse("allowed-tools: '  Read  '\n")
            .unwrap()
            .metadata
            .allowed_tools
            .as_deref(),
        Some("  Read  ")
    );
}

#[test]
fn absent_null_and_empty_declarations_keep_optional_semantics() {
    for input in ["", "allowed-tools: null\n"] {
        assert_eq!(parse(input).unwrap().metadata.allowed_tools, None);
    }
    for input in ["allowed-tools: ''\n", "allowed-tools: []\n"] {
        assert_eq!(
            parse(input).unwrap().metadata.allowed_tools.as_deref(),
            Some("")
        );
    }
    assert_eq!(
        parse("allowed-tools: ['', ' ', Read, '']\n")
            .unwrap()
            .metadata
            .allowed_tools
            .as_deref(),
        Some("   Read ")
    );
}

#[test]
fn non_string_tool_declarations_are_rejected_without_coercion() {
    for value in [
        "{}",
        "{read: true}",
        "[[Read]]",
        "[Read, 7]",
        "[true]",
        "[null]",
        "false",
        "42",
        "[Read, .NaN, +.Inf]",
        "-.Inf",
        "!!int 7",
        "[!!bool true]",
    ] {
        let result = parse(&format!("allowed-tools: {value}\n"));
        assert_eq!(result, Err(SkillError::InvalidAllowedTools), "{value}");
    }
    assert_eq!(
        SkillError::InvalidAllowedTools.code(),
        "invalid_allowed_tools"
    );
}

#[test]
fn tagged_and_aliased_types_keep_field_context() {
    for (field, expected) in [
        ("license", SkillError::InvalidLicense),
        ("compatibility", SkillError::InvalidCompatibility),
        ("metadata", SkillError::InvalidMetadata),
        ("allowed-tools", SkillError::InvalidAllowedTools),
    ] {
        for value in ["!!int 7", "!!float .Inf", "!!bool true"] {
            assert_eq!(parse(&format!("{field}: {value}\n")), Err(expected));
            assert_eq!(
                parse(&format!("extra: &value {value}\n{field}: *value\n")),
                Err(expected)
            );
        }
    }
    assert_eq!(
        parse("extra: &value [.NaN]\nallowed-tools: *value\n"),
        Err(SkillError::InvalidAllowedTools)
    );
    assert_eq!(
        parse("extra: &value .NaN\nallowed-tools: [*value]\n"),
        Err(SkillError::InvalidAllowedTools)
    );
    let valid = parse("allowed-tools: [!!str .NaN, '.Inf', '7', 'true']\n").unwrap();
    assert_eq!(
        valid.metadata.allowed_tools.as_deref(),
        Some(".NaN .Inf 7 true")
    );
    assert_eq!(
        parse("extra: &value !!str .NaN\nallowed-tools: [*value]\n")
            .unwrap()
            .metadata
            .allowed_tools
            .as_deref(),
        Some(".NaN")
    );
}

#[test]
fn known_field_errors_identify_the_responsible_field() {
    for (yaml, expected) in [
        ("description: ok\n", SkillError::InvalidNameField),
        (
            "name: [example-skill]\ndescription: ok\n",
            SkillError::InvalidNameField,
        ),
        ("name: example-skill\n", SkillError::InvalidDescription),
        (
            "name: example-skill\ndescription: true\n",
            SkillError::InvalidDescription,
        ),
        (
            "name: example-skill\ndescription: ''\n",
            SkillError::InvalidDescription,
        ),
        ("name: other\ndescription: ok\n", SkillError::InvalidName),
    ] {
        let text = format!("---\n{yaml}---\nBody");
        assert_eq!(
            parse_skill_md(text.as_bytes(), "example-skill"),
            Err(expected)
        );
    }
    for (field, expected) in [
        ("license", SkillError::InvalidLicense),
        ("compatibility", SkillError::InvalidCompatibility),
    ] {
        for value in ["[]", "{}", "false", "7"] {
            assert_eq!(parse(&format!("{field}: {value}\n")), Err(expected));
        }
    }
    assert_eq!(
        parse("compatibility: ''\n"),
        Err(SkillError::InvalidCompatibility)
    );
    assert_eq!(
        parse(&format!("compatibility: '{}'\n", "x".repeat(501))),
        Err(SkillError::InvalidCompatibility)
    );
    let text = format!(
        "---\nname: example-skill\ndescription: '{}'\n---\n",
        "x".repeat(1025)
    );
    assert_eq!(
        parse_skill_md(text.as_bytes(), "example-skill"),
        Err(SkillError::InvalidDescription)
    );
    assert_eq!(
        parse("license: null\ncompatibility: null\n")
            .unwrap()
            .metadata
            .license,
        None
    );
}

#[test]
fn metadata_remains_a_string_map_and_unknown_fields_remain_inert() {
    assert!(parse("").unwrap().metadata.metadata.is_empty());
    let document =
        parse("metadata: {author: example, version: '1.0'}\ntriggers: [user, model]\n").unwrap();
    assert_eq!(
        document
            .metadata
            .metadata
            .get("version")
            .map(String::as_str),
        Some("1.0")
    );
    for value in [
        "null",
        "[]",
        "{nested: {x: y}}",
        "{items: [a]}",
        "{flag: true}",
        "{version: 1}",
    ] {
        assert_eq!(
            parse(&format!("metadata: {value}\n")),
            Err(SkillError::InvalidMetadata)
        );
    }
}

#[test]
fn yaml_safety_limits_still_reject_invalid_documents() {
    assert_eq!(
        parse("allowed-tools: [Read\n"),
        Err(SkillError::MalformedYaml)
    );
    assert_eq!(
        parse("allowed-tools: !include secret\n"),
        Err(SkillError::ForbiddenTag)
    );
    for input in [
        "allowed-tools: Read\nallowed-tools: Grep\n",
        "metadata: {author: first, author: second}\n",
        "metadata: {<<: {author: example}}\n",
    ] {
        assert!(parse(input).is_err(), "{input}");
    }
    assert_eq!(
        parse(&format!("extra: '{}'\n", "x".repeat(MAX_FRONTMATTER_BYTES))),
        Err(SkillError::TooLarge)
    );
    let nested = format!("extra: {}0{}\n", "[".repeat(30), "]".repeat(30));
    assert!(parse(&nested).is_err());
    let aliases = format!("a: &a Read\nextra: [{}]\n", ["*a"; 17].join(","));
    assert_eq!(parse(&aliases), Err(SkillError::TooLarge));
}

#[test]
fn discovery_accepts_both_formats_and_emits_safe_field_codes() {
    let root =
        std::env::temp_dir().join(format!("vega-skill-declaration-{}", ulid::Ulid::generate()));
    std::fs::create_dir(&root).unwrap();
    for (name, fields) in [
        ("string-skill", "allowed-tools: Read\n"),
        ("list-skill", "allowed-tools: [Read, Grep]\n"),
        ("broken-skill", "allowed-tools: {private-value: secret}\n"),
    ] {
        let directory = root.join(name);
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Inspect files.\n{fields}---\nBody"),
        )
        .unwrap();
    }
    let source = SkillSource::imported_approved(&root, 0).unwrap();
    let discovery = source.discover().unwrap();
    assert_eq!(discovery.candidates.len(), 2);
    assert_eq!(discovery.diagnostics.len(), 1);
    assert_eq!(discovery.diagnostics[0].name, "broken-skill");
    assert_eq!(
        discovery.diagnostics[0].error.code(),
        "invalid_allowed_tools"
    );
    for candidate in &discovery.candidates {
        let bytes = source.load_candidate(candidate).unwrap();
        assert_eq!(
            parse_skill_md(&bytes, &candidate.name).unwrap().body,
            "Body"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
