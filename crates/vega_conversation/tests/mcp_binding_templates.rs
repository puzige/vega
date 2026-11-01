use vega_conversation::McpServerSettingsService;
use vega_conversation::types::{McpEnvironmentVariable, McpServerForm, McpServerTransport};

fn verify_template(variables: &[&str]) {
    let root = std::env::temp_dir()
        .canonicalize()
        .expect("temporary root")
        .join(format!("vega-template-{}", ulid::Ulid::generate()));
    let path = root.join("config.yaml");
    vega_store::config::load_from(&path).expect("initialize");
    let database = root.join("vega.db");
    let store = vega_store::Store::open(&database).expect("store");
    store.migrate().expect("schema");
    let service = McpServerSettingsService::new(database, root.clone());
    let server = service
        .create(McpServerForm {
            display_name: "Synthetic".into(),
            transport: McpServerTransport::Local {
                executable: "/synthetic/mcp".into(),
                args: vec![],
                working_directory: None,
                environment: variables
                    .iter()
                    .map(|variable| McpEnvironmentVariable {
                        variable: (*variable).into(),
                    })
                    .collect(),
            },
        })
        .expect("create without execution");
    let template = service
        .local_credential_binding_template(&server.id)
        .expect("template");
    if variables.is_empty() {
        assert_eq!(template, "credential_bindings: []\n");
    }
    let template = template.replace("origin: \"\"", "origin: \"https://example.invalid\"");
    std::fs::write(&path, format!("schema_version: 1\n{template}")).expect("paste");
    let loaded = vega_store::config::read_from(&path).expect("read actual config");
    assert_eq!(loaded.credential_bindings.len(), variables.len());
    for (binding, variable) in loaded.credential_bindings.iter().zip(variables) {
        assert_eq!(binding.env, *variable);
        binding.validate().expect("binding");
    }
    drop(store);
    drop(service);
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn empty_local_environment_template_loads_as_empty_bindings() {
    verify_template(&[]);
}

#[test]
fn populated_local_environment_template_loads_as_valid_bindings() {
    verify_template(&["EXAMPLE_KEY"]);
}
