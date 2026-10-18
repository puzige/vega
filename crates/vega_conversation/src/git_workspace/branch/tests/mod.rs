use super::*;
use std::os::unix::fs::PermissionsExt;
mod branch_stub;
mod codec_limits;
mod lease_cleanup;
mod mutation_runner;
mod snapshot_ids;
mod state_guards;
mod switch_e2e;

fn branch_id(snapshot: &BranchSnapshot, label: &str) -> BranchId {
    snapshot
        .branches
        .iter()
        .find(|branch| branch.label == label)
        .expect("fixture branch")
        .id
}

fn error_code<T>(result: Result<T, GitWorkspaceError>) -> GitWorkspaceErrorCode {
    match result {
        Ok(_) => panic!("expected failure"),
        Err(failure) => failure.code(),
    }
}
