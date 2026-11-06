use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().canonicalize().unwrap();
        let root = base.join(format!("configuration-failure-{}", ulid::Ulid::generate()));
        std::fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        fixture.write(b"schema_version: 1\n");
        fixture
    }

    fn path(&self) -> PathBuf {
        self.0.join("config.yaml")
    }

    fn write(&self, bytes: &[u8]) {
        std::fs::write(self.path(), bytes).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rejected(path: &Path) -> io::Error {
    match begin(path) {
        Ok(_) => panic!("unsafe configuration accepted"),
        Err(error) => error,
    }
}

#[test]
fn valid_configuration_is_reread_after_parse_failure() {
    let fixture = Fixture::new();
    assert_eq!(
        begin(&fixture.path()).unwrap().bytes().unwrap(),
        b"schema_version: 1\n"
    );
    fixture.write(b"schema_version: [private-fixture-content\n");
    let error = rejected(&fixture.path());
    assert_eq!(
        failure(&error).unwrap().stage,
        ConfigurationFailureStage::Parse
    );
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(!error.to_string().contains("private-fixture-content"));
    assert!(!error.to_string().contains(fixture.0.to_str().unwrap()));
    fixture.write(b"schema_version: 1\nui:\n  theme: light\n");
    let edit = begin(&fixture.path()).unwrap();
    let config: crate::config::AppConfig = decode(&edit.bytes().unwrap()).unwrap();
    assert_eq!(config.ui.theme, "light");
}

#[test]
fn lock_identity_failure_recovers_only_after_actual_repair() {
    let fixture = Fixture::new();
    drop(begin(&fixture.path()).unwrap());
    let lock = fixture.0.join(".config.lock");
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o644)).unwrap();
    let error = rejected(&fixture.path());
    assert_eq!(
        failure(&error).unwrap().stage,
        ConfigurationFailureStage::LockValidation
    );
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
    drop(begin(&fixture.path()).unwrap());
    std::fs::remove_file(&lock).unwrap();
    symlink(fixture.path(), &lock).unwrap();
    let error = rejected(&fixture.path());
    assert_eq!(
        failure(&error).unwrap().stage,
        ConfigurationFailureStage::LockOpen
    );
    std::fs::remove_file(&lock).unwrap();
    drop(begin(&fixture.path()).unwrap());
}

#[test]
fn configuration_replacement_and_directory_symlinks_remain_rejected() {
    let fixture = Fixture::new();
    let path = fixture.path();
    let old = fixture.0.join("old.yaml");
    std::fs::rename(&path, &old).unwrap();
    symlink(&old, &path).unwrap();
    assert_eq!(
        failure(&rejected(&path)).unwrap().stage,
        ConfigurationFailureStage::Read
    );
    std::fs::remove_file(&path).unwrap();
    fixture.write(b"schema_version: 1\n");
    drop(begin(&path).unwrap());
    let link = fixture.0.join("linked");
    symlink(&fixture.0, &link).unwrap();
    assert_eq!(
        failure(&rejected(&link.join("config.yaml"))).unwrap().stage,
        ConfigurationFailureStage::Directory
    );
}

#[test]
fn compare_and_swap_failure_never_becomes_poison_or_read_failure() {
    let fixture = Fixture::new();
    let edit = begin(&fixture.path()).unwrap();
    fixture.write(b"schema_version: 1\nui:\n  theme: light\n");
    let error = edit.check_current().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert!(failure(&error).is_none());
    drop(edit);
    drop(begin(&fixture.path()).unwrap());
}

#[test]
fn panic_while_holding_edit_lock_stays_fail_closed() {
    let fixture = Fixture::new();
    let path = fixture.path();
    let panic_path = path.clone();
    assert!(
        std::thread::spawn(move || {
            let _edit = begin(&panic_path).unwrap();
            panic!("synthetic holder failure");
        })
        .join()
        .is_err()
    );
    for _ in 0..2 {
        let error = rejected(&path);
        assert_eq!(
            failure(&error).unwrap().stage,
            ConfigurationFailureStage::Poisoned
        );
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(!error.to_string().contains(path.to_str().unwrap()));
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"schema_version: 1\n");
}
