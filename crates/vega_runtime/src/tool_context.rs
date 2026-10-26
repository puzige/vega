use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{RuntimeToolCall, RuntimeToolResult, RuntimeToolStatus};

const BODY_BYTES: usize = 8_192;
const METADATA_BYTES: usize = 1_024;
const MIN_DUPLICATE_BYTES: usize = 1_024;
const MAX_CANDIDATES: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextTool {
    Read,
    Grep,
    Glob,
    Bash,
}

impl TextTool {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "read" | "Read" => Some(Self::Read),
            "grep" => Some(Self::Grep),
            "glob" => Some(Self::Glob),
            "bash" => Some(Self::Bash),
            _ => None,
        }
    }

    fn retrieval(self) -> &'static str {
        match self {
            Self::Read => "Request a narrower read with offset/limit for omitted detail.",
            Self::Grep | Self::Glob => "Narrow the path or pattern for omitted detail.",
            Self::Bash => {
                "Inspect retained output or narrow/filter a safe command for detail. Do not automatically replay a command with side effects."
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolIdentity {
    kind: TextTool,
    parameters: [u8; 32],
    binding: [u8; 32],
}

impl ToolIdentity {
    pub(crate) fn new(id: &str, name: &str, input: &str) -> Option<Self> {
        let kind = TextTool::from_name(name)?;
        let parameters = fingerprint_call(name, input);
        let mut hash = Sha256::new();
        hash_field(&mut hash, 0, &parameters);
        hash_field(&mut hash, 1, id.as_bytes());
        Some(Self {
            kind,
            parameters,
            binding: hash.finalize().into(),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Projection {
    Source,
    First,
    Reference,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolContext {
    identity: ToolIdentity,
    canonical_digest: [u8; 32],
    complete_key: Option<[u8; 32]>,
    output_bytes: usize,
    status: RuntimeToolStatus,
    exit_code: Option<i32>,
    truncated: Option<bool>,
    projection: Projection,
    projected_digest: Option<[u8; 32]>,
    projected_binding: Option<[u8; 32]>,
}

impl ToolContext {
    pub(crate) fn new(
        call: &RuntimeToolCall,
        result: &RuntimeToolResult,
        canonical: &str,
    ) -> Option<Self> {
        let identity = ToolIdentity::new(&call.id, &call.name, &call.input_json)?;
        if canonical.len() < MIN_DUPLICATE_BYTES {
            return None;
        }
        let complete = identity.kind != TextTool::Bash
            && result.status == RuntimeToolStatus::Success
            && result.exit_code.is_none_or(|code| code == 0)
            && result.truncated == Some(false);
        let complete_key = complete.then(|| {
            let mut hash = Sha256::new();
            hash_field(&mut hash, 0, &identity.parameters);
            hash_field(&mut hash, 1, result.output.as_bytes());
            hash_field(&mut hash, 2, &[0]);
            if let Some(code) = result.exit_code {
                hash_field(&mut hash, 3, &code.to_le_bytes());
            }
            hash_field(&mut hash, 4, &[0]);
            hash.finalize().into()
        });
        Some(Self {
            identity,
            canonical_digest: digest(canonical),
            complete_key,
            output_bytes: result.output.len(),
            status: result.status,
            exit_code: result.exit_code,
            truncated: result.truncated,
            projection: Projection::Source,
            projected_digest: None,
            projected_binding: None,
        })
    }

    pub(crate) fn seal_projection(&mut self, content: &str, identity: Option<ToolIdentity>) {
        self.projected_digest = Some(digest(content));
        self.projected_binding = identity.map(|identity| identity.binding);
    }
}

#[derive(Default)]
pub(crate) struct ToolContextProjector {
    candidates: BTreeMap<[u8; 32], String>,
}

impl ToolContextProjector {
    pub(crate) fn observe_projected(
        &mut self,
        id: &str,
        content: &str,
        identity: Option<ToolIdentity>,
        metadata: Option<ToolContext>,
    ) {
        if let Some(metadata) = metadata
            && metadata.projection == Projection::First
            && metadata.projected_digest == Some(digest(content))
            && identity.is_some_and(|identity| metadata.projected_binding == Some(identity.binding))
            && let Some(key) = metadata.complete_key
        {
            self.register(key, id, metadata.output_bytes);
        }
    }

    fn register(&mut self, key: [u8; 32], id: &str, bytes: usize) {
        if self.candidates.len() >= MAX_CANDIDATES
            || self.candidates.contains_key(&key)
            || id.len() > METADATA_BYTES
        {
            return;
        }
        let reference = serde_json::json!({"vega_tool_result_reference":{"authority":"none","call_id":id,"bytes":bytes,"status":"success","truncated":false}}).to_string();
        if reference.len() <= METADATA_BYTES {
            self.candidates.insert(key, reference);
        }
    }

    pub(crate) fn project(
        &mut self,
        id: &str,
        content: &str,
        identity: Option<ToolIdentity>,
        metadata: Option<ToolContext>,
        cold: bool,
    ) -> (String, Option<ToolContext>) {
        let Some(identity) = identity else {
            return (content.into(), None);
        };
        let mut metadata = metadata.filter(|metadata| {
            metadata.identity == identity
                && metadata.canonical_digest == digest(content)
                && metadata.projection == Projection::Source
        });
        if !cold
            && let Some(metadata) = &mut metadata
            && let Some(key) = metadata.complete_key
        {
            if let Some(reference) = self.candidates.get(&key) {
                metadata.projection = Projection::Reference;
                return (reference.clone(), Some(*metadata));
            }
            self.register(key, id, metadata.output_bytes);
            metadata.projection = Projection::First;
        } else if let Some(metadata) = &mut metadata {
            metadata.complete_key = None;
            metadata.projection = Projection::First;
        }
        (preview(identity.kind, content, metadata), metadata)
    }
}

fn preview(kind: TextTool, content: &str, metadata: Option<ToolContext>) -> String {
    if content.len() <= BODY_BYTES {
        return content.into();
    }
    let mut head = BODY_BYTES / 2;
    while !content.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = content.len() - BODY_BYTES / 2;
    while !content.is_char_boundary(tail) {
        tail += 1;
    }
    let status = metadata.map(|metadata| match metadata.status {
        RuntimeToolStatus::Success => "success",
        RuntimeToolStatus::Failed => "failed",
        RuntimeToolStatus::Rejected => "rejected",
        RuntimeToolStatus::Cancelled => "cancelled",
    });
    let info = serde_json::json!({"vega_tool_preview":{
        "authority":"none", "source_bytes":content.len(), "omitted_bytes":tail-head,
        "runtime_bytes":metadata.map(|value| value.output_bytes), "status":status,
        "exit_code":metadata.and_then(|value| value.exit_code),
        "truncated":metadata.and_then(|value| value.truncated), "retrieve":kind.retrieval()
    }})
    .to_string();
    format!("{}\n{}\n{}", &content[..head], info, &content[tail..])
}

fn digest(content: &str) -> [u8; 32] {
    Sha256::digest(content.as_bytes()).into()
}

pub(crate) fn hash_field(hash: &mut Sha256, tag: u8, bytes: &[u8]) {
    hash.update([tag]);
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn json_digest(hash: &mut Sha256, value: &Value) {
    match value {
        Value::Null => hash_field(hash, 0, &[]),
        Value::Bool(value) => hash_field(hash, 1, &[u8::from(*value)]),
        Value::Number(value) => hash_field(hash, 2, value.to_string().as_bytes()),
        Value::String(value) => hash_field(hash, 3, value.as_bytes()),
        Value::Array(values) => {
            hash_field(hash, 4, &(values.len() as u64).to_le_bytes());
            for value in values {
                json_digest(hash, value);
            }
        }
        Value::Object(values) => {
            hash_field(hash, 5, &(values.len() as u64).to_le_bytes());
            for (key, value) in values.iter().collect::<BTreeMap<_, _>>() {
                hash_field(hash, 6, key.as_bytes());
                json_digest(hash, value);
            }
        }
    }
}

pub(crate) fn fingerprint_call(name: &str, input: &str) -> [u8; 32] {
    let name = match name {
        "Read" => "read",
        "Write" => "write",
        "Edit" => "edit",
        other => other,
    };
    let mut hash = Sha256::new();
    hash_field(&mut hash, 0, name.as_bytes());
    if let Ok(value) = serde_json::from_str::<Value>(input) {
        json_digest(&mut hash, &value);
    } else {
        hash_field(&mut hash, 7, input.as_bytes());
    }
    hash.finalize().into()
}
