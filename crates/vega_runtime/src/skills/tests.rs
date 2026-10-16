use super::*;
use std::fs;
use std::path::Path;

const BODY: &str = "---\nname: example\ndescription: Use for examples.\n---\n\nExample body.\n";

fn write_skill(root: &Path, name: &str, body: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("skill directory");
    fs::write(
        directory.join("SKILL.md"),
        body.replace("name: example", &format!("name: {name}")),
    )
    .expect("skill body");
}
