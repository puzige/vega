use super::*;

#[test]
fn retained_cap_is_inclusive_and_one_more_fails_closed() {
    let branch = PrivateBranch {
        id: BranchId {
            generation: 0,
            slot: 0,
            seal: 0,
        },
        short: OsString::from("main"),
        full: b"refs/heads/main".to_vec(),
        oid: vec![b'0'; 40],
        current: true,
    };
    let base = retained_size(&[], &[], &[], &[], std::slice::from_ref(&branch)).expect("base");
    let exact = vec![0; BRANCH_RETAINED_LIMIT - base];
    assert!(ensure_retained_parts(&[], &[], &exact, &[], std::slice::from_ref(&branch)).is_ok());
    let plus_one = vec![0; BRANCH_RETAINED_LIMIT - base + 1];
    assert_eq!(
        error_code(ensure_retained_parts(
            &[],
            &[],
            &plus_one,
            &[],
            std::slice::from_ref(&branch)
        )),
        GitWorkspaceErrorCode::OutputTooLarge
    );
}

#[test]
fn parser_rejects_delete_duplicate_and_malformed_records() {
    assert_eq!(
        error_code(parse_target_paths(b"D\0gone\0")),
        GitWorkspaceErrorCode::MalformedOutput
    );
    assert!(
        parse_refs(
            b"0000000000000000000000000000000000000000\0refs/heads/main\0",
            b"main",
            b"0000000000000000000000000000000000000000"
        )
        .is_err()
    );
    assert_eq!(
        error_code(parse_target_paths(b"M\0same\0A\0same\0")),
        GitWorkspaceErrorCode::MalformedOutput
    );
    assert_eq!(
        error_code(parse_refs(
            b"bad\0refs/heads/main\0\n",
            b"main",
            b"0000000000000000000000000000000000000000"
        )),
        GitWorkspaceErrorCode::MalformedOutput
    );
    assert_eq!(
        error_code(parse_delete_paths(b"M\0file\0")),
        GitWorkspaceErrorCode::MalformedOutput
    );
}

#[test]
fn raw_ref_oid_status_and_line_codecs_are_strict_and_bytes_first() {
    let sha1 = b"0000000000000000000000000000000000000000";
    let sha256 = b"0000000000000000000000000000000000000000000000000000000000000000";
    let mixed = [sha256.as_slice(), b"\0refs/heads/topic\0\n"].concat();
    assert_eq!(
        error_code(parse_refs(&mixed, b"main", sha1)),
        GitWorkspaceErrorCode::MalformedOutput
    );
    let raw = b"team/non-utf8-\xff";
    assert!(validate_branch_short(raw).is_ok());
    assert_eq!(escape_ref(raw), "team/non-utf8-\\xff");
    for invalid in [
        b"-topic".as_slice(),
        b"HEAD",
        b"@",
        b"a..b",
        b"a//b",
        b"a/@{b",
        b".hidden/topic",
        b"a/.hidden",
        b"a.lock",
        b"a/b.lock",
        b"a.",
        b"a b",
        b"a~b",
        b"a^b",
        b"a:b",
        b"a?b",
        b"a*b",
        b"a[b",
        b"a\\b",
    ] {
        assert!(validate_branch_short(invalid).is_err(), "{invalid:?}");
    }
    for valid in [b"A".as_slice(), b"M", b"T", b"R0", b"R100", b"C007"] {
        assert!(parse_acmrt_status(valid).is_ok(), "{valid:?}");
    }
    for invalid in [
        b"".as_slice(),
        b"Mgarbage",
        b"D",
        b"R",
        b"R101",
        b"R1000",
        b"R-1",
        b"C1x",
    ] {
        assert!(parse_acmrt_status(invalid).is_err(), "{invalid:?}");
    }
    assert_eq!(exact_single_line(b"one\n").expect("line"), b"one");
    for invalid in [b"one".as_slice(), b"one\n\n", b"one\r\n", b"one\0\n"] {
        assert!(exact_single_line(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn path_counts_and_filter_values_are_inclusive_and_fail_closed() {
    let oid = b"0000000000000000000000000000000000000000";
    let mut refs = Vec::new();
    for index in 0..BRANCH_LIMIT {
        refs.extend_from_slice(oid);
        refs.extend_from_slice(b"\0refs/heads/b");
        refs.extend_from_slice(index.to_string().as_bytes());
        refs.extend_from_slice(b"\0\n");
    }
    let parsed_refs = parse_refs(&refs, b"b0", oid).expect("exact branch count");
    assert_eq!(parsed_refs.len(), BRANCH_LIMIT);
    refs.extend_from_slice(oid);
    refs.extend_from_slice(b"\0refs/heads/overflow\0\n");
    assert_eq!(
        error_code(parse_refs(&refs, b"b0", oid)),
        GitWorkspaceErrorCode::OutputTooLarge
    );

    let mut exact = Vec::new();
    for index in 0..PATH_LIMIT {
        exact.extend_from_slice(b"A\0");
        exact.extend_from_slice(format!("p{index}").as_bytes());
        exact.push(0);
    }
    let parsed = parse_target_paths(&exact).expect("exact path count");
    assert_eq!(parsed.materialized.len(), PATH_LIMIT);
    exact.extend_from_slice(b"A\0overflow\0");
    assert_eq!(
        error_code(parse_target_paths(&exact)),
        GitWorkspaceErrorCode::OutputTooLarge
    );

    let mut rename_exact = Vec::new();
    for index in 0..(PATH_LIMIT / 2) {
        rename_exact.extend_from_slice(b"R100\0");
        rename_exact.extend_from_slice(format!("old-{index}").as_bytes());
        rename_exact.push(0);
        rename_exact.extend_from_slice(format!("new-{index}").as_bytes());
        rename_exact.push(0);
    }
    let parsed = parse_target_paths(&rename_exact).expect("exact authority count");
    assert_eq!(parsed.authority.len(), PATH_LIMIT);
    rename_exact.extend_from_slice(b"R100\0old-overflow\0new-overflow\0");
    assert_eq!(
        error_code(parse_target_paths(&rename_exact)),
        GitWorkspaceErrorCode::OutputTooLarge
    );

    let literal = parse_target_paths(b"M\0:(glob)**\0A\0:!safe\0T\0space tab\t\xff\0")
        .expect("literal path bytes");
    assert_eq!(literal.materialized.len(), 3);

    let paths = vec![b"file.txt".to_vec()];
    for value in [b"set".as_slice(), b"unset", b"unspecified", b"demo", b""] {
        let mut output = b"file.txt\0filter\0".to_vec();
        output.extend_from_slice(value);
        output.push(0);
        assert_eq!(
            error_code(validate_branch_attrs(&paths, &output)),
            GitWorkspaceErrorCode::BranchUnsafeFilter,
            "filter value {value:?}"
        );
    }
}

#[test]
fn switch_authority_uses_one_checked_combined_budget() {
    let fixed = std::mem::size_of::<BranchSwitchPermit>() + std::mem::size_of::<SwitchAuthority>();
    let paths = vec![b"old/name".to_vec(), b"new/name".to_vec()];
    let mut budget = RetainedBudget::new(BRANCH_RETAINED_LIMIT);
    budget.charge(fixed).expect("fixed");
    budget.charge(1024).expect("acmrt raw");
    charge_paths(&mut budget, &paths).expect("materialized");
    charge_paths(&mut budget, &paths).expect("authority");
    budget.charge(2048).expect("delete raw");
    budget.charge(128).expect("stdin");
    budget
        .charge(2 * std::mem::size_of::<usize>())
        .expect("arc counters");
    budget.charge(4096).expect("attrs");
    let remaining = budget.remaining();
    budget.charge(remaining).expect("inclusive cap");
    assert_eq!(
        error_code(budget.charge(1)),
        GitWorkspaceErrorCode::OutputTooLarge
    );
}
