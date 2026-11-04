use super::source::*;
use super::{RunBinding, SkillApproval, SkillCatalog, SkillRun, SkillSelection};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct TestError(String);

impl From<SkillError> for TestError {
    fn from(error: SkillError) -> Self {
        Self(format!("{error:?}"))
    }
}
impl From<std::io::Error> for TestError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}
impl From<serde_json::Error> for TestError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}
impl From<std::num::ParseIntError> for TestError {
    fn from(error: std::num::ParseIntError) -> Self {
        Self(error.to_string())
    }
}
impl From<&str> for TestError {
    fn from(error: &str) -> Self {
        Self(error.to_string())
    }
}

type Result<T = ()> = std::result::Result<T, TestError>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const BODY: &str = "---\nname: example\ndescription: Example workflow\n---\nReviewed body\n";

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    target: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self> {
        let base = std::env::temp_dir().join(format!(
            "skill-boundary-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("catalog");
        let target = base.join("target");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(target.join("references"))?;
        fs::create_dir_all(target.join("assets"))?;
        fs::write(target.join("SKILL.md"), BODY)?;
        fs::write(target.join("references/first.md"), "First reference")?;
        fs::write(target.join("references/next.md"), "Next reference")?;
        fs::write(target.join("assets/file.bin"), b"asset")?;
        symlink(&target, root.join("example"))?;
        Ok(Self { base, root, target })
    }
    fn source(&self) -> Result<SkillSource> {
        Ok(SkillSource::imported_approved(&self.root, 0)?)
    }
    fn candidate(&self) -> Result<SkillCandidate> {
        let mut candidates = self.source()?.discover()?.candidates;
        if candidates.len() != 1 {
            return Err("expected one valid candidate".into());
        }
        Ok(candidates.remove(0))
    }
    fn retarget(&self, target: &Path) -> Result {
        symlink(target, self.root.join("next"))?;
        fs::rename(self.root.join("next"), self.root.join("example"))?;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn bound_target_serves_body_reference_and_asset_but_never_internal_links() -> Result {
    let fixture = Fixture::new()?;
    let candidate = fixture.candidate()?;
    let source = &candidate.source;
    assert_eq!(source.load_candidate(&candidate)?, BODY.as_bytes());
    assert_eq!(
        source.read_reference("example", "references/first.md")?,
        "First reference"
    );
    assert_eq!(
        source.inspect_asset_metadata("example", "assets/file.bin")?,
        5
    );
    let outside = fixture.base.join("outside");
    fs::write(&outside, "Secret")?;
    symlink(&outside, fixture.target.join("references/symlink.md"))?;
    fs::hard_link(&outside, fixture.target.join("references/hardlink.md"))?;
    symlink(
        fixture.base.as_path(),
        fixture.target.join("references/directory"),
    )?;
    symlink(&outside, fixture.target.join("assets/symlink.bin"))?;
    fs::hard_link(&outside, fixture.target.join("assets/hardlink.bin"))?;
    assert_eq!(
        source.read_reference("example", "references/symlink.md"),
        Err(SkillError::UnsafePath)
    );
    assert_eq!(
        source.read_reference("example", "references/hardlink.md"),
        Err(SkillError::Hardlink)
    );
    assert_eq!(
        source.read_reference("example", "references/directory/outside"),
        Err(SkillError::UnsafePath)
    );
    assert_eq!(
        source.inspect_asset_metadata("example", "assets/symlink.bin"),
        Err(SkillError::UnsafePath)
    );
    assert_eq!(
        source.inspect_asset_metadata("example", "assets/hardlink.bin"),
        Err(SkillError::Hardlink)
    );
    for path in [
        "references/../../outside",
        "/references/first.md",
        "references/./first.md",
        "references//first.md",
    ] {
        assert_eq!(
            source.read_reference("example", path),
            Err(SkillError::UnsafePath)
        );
    }
    fs::remove_file(fixture.target.join("SKILL.md"))?;
    symlink(&outside, fixture.target.join("SKILL.md"))?;
    assert_eq!(
        source.load_candidate(&candidate),
        Err(SkillError::UnsafePath)
    );
    Ok(())
}

#[test]
fn same_bytes_at_another_target_or_replaced_target_never_match_prior_approval() -> Result {
    let fixture = Fixture::new()?;
    let before = fixture.candidate()?;
    let approval = SkillApproval::reviewed(&before, "skill-example", true, false)?;
    let other = fixture.base.join("other");
    fs::create_dir(&other)?;
    fs::write(other.join("SKILL.md"), BODY)?;
    fixture.retarget(&other)?;
    let after = fixture.candidate()?;
    assert_eq!(before.sha256, after.sha256);
    assert_ne!(
        SkillSelection::from_candidate(&before),
        SkillSelection::from_candidate(&after)
    );
    assert!(before.source.load_candidate(&before).is_err());
    let catalog = SkillCatalog::freeze(vec![after], std::slice::from_ref(&approval), false)?;
    assert_eq!(catalog.exclusions().len(), 1);
    let mut run = SkillRun::new(catalog, true);
    assert!(
        run.load_explicit(&approval.selection(), |_| true)
            .must_pause
    );
    fixture.retarget(&fixture.target)?;
    let before = fixture.candidate()?;
    fs::rename(&fixture.target, fixture.base.join("old-target"))?;
    fs::create_dir(&fixture.target)?;
    fs::write(fixture.target.join("SKILL.md"), BODY)?;
    assert!(before.source.load_candidate(&before).is_err());
    let replacement = fixture.candidate()?;
    assert_ne!(
        SkillSelection::from_candidate(&before),
        SkillSelection::from_candidate(&replacement)
    );
    Ok(())
}

#[test]
fn frozen_content_survives_retarget_while_unread_resources_remain_fenced() -> Result {
    let fixture = Fixture::new()?;
    let candidate = fixture.candidate()?;
    let approval = SkillApproval::reviewed(&candidate, "skill-example", true, false)?;
    let catalog = SkillCatalog::freeze(vec![candidate], std::slice::from_ref(&approval), false)?;
    let binding = RunBinding::for_catalog("run", "thread", 1, 0, &catalog)?;
    let mut run = SkillRun::new_bound(catalog, true, binding.clone())?;
    assert_eq!(
        run.load_explicit(&approval.selection(), |_| true)
            .receipt
            .status,
        "loaded"
    );
    let frozen = run.read_reference("example", "references/first.md", |_| true)?;
    let snapshot = run.export_snapshot()?;
    let other = fixture.base.join("other");
    fs::create_dir(&other)?;
    fs::write(other.join("SKILL.md"), BODY)?;
    fixture.retarget(&other)?;
    assert!(run.render_skill_envelope()?.contains("Reviewed body"));
    assert_eq!(
        run.read_reference("example", "references/first.md", |_| true)?,
        frozen
    );
    assert!(
        run.read_reference("example", "references/next.md", |_| true)
            .is_err()
    );
    let mut restored = SkillRun::restore_snapshot(snapshot.bytes(), &binding, snapshot.sha256())?;
    assert_eq!(restored.active_selections(), run.active_selections());
    assert_eq!(
        restored.read_reference("example", "references/first.md", |_| true)?,
        frozen
    );
    assert!(
        restored
            .read_reference("example", "references/next.md", |_| true)
            .is_err()
    );
    Ok(())
}

#[test]
fn ordinary_approval_does_not_become_link_consent_and_empty_fields_preserve_old_shape() -> Result {
    let fixture = Fixture::new()?;
    fs::remove_file(fixture.root.join("example"))?;
    fs::create_dir(fixture.root.join("example"))?;
    fs::write(fixture.root.join("example/SKILL.md"), BODY)?;
    let ordinary = fixture.candidate()?;
    assert!(ordinary.source.target_binding()?.is_none());
    let value = serde_json::to_value(&ordinary)?;
    assert!(value["source"].get("linked_target").is_none());
    let approval = SkillApproval::reviewed(&ordinary, "skill-example", true, false)?;
    assert!(
        serde_json::to_value(approval.selection())?
            .get("target")
            .is_none()
    );
    let catalog = SkillCatalog::freeze(vec![ordinary], std::slice::from_ref(&approval), false)?;
    let binding = RunBinding::for_catalog("run", "thread", 1, 0, &catalog)?;
    let mut run = SkillRun::new_bound(catalog, true, binding.clone())?;
    assert_eq!(
        run.load_explicit(&approval.selection(), |_| true)
            .receipt
            .status,
        "loaded"
    );
    let snapshot = run.export_snapshot()?;
    assert!(SkillRun::restore_snapshot(snapshot.bytes(), &binding, snapshot.sha256()).is_ok());
    fs::rename(fixture.root.join("example"), fixture.root.join("old"))?;
    symlink(&fixture.target, fixture.root.join("example"))?;
    let linked = fixture.candidate()?;
    assert_eq!(linked.sha256, approval.selection().sha256());
    let catalog = SkillCatalog::freeze(vec![linked], std::slice::from_ref(&approval), false)?;
    assert_eq!(catalog.exclusions().len(), 1);
    assert!(
        SkillRun::new(catalog, true)
            .load_explicit(&approval.selection(), |_| true)
            .must_pause
    );
    Ok(())
}

#[test]
fn cyclic_links_hardlinked_bodies_and_invalid_metadata_remain_excluded() -> Result {
    let fixture = Fixture::new()?;
    symlink("cycle", fixture.root.join("cycle"))?;
    let discovery = fixture.source()?.discover()?;
    assert_eq!(discovery.candidates.len(), 1);
    assert!(
        discovery
            .diagnostics
            .iter()
            .any(|item| item.name == "cycle")
    );
    fs::hard_link(
        fixture.target.join("SKILL.md"),
        fixture.base.join("duplicate.md"),
    )?;
    let discovery = fixture.source()?.discover()?;
    assert!(discovery.candidates.is_empty());
    assert!(
        discovery
            .diagnostics
            .iter()
            .any(|item| item.name == "example" && item.error == SkillError::Hardlink)
    );
    let nested = b"---\nname: example\ndescription: Example\nmetadata:\n  openclaw:\n    name: nested\n---\nBody";
    assert_eq!(
        super::parse_skill_md(nested, "example"),
        Err(SkillError::InvalidMetadata)
    );
    assert_eq!(
        super::parse_skill_md(BODY.as_bytes(), "different"),
        Err(SkillError::InvalidName)
    );
    assert_eq!(
        super::parse_skill_md(b"---\nname: [\n---\nBody", "example"),
        Err(SkillError::MalformedYaml)
    );
    Ok(())
}
