//! ohagent-desktop-mcp: MCP server for desktop control.
//!
//! Provides tools for screenshot capture, mouse/keyboard control,
//! accessibility tree inspection, and window management.
//!
//! Protocol: MCP (Model Context Protocol) over stdio JSON-RPC 2.0.
//!
//! ## Usage
//!
//! ```bash
//! cargo run -p ohagent-desktop-mcp
//! ```
//!
//! Register in ~/.jcode/mcp.json:
//! ```json
//! {
//!   "mcpServers": {
//!     "desktop": {
//!       "command": "/path/to/ohagent-desktop-mcp",
//!       "args": []
//!     }
//!   }
//! }
//! ```

mod protocol;
mod tools;

use protocol::{
    ContentBlock, InitializeResult, Request, Response, ServerCapabilities, ServerInfo,
    ToolCallParams, ToolCallResult, ToolsCapability, ToolsListResult,
};
use std::io::{BufRead, Write};
use tools::ToolRegistry;

fn main() {
    let registry = build_registry();

    // JSON-RPC loop: read request from stdin, write response to stdout
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin read error: {e}");
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let response = handle_request(&line, &registry);

        // Write response
        if let Some(resp) = response {
            let json = serde_json::to_string(&resp).unwrap_or_else(|e| {
                format!(r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":-32700,"message":"{e}"}}}}"#)
            });
            println!("{json}");
            let _ = stdout.flush();
        }
    }
}

fn handle_request(line: &str, registry: &ToolRegistry) -> Option<Response> {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Some(Response {
                jsonrpc: "2.0",
                id: None,
                result: None,
                error: Some(protocol::ErrorBody {
                    code: -32700,
                    message: format!("Parse error: {e}"),
                }),
            });
        }
    };

    let id = req.id;

    match req.method.as_str() {
        "initialize" => Some(Response {
            jsonrpc: "2.0",
            id,
            result: Some(serde_json::to_value(InitializeResult {
                protocol_version: "2024-11-05",
                capabilities: ServerCapabilities {
                    tools: ToolsCapability {
                        list_changed: false,
                    },
                },
                server_info: ServerInfo {
                    name: "ohagent-desktop-mcp",
                    version: env!("CARGO_PKG_VERSION"),
                },
            }).unwrap()),
            error: None,
        }),

        "notifications/initialized" => {
            // No response needed for notifications
            None
        }

        "tools/list" => {
            let tools = registry.list();
            Some(Response {
                jsonrpc: "2.0",
                id,
                result: Some(serde_json::to_value(ToolsListResult { tools }).unwrap()),
                error: None,
            })
        }

        "tools/call" => {
            let params: ToolCallParams = match req.params.as_ref().and_then(|p| {
                serde_json::from_value(p.clone()).ok()
            }) {
                Some(p) => p,
                None => {
                    return Some(Response {
                        jsonrpc: "2.0",
                        id,
                        result: None,
                        error: Some(protocol::ErrorBody {
                            code: -32602,
                            message: "Invalid params: expected {name, arguments}".to_string(),
                        }),
                    });
                }
            };

            match registry.call(&params.name, params.arguments) {
                Ok(content) => Some(Response {
                    jsonrpc: "2.0",
                    id,
                    result: Some(
                        serde_json::to_value(ToolCallResult {
                            content,
                            is_error: false,
                        })
                        .unwrap(),
                    ),
                    error: None,
                }),
                Err(msg) => Some(Response {
                    jsonrpc: "2.0",
                    id,
                    result: Some(
                        serde_json::to_value(ToolCallResult {
                            content: vec![ContentBlock::Text {
                                text: msg.clone(),
                            }],
                            is_error: true,
                        })
                        .unwrap(),
                    ),
                    error: None,
                }),
            }
        }

        _ => Some(Response {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(protocol::ErrorBody {
                code: -32601,
                message: format!("Method not found: {}", req.method),
            }),
        }),
    }
}

fn build_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();

    // ── Screenshot ──
    reg.register(
        "screenshot",
        "Take a screenshot of the primary monitor. Optionally specify monitor index, or a crop region (x, y, width, height). Returns a base64-encoded PNG image.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "monitor": {"type": "integer", "description": "Monitor index (0 = primary, default: 0)"},
                "x": {"type": "integer", "description": "Crop region X offset"},
                "y": {"type": "integer", "description": "Crop region Y offset"},
                "width": {"type": "integer", "description": "Crop region width"},
                "height": {"type": "integer", "description": "Crop region height"}
            }
        }),
        tools::screenshot::screenshot,
    );

    // ── Mouse ──
    reg.register(
        "mouse_move",
        "Move the mouse cursor to absolute screen coordinates (x, y).",
        serde_json::json!({
            "type": "object",
            "required": ["x", "y"],
            "properties": {
                "x": {"type": "integer", "description": "X coordinate (pixels from left)"},
                "y": {"type": "integer", "description": "Y coordinate (pixels from top)"}
            }
        }),
        tools::mouse::mouse_move,
    );

    reg.register(
        "mouse_click",
        "Click a mouse button, optionally at specific coordinates. Moves first if x/y given.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "button": {"type": "string", "enum": ["left", "right", "middle"], "description": "Mouse button (default: left)"},
                "x": {"type": "integer", "description": "X coordinate to click at (optional)"},
                "y": {"type": "integer", "description": "Y coordinate to click at (optional)"}
            }
        }),
        tools::mouse::mouse_click,
    );

    reg.register(
        "mouse_drag",
        "Drag the mouse from (x1, y1) to (x2, y2) with left button held.",
        serde_json::json!({
            "type": "object",
            "required": ["x1", "y1", "x2", "y2"],
            "properties": {
                "x1": {"type": "integer", "description": "Start X"},
                "y1": {"type": "integer", "description": "Start Y"},
                "x2": {"type": "integer", "description": "End X"},
                "y2": {"type": "integer", "description": "End Y"}
            }
        }),
        tools::mouse::mouse_drag,
    );

    // ── Keyboard ──
    reg.register(
        "keyboard_type",
        "Type a text string as simulated keystrokes.",
        serde_json::json!({
            "type": "object",
            "required": ["text"],
            "properties": {
                "text": {"type": "string", "description": "Text to type"}
            }
        }),
        tools::keyboard::keyboard_type,
    );

    reg.register(
        "keyboard_press",
        "Press a key combination (e.g., ctrl+c, alt+tab). Provide keys as an array: [\"ctrl\", \"c\"]. Supports: return, tab, space, backspace, delete, escape, arrows, home, end, pageup/down, f1-f12, control/ctrl, alt, shift, meta/cmd, and single characters.",
        serde_json::json!({
            "type": "object",
            "required": ["keys"],
            "properties": {
                "keys": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Keys to press together (modifiers first)"
                }
            }
        }),
        tools::keyboard::keyboard_press,
    );

    // ── Accessibility tree ──
    reg.register(
        "accessibility_tree",
        "Dump the accessibility tree via AT-SPI (Linux only). Allows filtering by application name and limiting tree depth.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "max_depth": {"type": "integer", "description": "Maximum tree depth (default: 3)"},
                "app_name": {"type": "string", "description": "Filter by application name (substring match)"}
            }
        }),
        tools::accessibility::accessibility_tree,
    );

    // ── Window management ──
    reg.register(
        "window_list",
        "List all open windows with their titles and IDs. Linux: wmctrl, macOS: osascript, Windows: PowerShell.",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        tools::window::window_list,
    );

    reg.register(
        "window_focus",
        "Focus a window by title (substring match).",
        serde_json::json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": {"type": "string", "description": "Window title (or substring)"}
            }
        }),
        tools::window::window_focus,
    );

    reg.register(
        "get_screen_size",
        "Get screen dimensions and monitor layout information.",
        serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        tools::window::get_screen_size,
    );

    reg
}
