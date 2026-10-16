use super::*;
use std::fs;
use std::path::Path;

fn write_skill(root: &Path, name: &str, description: &str, body: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("skill directory");
    fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}"),
    )
    .expect("skill file");
}
