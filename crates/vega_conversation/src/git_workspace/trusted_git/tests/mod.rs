use super::*;
use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

#[path = "codec_topology.rs"]
mod codec_topology;
mod command_stub;
#[path = "commit_proof.rs"]
mod commit_proof;
#[path = "filter_gitlink.rs"]
mod filter_gitlink;
#[path = "runner_mutation.rs"]
mod runner_mutation;
#[path = "selection_noop.rs"]
mod selection_noop;
#[path = "selection_topology.rs"]
mod selection_topology;
#[path = "summary_draft.rs"]
mod summary_draft;

fn test_head(unborn: bool, width: usize) -> HeadAuthority {
    HeadAuthority {
        unborn,
        oid: vec![if unborn { b'0' } else { b'a' }; width],
        short: b"master".to_vec(),
        full_ref: b"refs/heads/master".to_vec(),
    }
}

fn status_prefix(head: &HeadAuthority) -> Vec<u8> {
    let mut bytes = b"# branch.oid ".to_vec();
    if head.unborn {
        bytes.extend_from_slice(b"(initial)");
    } else {
        bytes.extend_from_slice(&head.oid);
    }
    bytes.extend_from_slice(b"\0# branch.head ");
    bytes.extend_from_slice(&head.short);
    bytes.push(0);
    bytes
}

fn stage_record(mode: &[u8], oid: &[u8], path: &[u8]) -> Vec<u8> {
    let mut bytes = mode.to_vec();
    bytes.push(b' ');
    bytes.extend_from_slice(oid);
    bytes.extend_from_slice(b" 0\t");
    bytes.extend_from_slice(path);
    bytes.push(0);
    bytes
}

fn tree_record(mode: &[u8], object_type: &[u8], oid: &[u8], path: &[u8]) -> Vec<u8> {
    let mut bytes = mode.to_vec();
    bytes.push(b' ');
    bytes.extend_from_slice(object_type);
    bytes.push(b' ');
    bytes.extend_from_slice(oid);
    bytes.push(b'\t');
    bytes.extend_from_slice(path);
    bytes.push(0);
    bytes
}

fn status_rc_record(
    kind: u8,
    head_oid: &[u8],
    index_oid: &[u8],
    current: &[u8],
    previous: &[u8],
) -> Vec<u8> {
    let mut bytes = b"2 ".to_vec();
    bytes.push(kind);
    bytes.extend_from_slice(b". N... 100644 100644 100644 ");
    bytes.extend_from_slice(head_oid);
    bytes.push(b' ');
    bytes.extend_from_slice(index_oid);
    bytes.push(b' ');
    bytes.push(kind);
    bytes.extend_from_slice(b"100 ");
    bytes.extend_from_slice(current);
    bytes.push(0);
    bytes.extend_from_slice(previous);
    bytes.push(0);
    bytes
}

fn expected_mutation_argv(verb: &[u8], tail: &[&[u8]]) -> Vec<u8> {
    let mut expected = Vec::new();
    for argument in PREFIX
        .iter()
        .map(|value| value.as_bytes())
        .chain([b"-c".as_slice(), b"core.hooksPath=/dev/null", verb])
        .chain(tail.iter().copied())
    {
        expected.extend_from_slice(argument);
        expected.push(0);
    }
    expected
}

fn assert_terminal_workspace(trusted: &TrustedGitService, terminal: &WorkspaceSnapshot) {
    let workspace = trusted
        .workspace
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert_eq!(workspace.snapshot.as_ref(), Some(terminal));
    assert!(workspace.active_mutation_owner.is_none());
    drop(workspace);
    let state = trusted
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    assert!(!state.mutation_active);
}
