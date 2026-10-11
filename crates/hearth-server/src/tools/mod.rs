//! The tool registry: every operation the assistant, the UI and the API share.
//!
//! One definition per tool (name, description, JSON schema, handler). Reads
//! are open to any caller; writes need a `read_write` key and pass through the
//! autonomy gate when the caller is the assistant. The MCP server, the
//! `/api/tools/{name}` endpoint, proposal approval and undo all call
//! [`call`], so the UI and the assistant can never disagree.

mod read;
mod write;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;

use hearth_core::settings::AutonomyArea;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::api::AppState;
use crate::services::{self, Actor, Ctx};

pub type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("unknown tool {0}")]
    Unknown(String),
    #[error("invalid arguments: {0}")]
    Args(String),
    #[error("this key is read-only")]
    ReadOnly,
    #[error(transparent)]
    Service(#[from] services::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ToolError>;

/// Everything a handler needs; owned so handlers are plain `fn` pointers.
#[derive(Clone)]
pub struct ToolCtx {
    pub state: AppState,
    pub ctx: Ctx,
    pub actor: Actor,
    pub can_write: bool,
}

pub type Handler = fn(ToolCtx, Value) -> BoxFut<Result<Value>>;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: fn() -> Value,
    pub writes: bool,
    pub area: Option<AutonomyArea>,
    pub handler: Handler,
}

impl ToolDef {
    pub fn schema(&self) -> Value {
        (self.input_schema)()
    }
}

pub(crate) fn parse_args<T: DeserializeOwned>(args: Value) -> Result<T> {
    let args = if args.is_null() {
        Value::Object(Default::default())
    } else {
        args
    };
    serde_json::from_value(args).map_err(|e| ToolError::Args(e.to_string()))
}

pub(crate) fn schema_of<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or_default()
}

/// Define a tool with a typed input and an async body.
macro_rules! tool {
    ($name:literal, $desc:literal, $input:ty, writes = $writes:expr, area = $area:expr, |$tc:ident, $input_var:ident| $body:block) => {
        $crate::tools::ToolDef {
            name: $name,
            description: $desc,
            input_schema: || $crate::tools::schema_of::<$input>(),
            writes: $writes,
            area: $area,
            handler: |tc, args| {
                Box::pin(async move {
                    let $input_var: $input = $crate::tools::parse_args(args)?;
                    let $tc = tc;
                    let out = $body;
                    Ok(serde_json::to_value(out)?)
                })
            },
        }
    };
}
pub(crate) use tool;

static REGISTRY: LazyLock<BTreeMap<&'static str, ToolDef>> = LazyLock::new(|| {
    let mut m = BTreeMap::new();
    for t in read::tools().into_iter().chain(write::tools()) {
        assert!(m.insert(t.name, t).is_none(), "duplicate tool name");
    }
    m
});

pub fn all() -> impl Iterator<Item = &'static ToolDef> {
    REGISTRY.values()
}

pub fn get(name: &str) -> Option<&'static ToolDef> {
    REGISTRY.get(name)
}

/// Invoke a tool by name.
pub async fn call(tc: ToolCtx, name: &str, args: Value) -> Result<Value> {
    let def = get(name).ok_or_else(|| ToolError::Unknown(name.to_string()))?;
    if def.writes && !tc.can_write {
        return Err(ToolError::ReadOnly);
    }
    (def.handler)(tc, args).await
}

/// Shorthand used by write tools: run `apply` under the autonomy gate.
pub(crate) async fn gated<T, F, Fut>(
    tc: &ToolCtx,
    area: AutonomyArea,
    tool: &str,
    args: &Value,
    summary: String,
    refs: (Option<uuid::Uuid>, Option<uuid::Uuid>),
    apply: F,
) -> Result<services::assistant::Gated<T>>
where
    T: serde::Serialize,
    F: FnOnce() -> Fut,
    Fut: Future<
        Output = services::Result<(
            T,
            Option<Value>,
            Option<Value>,
            Option<services::questions::Proposal>,
        )>,
    >,
{
    let intent = services::assistant::WriteIntent {
        area,
        tool: tool.to_string(),
        args: args.clone(),
        summary,
        entity: None,
        entity_id: None,
        transaction_id: refs.0,
        bill_id: refs.1,
    };
    Ok(services::assistant::gate(&tc.ctx, &tc.actor, intent, apply).await?)
}

pub(crate) fn undo(
    tool: &str,
    args: Value,
    summary: impl Into<String>,
) -> Option<services::questions::Proposal> {
    Some(services::questions::Proposal {
        tool: tool.to_string(),
        args,
        summary: summary.into(),
    })
}

pub(crate) fn json<T: serde::Serialize>(v: &T) -> Option<Value> {
    serde_json::to_value(v).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_well_formed() {
        let names: Vec<&str> = all().map(|t| t.name).collect();
        assert!(
            names.len() > 40,
            "expected a full surface, got {}",
            names.len()
        );
        for t in all() {
            assert!(t.name.contains('.'), "{} should be <noun>.<verb>", t.name);
            assert!(!t.description.is_empty());
            let s = t.schema();
            assert!(s.is_object(), "{} schema", t.name);
            if t.writes {
                assert!(
                    t.area.is_some()
                        || t.name.starts_with("alerts.")
                        || t.name.starts_with("questions.")
                        || t.name.starts_with("assistant.")
                        || t.name.starts_with("jobs."),
                    "{} needs an area",
                    t.name
                );
            }
        }
    }
}
