use super::*;

#[test]
fn git_workspace_hunk_suffix_no_newline_and_line_cap_are_preserved() {
    let exact_line = "x".repeat(PATCH_LINE_LIMIT);
    let patch = format!("@@ -0,0 +1,1 @@ fn name\n+{exact_line}\n\\ No newline at end of file\n");
    let mut rows = PATCH_ROW_LIMIT;
    let section = parse_patch(DiffLayer::Unstaged, patch.as_bytes(), &mut rows).unwrap();
    let hunk = &section.hunks[0];
    assert_eq!(hunk.heading_suffix.as_deref(), Some("fn name"));
    assert!(hunk.missing_trailing_newline);
    assert_eq!(hunk.rows[0].text.len(), PATCH_LINE_LIMIT);
    let too_long = format!("@@ -0,0 +1,1 @@\n+{}\n", "x".repeat(PATCH_LINE_LIMIT + 1));
    let error = match parse_patch(DiffLayer::Unstaged, too_long.as_bytes(), &mut rows) {
        Ok(_) => panic!("oversized line was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::OutputTooLarge);
    let mut rows = PATCH_ROW_LIMIT;
    let bad_marker = b"@@ -1,1 +1,1 @@\n same\n\\ unexpected marker\n";
    let error = match parse_patch(DiffLayer::Unstaged, bad_marker, &mut rows) {
        Ok(_) => panic!("unknown backslash marker was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
    let mut rows = PATCH_ROW_LIMIT;
    let overflow = b"@@ -4294967295,1 +1,1 @@\n same\n";
    let error = match parse_patch(DiffLayer::Unstaged, overflow, &mut rows) {
        Ok(_) => panic!("overflowing line coordinate was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code(), GitWorkspaceErrorCode::MalformedOutput);
}

#[test]
fn git_workspace_combined_patch_byte_and_row_caps_are_inclusive() {
    let mut bytes = PATCH_LIMIT;
    consume_projection_bytes(&mut bytes, PATCH_LIMIT / 2).unwrap();
    consume_projection_bytes(&mut bytes, PATCH_LIMIT - PATCH_LIMIT / 2).unwrap();
    assert_eq!(bytes, 0);
    assert_eq!(
        consume_projection_bytes(&mut bytes, 1).unwrap_err().code(),
        GitWorkspaceErrorCode::OutputTooLarge
    );

    let patch = |rows: usize| {
        let mut body = format!("@@ -0,0 +1,{rows} @@\n");
        for _ in 0..rows {
            body.push_str("+x\n");
        }
        body
    };
    let mut rows = PATCH_ROW_LIMIT;
    let staged = parse_patch(
        DiffLayer::Staged,
        patch(PATCH_ROW_LIMIT / 2).as_bytes(),
        &mut rows,
    )
    .unwrap();
    let unstaged = parse_patch(
        DiffLayer::Unstaged,
        patch(PATCH_ROW_LIMIT / 2).as_bytes(),
        &mut rows,
    )
    .unwrap();
    assert_eq!(rows, 0);
    assert_eq!(staged.hunks[0].rows.len(), PATCH_ROW_LIMIT / 2);
    assert_eq!(unstaged.hunks[0].rows.len(), PATCH_ROW_LIMIT / 2);
    let mut rows = PATCH_ROW_LIMIT;
    parse_patch(
        DiffLayer::Staged,
        patch(PATCH_ROW_LIMIT / 2).as_bytes(),
        &mut rows,
    )
    .unwrap();
    let row_error = match parse_patch(
        DiffLayer::Unstaged,
        patch(PATCH_ROW_LIMIT / 2 + 1).as_bytes(),
        &mut rows,
    ) {
        Ok(_) => panic!("combined row cap +1 was accepted"),
        Err(error) => error,
    };
    assert_eq!(row_error.code(), GitWorkspaceErrorCode::OutputTooLarge);
}

#[test]
fn git_workspace_environment_scrub_is_exact() {
    let mut command = Command::new("/usr/bin/true");
    command
        .env("GIT_DIR", "/private/leak")
        .env("GIT_CONFIG_COUNT", "9")
        .env("DEVELOPER_DIR", "/private/leak-xcode")
        .env("TOOLCHAINS", "private-toolchain")
        .env("DYLD_INSERT_LIBRARIES", "/private/leak.dylib")
        .env("LD_PRELOAD", "/private/leak.so")
        .env("VEGA_KEEP", "yes");
    scrub_git_environment(&mut command);
    let env: HashMap<_, _> = command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
        .collect();
    assert_eq!(env.get(OsStr::new("GIT_DIR")), Some(&None));
    assert_eq!(env.get(OsStr::new("GIT_CONFIG_COUNT")), Some(&None));
    for key in [
        "DEVELOPER_DIR",
        "TOOLCHAINS",
        "DYLD_INSERT_LIBRARIES",
        "LD_PRELOAD",
    ] {
        assert_eq!(
            env.get(OsStr::new(key)),
            Some(&None),
            "{key} survived scrub"
        );
    }
    assert_eq!(
        env.get(OsStr::new("GIT_LITERAL_PATHSPECS"))
            .and_then(|value| value.as_deref()),
        Some(OsStr::new("1"))
    );
    assert_eq!(
        env.get(OsStr::new("GIT_NO_LAZY_FETCH"))
            .and_then(|value| value.as_deref()),
        Some(OsStr::new("1"))
    );
    assert_eq!(
        env.get(OsStr::new("VEGA_KEEP"))
            .and_then(|value| value.as_deref()),
        Some(OsStr::new("yes"))
    );
}

#[tokio::test]
async fn git_workspace_explicit_filter_attribute_rejects_before_driver_execution() {
    let fixture = super::lifecycle_stub::LifecycleFixture::new("owner-terminal");
    fixture.set_attrs("tracked.txt\0filter\0evil\0");
    let service = fixture.service();
    assert_eq!(
        service
            .refresh(CancellationToken::new())
            .await
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::GitFailed
    );
    fixture.assert_clean();
    assert_eq!(
        validate_filter_attrs(&[b"victim.txt".to_vec()], b"victim.txt\0filter\0unset\0")
            .unwrap_err()
            .code(),
        GitWorkspaceErrorCode::GitFailed
    );
}

#[test]
fn summary_reader_chunk_partition_is_irrelevant() {
    struct Chunked {
        bytes: Vec<u8>,
        offset: usize,
        chunk: usize,
    }
    impl Read for Chunked {
        fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
            let available = self.bytes.len().saturating_sub(self.offset);
            let read = available.min(self.chunk).min(target.len());
            target[..read].copy_from_slice(&self.bytes[self.offset..self.offset + read]);
            self.offset += read;
            Ok(read)
        }
    }
    let bytes: Vec<u8> = (0..65_567).map(|index| (index % 251) as u8).collect();
    for chunk in [1, 4 * 1024, IO_CHUNK] {
        let (sender, receiver) = mpsc::channel();
        spawn_reader(
            Chunked {
                bytes: bytes.clone(),
                offset: 0,
                chunk,
            },
            Stream::Stdout,
            65_536,
            Arc::new(AtomicBool::new(false)),
            false,
            sender,
        );
        let output = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(output.bytes, bytes[..65_536]);
        assert!(output.overflow);
        assert!(!output.failed);
    }
}

#[test]
fn summary_reader_first_read_crosses_cap_and_still_drains_tail() {
    struct OneRead {
        bytes: Vec<u8>,
        consumed: Arc<AtomicUsize>,
    }
    impl Read for OneRead {
        fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
            if self.bytes.is_empty() {
                return Ok(0);
            }
            let read = self.bytes.len().min(target.len());
            target[..read].copy_from_slice(&self.bytes[..read]);
            self.bytes.drain(..read);
            self.consumed.fetch_add(read, Ordering::SeqCst);
            Ok(read)
        }
    }
    const CAP: usize = 37;
    let bytes: Vec<u8> = (0..(CAP + 211)).map(|index| (index % 251) as u8).collect();
    assert!(bytes.len() < IO_CHUNK, "fixture must cross cap in one read");
    let consumed = Arc::new(AtomicUsize::new(0));
    let (sender, receiver) = mpsc::channel();
    spawn_reader(
        OneRead {
            bytes: bytes.clone(),
            consumed: consumed.clone(),
        },
        Stream::Stdout,
        CAP,
        Arc::new(AtomicBool::new(false)),
        false,
        sender,
    );
    let output = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(output.bytes, bytes[..CAP]);
    assert!(output.overflow);
    assert_eq!(consumed.load(Ordering::SeqCst), bytes.len());
}
