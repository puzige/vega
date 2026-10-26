use std::path::PathBuf;

use super::{CandidateStatus, Manifest, finish_startup};
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
        let manifest = Manifest {
            new_hash: platform::hash(&target.join("Contents/MacOS/vega")).expect("candidate hash"),
            old_hash: platform::hash(&previous.join("Contents/MacOS/vega")).expect("backup hash"),
            target,
            old_version: "1.0.0".into(),
            new_version: "1.0.1".into(),
            device: 0,
            inode: 0,
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
            std::fs::read(self.staging.join("previous.app/Contents/MacOS/vega"))
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
        let ready = Fixture::new();
        ready.receipt("startup-ok");
        ready.receipt("workspace-ready");
        assert!(finish_startup(&ready.staging, &ready.manifest, status).expect("ready cleanup"));
        assert!(!ready.staging.exists());
        assert_eq!(
            std::fs::read(ready.manifest.target.join("Contents/MacOS/vega"))
                .expect("ready candidate retained"),
            b"candidate bytes"
        );
    }
}
