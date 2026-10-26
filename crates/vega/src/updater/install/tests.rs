use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use super::{
    CandidateStatus, Manifest, exclude_legacy_rollback, finish_startup, retained_backup,
    rollback_with_candidate_verifier,
};
use crate::updater::platform;

struct Fixture {
    _root: tempfile::TempDir,
    staging: PathBuf,
    manifest: Manifest,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("isolated update fixture");
        let staging = root.path().join(".vega-update-fixture");
        let target = root.path().join("Vega.app");
        let previous = staging.join("previous.app");
        for bundle in [&target, &previous] {
            std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("fixture bundle");
        }
        std::fs::write(target.join("Contents/MacOS/vega"), b"candidate bytes")
            .expect("candidate identity");
        std::fs::write(previous.join("Contents/MacOS/vega"), b"previous bytes")
            .expect("backup identity");
        std::fs::write(
            staging.join("install.json"),
            b"retained installation record",
        )
        .expect("installation evidence");
        let previous_metadata = std::fs::symlink_metadata(&previous).expect("backup metadata");
        let manifest = Manifest {
            new_hash: platform::hash(&target.join("Contents/MacOS/vega")).expect("candidate hash"),
            old_hash: platform::hash(&previous.join("Contents/MacOS/vega")).expect("backup hash"),
            target,
            old_version: "1.0.0".into(),
            new_version: "1.0.1".into(),
            device: previous_metadata.dev(),
            inode: previous_metadata.ino(),
            parent_pid: 0,
            delta: false,
        };
        Self {
            _root: root,
            staging,
            manifest,
        }
    }

    fn receipt(&self, name: &str) {
        platform::write_new(&self.staging.join(name), b"ready").expect("fixture receipt");
    }

    fn assert_retained(&self) {
        assert_eq!(
            std::fs::read(self.manifest.target.join("Contents/MacOS/vega"))
                .expect("candidate retained"),
            b"candidate bytes"
        );
        assert_eq!(
            std::fs::read(
                retained_backup(&self.staging, &self.manifest)
                    .expect("exact retained backup")
                    .join("Contents/MacOS/vega"),
            )
            .expect("backup retained"),
            b"previous bytes"
        );
        assert_eq!(
            std::fs::read(self.staging.join("install.json")).expect("record retained"),
            b"retained installation record"
        );
        assert!(!self.staging.join("failed.app").exists());
    }
}

#[test]
fn blocked_startup_exit_or_timeout_retains_candidate_backup_and_recovery_evidence() {
    for acknowledged in [false, true] {
        for status in [CandidateStatus::Exited, CandidateStatus::TimedOut] {
            let fixture = Fixture::new();
            if acknowledged {
                fixture.receipt("startup-ok");
            }
            assert!(finish_startup(&fixture.staging, &fixture.manifest, status).is_err());
            fixture.assert_retained();
            assert!(fixture.staging.join("recovery-required.txt").is_file());
            assert!(!fixture.staging.join("workspace-ready").exists());
        }
    }
}

#[test]
fn launch_receipt_alone_waits_without_deleting_the_backup() {
    let fixture = Fixture::new();
    fixture.receipt("startup-ok");
    assert!(
        !finish_startup(
            &fixture.staging,
            &fixture.manifest,
            CandidateStatus::Running
        )
        .expect("gate remains pending")
    );
    fixture.assert_retained();
    assert!(!fixture.staging.join("recovery-required.txt").exists());
}

#[test]
fn workspace_receipt_requires_launch_receipt_and_matching_candidate_before_cleanup() {
    let missing_launch = Fixture::new();
    missing_launch.receipt("workspace-ready");
    assert!(
        finish_startup(
            &missing_launch.staging,
            &missing_launch.manifest,
            CandidateStatus::Exited
        )
        .is_err()
    );
    missing_launch.assert_retained();

    let changed = Fixture::new();
    changed.receipt("startup-ok");
    changed.receipt("workspace-ready");
    std::fs::write(
        changed.manifest.target.join("Contents/MacOS/vega"),
        b"changed bytes",
    )
    .expect("changed candidate");
    assert!(finish_startup(&changed.staging, &changed.manifest, CandidateStatus::Exited).is_err());
    assert!(changed.staging.join("previous.app").is_dir());
    assert!(changed.staging.join("install.json").is_file());
    assert_eq!(
        std::fs::read(changed.manifest.target.join("Contents/MacOS/vega"))
            .expect("changed candidate untouched"),
        b"changed bytes"
    );

    for status in [CandidateStatus::Running, CandidateStatus::Exited] {
        for moved in [false, true] {
            let ready = Fixture::new();
            if moved {
                exclude_legacy_rollback(&ready.staging, &ready.manifest).expect("backup handoff");
            }
            ready.receipt("startup-ok");
            ready.receipt("workspace-ready");
            assert!(
                finish_startup(&ready.staging, &ready.manifest, status).expect("ready cleanup")
            );
            assert!(!ready.staging.exists());
            assert_eq!(
                std::fs::read(ready.manifest.target.join("Contents/MacOS/vega"))
                    .expect("ready candidate retained"),
                b"candidate bytes"
            );
        }
    }
}

#[test]
fn legacy_absent_receipt_then_candidate_exit_cannot_rollback_after_backup_handoff() {
    let control = Fixture::new();
    rollback_with_candidate_verifier(&control.staging, &control.manifest, None, |path, _, _| {
        platform::hash(&path.join("Contents/MacOS/vega")).map(|_| ())
    })
    .expect("prelaunch rollback remains possible");
    assert_eq!(
        std::fs::read(control.manifest.target.join("Contents/MacOS/vega"))
            .expect("restored backup"),
        b"previous bytes"
    );
    assert_eq!(
        std::fs::read(control.staging.join("failed.app/Contents/MacOS/vega"))
            .expect("retained candidate"),
        b"candidate bytes"
    );

    let fixture = Fixture::new();
    assert!(!fixture.staging.join("startup-ok").is_file());
    exclude_legacy_rollback(&fixture.staging, &fixture.manifest).expect("atomic backup handoff");
    fixture.receipt("startup-ok");
    assert!(
        rollback_with_candidate_verifier(
            &fixture.staging,
            &fixture.manifest,
            None,
            |path, _, _| platform::hash(&path.join("Contents/MacOS/vega")).map(|_| ()),
        )
        .is_err()
    );
    fixture.assert_retained();
    assert!(!fixture.staging.join("previous.app").exists());
    assert!(fixture.staging.join("recovery.app").is_dir());
    assert!(finish_startup(&fixture.staging, &fixture.manifest, CandidateStatus::Exited).is_err());
    fixture.assert_retained();
}

#[test]
fn backup_handoff_is_idempotent_and_refuses_conflicting_or_changed_backup() {
    let fixture = Fixture::new();
    exclude_legacy_rollback(&fixture.staging, &fixture.manifest).expect("first handoff");
    exclude_legacy_rollback(&fixture.staging, &fixture.manifest).expect("same backup retry");
    fixture.assert_retained();

    let conflicting = Fixture::new();
    std::fs::create_dir(conflicting.staging.join("recovery.app")).expect("conflicting destination");
    std::fs::write(
        conflicting.staging.join("recovery.app/retained"),
        b"do not overwrite",
    )
    .expect("destination evidence");
    assert!(exclude_legacy_rollback(&conflicting.staging, &conflicting.manifest).is_err());
    assert_eq!(
        std::fs::read(conflicting.staging.join("recovery.app/retained"))
            .expect("destination retained"),
        b"do not overwrite"
    );
    assert_eq!(
        std::fs::read(conflicting.staging.join("previous.app/Contents/MacOS/vega"))
            .expect("source retained"),
        b"previous bytes"
    );

    let changed = Fixture::new();
    std::fs::write(
        changed.staging.join("previous.app/Contents/MacOS/vega"),
        b"changed backup",
    )
    .expect("changed backup fixture");
    assert!(exclude_legacy_rollback(&changed.staging, &changed.manifest).is_err());
    assert!(!changed.staging.join("recovery.app").exists());
    assert_eq!(
        std::fs::read(changed.staging.join("previous.app/Contents/MacOS/vega"))
            .expect("changed source retained"),
        b"changed backup"
    );
    assert_eq!(
        std::fs::read(changed.manifest.target.join("Contents/MacOS/vega"))
            .expect("candidate unchanged"),
        b"candidate bytes"
    );
}
