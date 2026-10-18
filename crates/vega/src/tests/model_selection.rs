use super::*;

pub(super) fn model_selection_config(path: &std::path::Path) {
    fs::write(
        path,
        r#"[[providers]]
name = "owned"
base_url = "https://provider.invalid/v1"
models = ["gpt-5.6-terra", "gpt-5.6-luna"]
key_ref = "owned"

[defaults]
model = "gpt-5.6-terra"
permission_mode = "confirm"

[ui]
theme = "dark"
"#,
    )
    .expect("owned model config");
}
