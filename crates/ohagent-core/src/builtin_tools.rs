//! Built-in tools for the agent: bash, write, edit, read, ls.
//!
//! These are the core tools that make ohAgent a real coding agent,
//! not just a chatbot. Registered on the bridge at startup and
//! available to the agent via tool-calling.

use std::process::Command;
use std::sync::Arc;

use crate::tools::{Tool, ToolRegistry, ToolResult};

/// Register all built-in tools on a registry.
pub fn register_builtin_tools(registry: &mut ToolRegistry, workspace_dir: &str) {
    registry.register(bash_tool(workspace_dir));
    registry.register(write_tool(workspace_dir));
    registry.register(edit_tool(workspace_dir));
    registry.register(read_tool(workspace_dir));
    registry.register(ls_tool(workspace_dir));
}

// ── bash ──

fn bash_tool(workspace_dir: &str) -> Tool {
    let dir = workspace_dir.to_string();
    Tool {
        name: "bash".into(),
        description: "Run a bash command in the workspace. Returns stdout + stderr + exit code. \
                      Use for: building, testing, git, file operations, package management.".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000 = 2min)"
                }
            }
        }),
        handler: Arc::new(move |params| {
            let command = match params.get("command").and_then(|v| v.as_str()) {
                Some(c) => c.to_string(),
                None => return ToolResult::err("Missing command", "command is required"),
            };

            let _timeout_ms = params
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(120_000);

            let output = match Command::new("bash")
                .arg("-c")
                .arg(&command)
                .current_dir(&dir)
                .output()
            {
                Ok(o) => o,
                Err(e) => return ToolResult::err(
                    format!("Failed to execute: {e}"),
                    format!("{e}"),
                ),
            };

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output.status.code().unwrap_or(-1);

            // Truncate long output
            let max_len = 8000;
            let truncate = |s: &str| {
                if s.len() > max_len {
                    format!("{}... (truncated, {} total bytes)", &s[..max_len], s.len())
                } else {
                    s.to_string()
                }
            };

            let result = format!(
                "exit_code: {exit_code}\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                truncate(&stdout),
                truncate(&stderr),
            );

            let data = serde_json::json!({
                "exit_code": exit_code,
                "stdout_len": stdout.len(),
                "stderr_len": stderr.len(),
            });

            // Consider non-zero exit as "error" but still return output
            if exit_code == 0 {
                ToolResult::ok_with_data(result, data)
            } else {
                // Return success=true but output includes the error
                // so the agent can decide what to do
                ToolResult::ok_with_data(result, data)
            }
        }),
    }
}

// ── write ──

fn write_tool(workspace_dir: &str) -> Tool {
    let dir = workspace_dir.to_string();
    Tool {
        name: "write".into(),
        description: "Write content to a file in the workspace. Creates parent directories if needed. \
                      Overwrites existing files.".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "required": ["file_path", "content"],
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file, relative to workspace or absolute"
                },
                "content": {
                    "type": "string",
                    "description": "File content to write"
                }
            }
        }),
        handler: Arc::new(move |params| {
            let file_path = match params.get("file_path").and_then(|v| v.as_str()) {
                Some(p) => p.to_string(),
                None => return ToolResult::err("Missing file_path", "file_path is required"),
            };
            let content = match params.get("content").and_then(|v| v.as_str()) {
                Some(c) => c.to_string(),
                None => return ToolResult::err("Missing content", "content is required"),
            };

            let full_path = if std::path::Path::new(&file_path).is_absolute() {
                std::path::PathBuf::from(&file_path)
            } else {
                std::path::PathBuf::from(&dir).join(&file_path)
            };

            // Create parent directories
            if let Some(parent) = full_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return ToolResult::err(
                        format!("Failed to create directory: {e}"),
                        format!("{e}"),
                    );
                }
            }

            match std::fs::write(&full_path, &content) {
                Ok(()) => ToolResult::ok(format!("Wrote {} bytes to {}", content.len(), full_path.display())),
                Err(e) => ToolResult::err(
                    format!("Failed to write file: {e}"),
                    format!("{e}"),
                ),
            }
        }),
    }
}

// ── edit ──

fn edit_tool(workspace_dir: &str) -> Tool {
    let dir = workspace_dir.to_string();
    Tool {
        name: "edit".into(),
        description: "Replace text in a file. Finds old_string and replaces with new_string. \
                      Use replace_all=true to replace all occurrences.".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "required": ["file_path", "old_string", "new_string"],
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "old_string": {
                    "type": "string",
                    "description": "Text to find and replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false)"
                }
            }
        }),
        handler: Arc::new(move |params| {
            let file_path = match params.get("file_path").and_then(|v| v.as_str()) {
                Some(p) => p.to_string(),
                None => return ToolResult::err("Missing file_path", "file_path is required"),
            };
            let old_string = match params.get("old_string").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return ToolResult::err("Missing old_string", "old_string is required"),
            };
            let new_string = match params.get("new_string").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return ToolResult::err("Missing new_string", "new_string is required"),
            };
            let replace_all = params.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);

            let full_path = if std::path::Path::new(&file_path).is_absolute() {
                std::path::PathBuf::from(&file_path)
            } else {
                std::path::PathBuf::from(&dir).join(&file_path)
            };

            let original = match std::fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(e) => return ToolResult::err(format!("Failed to read: {e}"), format!("{e}")),
            };

            if replace_all {
                let count = original.matches(&old_string).count();
                if count == 0 {
                    return ToolResult::err(
                        format!("old_string not found in {}", full_path.display()),
                        "not_found",
                    );
                }
                let updated = original.replace(&old_string, &new_string);
                match std::fs::write(&full_path, &updated) {
                    Ok(()) => ToolResult::ok(format!(
                        "Replaced {} occurrence(s) in {}", count, full_path.display()
                    )),
                    Err(e) => ToolResult::err(format!("Failed to write: {e}"), format!("{e}")),
                }
            } else {
                if !original.contains(&old_string) {
                    return ToolResult::err(
                        format!("old_string not found in {}", full_path.display()),
                        "not_found",
                    );
                }
                let updated = original.replacen(&old_string, &new_string, 1);
                match std::fs::write(&full_path, &updated) {
                    Ok(()) => ToolResult::ok(format!("Edited {}", full_path.display())),
                    Err(e) => ToolResult::err(format!("Failed to write: {e}"), format!("{e}")),
                }
            }
        }),
    }
}

// ── read ──

fn read_tool(workspace_dir: &str) -> Tool {
    let dir = workspace_dir.to_string();
    Tool {
        name: "read".into(),
        description: "Read the contents of a file. Optionally specify start_line and limit \
                      for partial reads.".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "required": ["file_path"],
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file"
                },
                "start_line": {
                    "type": "integer",
                    "description": "1-based start line (default: 1)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max lines to read (default: 500)"
                }
            }
        }),
        handler: Arc::new(move |params| {
            let file_path = match params.get("file_path").and_then(|v| v.as_str()) {
                Some(p) => p.to_string(),
                None => return ToolResult::err("Missing file_path", "file_path is required"),
            };
            let start_line = params.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(500) as usize;

            let full_path = if std::path::Path::new(&file_path).is_absolute() {
                std::path::PathBuf::from(&file_path)
            } else {
                std::path::PathBuf::from(&dir).join(&file_path)
            };

            let content = match std::fs::read_to_string(&full_path) {
                Ok(c) => c,
                Err(e) => return ToolResult::err(format!("Failed to read: {e}"), format!("{e}")),
            };

            let lines: Vec<&str> = content.lines().collect();
            let total = lines.len();
            let start_idx = (start_line.saturating_sub(1)).min(total);
            let end_idx = (start_idx + limit).min(total);
            let selected: Vec<String> = lines[start_idx..end_idx]
                .iter()
                .enumerate()
                .map(|(i, l)| format!("{:6}| {}", start_idx + i + 1, l))
                .collect();

            let output = format!(
                "{} (lines {}-{} of {})\n{}",
                full_path.display(),
                start_idx + 1,
                end_idx,
                total,
                selected.join("\n"),
            );

            ToolResult::ok_with_data(output, serde_json::json!({
                "total_lines": total,
                "read_lines": selected.len(),
            }))
        }),
    }
}

// ── ls ──

fn ls_tool(workspace_dir: &str) -> Tool {
    let dir = workspace_dir.to_string();
    Tool {
        name: "ls".into(),
        description: "List directory contents. Returns file/directory names and types.".into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path (default: workspace root)"
                }
            }
        }),
        handler: Arc::new(move |params| {
            let target = params
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");

            let full_path = if std::path::Path::new(target).is_absolute() {
                std::path::PathBuf::from(target)
            } else {
                std::path::PathBuf::from(&dir).join(target)
            };

            let entries = match std::fs::read_dir(&full_path) {
                Ok(e) => e,
                Err(e) => return ToolResult::err(format!("Failed to list: {e}"), format!("{e}")),
            };

            let mut result = String::new();
            let mut count = 0;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let file_type = entry.file_type().ok();
                let kind = match file_type {
                    Some(ft) if ft.is_dir() => "📁",
                    Some(ft) if ft.is_symlink() => "🔗",
                    _ => "📄",
                };
                result.push_str(&format!("{kind} {name}\n"));
                count += 1;
            }

            ToolResult::ok_with_data(
                format!("{full_path} ({count} entries):\n{result}", full_path = full_path.display()),
                serde_json::json!({"entry_count": count}),
            )
        }),
    }
}
