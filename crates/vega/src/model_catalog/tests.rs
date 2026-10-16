use super::*;

fn payload() -> Vec<u8> {
    br#"{"official":{"id":"official","name":"Official","models":{"model-a":{"id":"model-a","name":"Model A","canonical_model_id":"lab/model-a","limit":{"context":200000,"input":150000,"output":50000},"modalities":{"input":["text"],"output":["text"]},"reasoning":true,"tool_call":true,"reasoning_options":[{"type":"effort","values":["low","high"]}],"cost":{"input":1,"output":2,"cache_read":0.1,"cache_write":0.2}}}}}"#.to_vec()
}
fn owner(provider: &str) -> CatalogIdentity {
    CatalogIdentity {
        provider: provider.into(),
        model: "model-a".into(),
    }
}
fn request(provider: &str, action: CatalogAction) -> ModelCatalogRequested {
    ModelCatalogRequested {
        generation: 1,
        owner: owner(provider),
        action,
    }
}
