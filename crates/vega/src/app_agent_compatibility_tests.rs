use super::*;

#[test]
fn replay_route_identity_changes_with_actual_api_endpoint_credential_and_owner_domain() {
    use std::path::Path;
    let provider = vega_store::config::ProviderConfig {
        api: vega_store::config::ProviderApi::ChatCompletions,
        enabled: true,
        name: "fixture".into(),
        base_url: "https://proxy.example/v1".into(),
        models: vec!["model".into()],
        key_ref: "fixture-key".into(),
    };
    let identity = provider_replay_route_fingerprint(
        &provider,
        Path::new("/owned/credentials"),
        "credential-one",
        Path::new("/owned/store.sqlite"),
        "project",
    );
    for field in ["api", "endpoint", "key-reference", "alias"] {
        let mut changed = provider.clone();
        match field {
            "api" => changed.api = vega_store::config::ProviderApi::Responses,
            "endpoint" => changed.base_url = "https://other.example/v1".into(),
            "key-reference" => changed.key_ref = "other-key".into(),
            _ => changed.name = "other".into(),
        }
        assert_ne!(
            identity,
            provider_replay_route_fingerprint(
                &changed,
                Path::new("/owned/credentials"),
                "credential-one",
                Path::new("/owned/store.sqlite"),
                "project"
            ),
            "{field}"
        );
    }
    for (root, key, database, project) in [
        (
            "/owned/credentials",
            "credential-two",
            "/owned/store.sqlite",
            "project",
        ),
        (
            "/other/credentials",
            "credential-one",
            "/owned/store.sqlite",
            "project",
        ),
        (
            "/owned/credentials",
            "credential-one",
            "/other/store.sqlite",
            "project",
        ),
        (
            "/owned/credentials",
            "credential-one",
            "/owned/store.sqlite",
            "other-project",
        ),
    ] {
        assert_ne!(
            identity,
            provider_replay_route_fingerprint(
                &provider,
                Path::new(root),
                key,
                Path::new(database),
                project
            )
        );
    }
}
