use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogIdentity {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CatalogReasoningOption {
    Toggle,
    Effort(Vec<Option<String>>),
    Budget { min: Option<i64>, max: Option<u64> },
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCost {
    pub input: Option<String>,
    pub output: Option<String>,
    pub cache_read: Option<String>,
    pub cache_write: Option<String>,
    pub complex: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub canonical_id: Option<String>,
    pub context: u64,
    pub input: Option<u64>,
    pub output: u64,
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub reasoning_options: Vec<CatalogReasoningOption>,
    pub cost: Option<CatalogCost>,
    pub text_pricing: bool,
    pub status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    pub models: Vec<CatalogModel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogAction {
    Load,
    Refresh,
    Map(CatalogIdentity),
    Unmap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelCatalogRequested {
    pub generation: u64,
    pub owner: CatalogIdentity,
    pub action: CatalogAction,
}

#[derive(Clone, Debug, Default)]
pub struct ModelCatalogProjection {
    pub providers: Arc<Vec<CatalogProvider>>,
    pub mapped: Option<CatalogIdentity>,
    pub fetched_at_ms: Option<u64>,
    pub digest: Option<String>,
    pub schema_version: u32,
    pub stale: bool,
    pub refreshing: bool,
    pub error: Option<String>,
}

impl ModelCatalogProjection {
    pub fn matched_model(&self) -> Option<&CatalogModel> {
        let mapped = self.mapped.as_ref()?;
        self.providers
            .iter()
            .find(|p| p.id == mapped.provider)?
            .models
            .iter()
            .find(|m| m.id == mapped.model)
    }
}
