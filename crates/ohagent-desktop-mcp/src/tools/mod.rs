//! Tool registry — maps tool names to their schemas and handler functions.

pub mod accessibility;
pub mod keyboard;
pub mod mouse;
pub mod screenshot;
pub mod window;

use crate::protocol::{ContentBlock, ToolDef};
use serde_json::Value;
use std::collections::HashMap;

/// A tool handler: takes JSON arguments, returns content blocks.
pub type ToolHandler = fn(Value) -> Result<Vec<ContentBlock>, String>;

/// Registry of available tools.
pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
    /// Ordered list for stable `tools/list` output.
    order: Vec<String>,
}

struct RegisteredTool {
    def: ToolDef,
    handler: ToolHandler,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Register a tool.
    pub fn register(
        &mut self,
        name: &str,
        description: &str,
        input_schema: Value,
        handler: ToolHandler,
    ) {
        let def = ToolDef {
            name: name.to_string(),
            description: description.to_string(),
            input_schema,
        };
        self.tools.insert(
            name.to_string(),
            RegisteredTool { def, handler },
        );
        self.order.push(name.to_string());
    }

    /// List all tool definitions (in registration order).
    pub fn list(&self) -> Vec<ToolDef> {
        self.order
            .iter()
            .filter_map(|name| self.tools.get(name).map(|t| t.def.clone()))
            .collect()
    }

    /// Execute a tool by name.
    pub fn call(&self, name: &str, arguments: Value) -> Result<Vec<ContentBlock>, String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| format!("Unknown tool: {name}"))?;
        (tool.handler)(arguments)
    }
}
