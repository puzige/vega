use vega_conversation::McpServerSettingsService;
use vega_conversation::types::{McpEnvironmentVariable, McpServerForm, McpServerTransport};

#[test]
fn copied_authorization_template_loads_as_yaml_configuration() {
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
                environment: vec![McpEnvironmentVariable {
                    variable: "EXAMPLE_KEY".into(),
                }],
            },
        })
        .expect("create without execution");
    let template = service
        .local_credential_binding_template(&server.id)
        .expect("template");
    let template = template.replace("origin: \"\"", "origin: \"https://example.invalid\"");
    std::fs::write(&path, format!("schema_version: 1\n{template}")).expect("paste");
    let loaded = vega_store::config::read_from(&path).expect("read actual config");
    assert_eq!(loaded.credential_bindings.len(), 1);
    assert_eq!(loaded.credential_bindings[0].env, "EXAMPLE_KEY");
    loaded.credential_bindings[0].validate().expect("binding");
    drop(store);
    drop(service);
    std::fs::remove_dir_all(root).expect("cleanup");
}
