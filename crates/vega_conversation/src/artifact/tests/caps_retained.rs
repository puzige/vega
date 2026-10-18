use super::*;

#[test]
fn artifact_retained_caps_are_inclusive_and_plus_one_fails_closed() {
    let exact_id = "i".repeat(CALL_ID_BYTES);
    let exact_total_input = "x".repeat(PROPOSAL_RETAINED_BYTES - exact_id.len() - 5);
    let exact_proposal = ToolCall {
        id: exact_id.clone(),
        tool: "write".into(),
        input_json: exact_total_input,
    };
    assert!(ArtifactService::validate_proposal(&exact_proposal).is_ok());
    let mut plus_one_total = exact_proposal.clone();
    plus_one_total.input_json.push('x');
    assert_eq!(
        ArtifactService::validate_proposal(&plus_one_total).map_err(|failure| failure.code()),
        Err(GitWorkspaceErrorCode::ArtifactLimit)
    );
    let mut plus_one_id = exact_proposal;
    plus_one_id.id.push('i');
    plus_one_id.input_json.clear();
    assert_eq!(
        ArtifactService::validate_proposal(&plus_one_id).map_err(|failure| failure.code()),
        Err(GitWorkspaceErrorCode::ArtifactLimit)
    );

    let repo = Repo::new();
    let workspace = Arc::new(GitWorkspaceService::new(repo.path()).unwrap());
    let service =
        ArtifactService::new(workspace, PROJECT_ID.into(), THREAD_ID.into(), 909).unwrap();
    let call = write_call("cap", "artifact.txt", 1);
    let mut exact_envelope = write_result("cap", "artifact.txt", 1, false);
    exact_envelope.output = "x".repeat(TERMINAL_SUCCESS_BYTES);
    assert!(matches!(
        service
            .prepare_capture(&call, &exact_envelope)
            .map_err(|failure| failure.code()),
        Err(code) if code != GitWorkspaceErrorCode::ArtifactLimit
    ));
    exact_envelope.output.push('x');
    assert!(matches!(
        service
            .prepare_capture(&call, &exact_envelope)
            .map_err(|failure| failure.code()),
        Err(GitWorkspaceErrorCode::ArtifactLimit)
    ));

    let exact_path = "p".repeat(LOGICAL_PATH_BYTES);
    assert!(
        service
            .prepare_capture(
                &write_call("path-cap", &exact_path, 1),
                &write_result("path-cap", &exact_path, 1, false),
            )
            .is_ok()
    );
    let plus_one_path = "p".repeat(LOGICAL_PATH_BYTES + 1);
    assert!(matches!(
        service
            .prepare_capture(
                &write_call("path-over", &plus_one_path, 1),
                &write_result("path-over", &plus_one_path, 1, false),
            )
            .map_err(|failure| failure.code()),
        Err(GitWorkspaceErrorCode::ArtifactLimit)
    ));

    let fixed = std::mem::size_of::<ArtifactCaptureCandidate>() + 1 + 64;
    let exact_candidate = ArtifactCaptureCandidate {
        call_id: "c".into(),
        fingerprint: TerminalFingerprint::Write {
            path: "p".repeat(CAPTURE_CANDIDATE_RETAINED_BYTES - fixed),
            input_fingerprint: "a".repeat(64),
            bytes_written: 1,
        },
    };
    assert!(validate_candidate_retained(&exact_candidate).is_ok());
    let mut plus_one_candidate = exact_candidate;
    if let TerminalFingerprint::Write { path, .. } = &mut plus_one_candidate.fingerprint {
        path.push('p');
    }
    assert_eq!(
        validate_candidate_retained(&plus_one_candidate).map_err(|failure| failure.code()),
        Err(GitWorkspaceErrorCode::ArtifactLimit)
    );
}

#[test]
fn text_preview_allowlist_is_exact_and_case_insensitive_only_for_extensions() {
    for extension in [
        "txt", "md", "markdown", "rst", "adoc", "csv", "tsv", "json", "jsonl", "yaml", "yml",
        "toml", "xml", "html", "htm", "css", "scss", "sass", "less", "js", "jsx", "mjs", "cjs",
        "ts", "tsx", "rs", "py", "rb", "go", "java", "kt", "kts", "swift", "c", "h", "cc", "cpp",
        "cxx", "hpp", "hxx", "m", "mm", "sh", "bash", "zsh", "fish", "sql", "graphql", "gql",
        "proto", "diff", "patch", "log",
    ] {
        let accepted = format!("nested/file.{}", extension.to_ascii_uppercase());
        assert!(
            text_preview_path_allowed(OsStr::new(&accepted)),
            "{accepted}"
        );
    }
    for basename in [
        "README",
        "LICENSE",
        "NOTICE",
        "CHANGELOG",
        "Makefile",
        "Dockerfile",
        ".gitignore",
        ".gitattributes",
        ".editorconfig",
    ] {
        assert!(
            text_preview_path_allowed(OsStr::new(basename)),
            "{basename}"
        );
    }
    for rejected in [
        ".env",
        ".npmrc",
        "README.md.bak",
        "readme",
        "image.svg",
        "image.png",
        "unknown.bin",
    ] {
        assert!(
            !text_preview_path_allowed(OsStr::new(rejected)),
            "{rejected}"
        );
    }
}

#[test]
fn preview_line_caps_are_inclusive() {
    let exact_lines = "x\n".repeat(PREVIEW_LINES);
    assert!(validate_preview_lines(&exact_lines).is_ok());
    let too_many = format!("{exact_lines}x");
    assert_eq!(
        validate_preview_lines(&too_many).map_err(|failure| failure.code()),
        Err(GitWorkspaceErrorCode::OutputTooLarge)
    );
    assert!(validate_preview_lines(&"x".repeat(PREVIEW_LINE_BYTES)).is_ok());
    assert_eq!(
        validate_preview_lines(&"x".repeat(PREVIEW_LINE_BYTES + 1))
            .map_err(|failure| failure.code()),
        Err(GitWorkspaceErrorCode::OutputTooLarge)
    );
}

#[tokio::test]
async fn artifact_route_card_limit_is_inclusive() {
    let repo = Repo::new();
    let workspace = refreshed_workspace(&repo).await;
    let service =
        ArtifactService::new(workspace, PROJECT_ID.to_owned(), THREAD_ID.to_owned(), 11).unwrap();
    for slot in 0..ROUTE_CARD_LIMIT {
        let call_id = format!("missing-{slot}");
        let card = service
            .capture(
                &write_call(&call_id, "missing.txt", 1),
                &write_result(&call_id, "missing.txt", 1, false),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(card.source, ArtifactSource::WorkspaceChange);
        assert!(card.current_file_id.is_none());
    }
    assert_eq!(service.cards().len(), ROUTE_CARD_LIMIT);
    let failure = service
        .capture(
            &write_call("missing-overflow", "missing.txt", 1),
            &write_result("missing-overflow", "missing.txt", 1, false),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code(), GitWorkspaceErrorCode::ArtifactLimit);
    assert_eq!(service.cards().len(), ROUTE_CARD_LIMIT);
}
