//! Window management tools.
//!
//! Uses external commands: `wmctrl` on Linux/X11, `osascript` on macOS,
//! `powershell` on Windows.

use crate::protocol::ContentBlock;
use serde_json::{json, Value};
use std::process::Command;

/// List open windows.
/// Linux: uses `wmctrl -l` (X11 only, Wayland not supported).
pub fn window_list(_args: Value) -> Result<Vec<ContentBlock>, String> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("wmctrl")
            .arg("-l")
            .output()
            .map_err(|e| format!("wmctrl not found: {e}. Install with: sudo apt install wmctrl"))?;

        if !output.status.success() {
            return Err(format!(
                "wmctrl failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let windows: Vec<Value> = text
            .lines()
            .map(|line| {
                let parts: Vec<&str> = line.splitn(5, ' ').collect();
                json!({
                    "id": parts.first().unwrap_or(&"").to_string(),
                    "desktop": parts.get(1).unwrap_or(&"").to_string(),
                    "title": parts.get(4).unwrap_or(&"").to_string(),
                })
            })
            .collect();

        Ok(vec![ContentBlock::Text {
            text: serde_json::to_string_pretty(&json!({"windows": windows}))
                .unwrap_or_else(|_| text),
        }])
    }
    #[cfg(target_os = "macos")]
    {
        let script = r#"tell application "System Events" to get name of every process whose background only is false"#;
        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| format!("osascript failed: {e}"))?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let apps: Vec<&str> = text.split(", ").collect();
        Ok(vec![ContentBlock::Text {
            text: serde_json::to_string_pretty(&json!({"applications": apps}))
                .unwrap_or_else(|_| text),
        }])
    }
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args(["-Command", "Get-Process | Where-Object {$_.MainWindowTitle} | Select-Object Id, MainWindowTitle | ConvertTo-Json"])
            .output()
            .map_err(|e| format!("powershell failed: {e}"))?;

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(vec![ContentBlock::Text { text }])
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err("window_list not supported on this OS".to_string())
    }
}

/// Focus a window by title (substring match).
/// Linux: uses `wmctrl -a`.
pub fn window_focus(args: Value) -> Result<Vec<ContentBlock>, String> {
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'title' parameter")?;

    #[cfg(target_os = "linux")]
    {
        let output = Command::new("wmctrl")
            .arg("-a")
            .arg(title)
            .output()
            .map_err(|e| format!("wmctrl not found: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "wmctrl focus failed: {}. Window '{}' not found or wmctrl not installed.",
                String::from_utf8_lossy(&output.stderr),
                title,
            ));
        }

        Ok(vec![ContentBlock::Text {
            text: format!("Focused window: {title}"),
        }])
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"tell application "System Events" to set frontmost of process "{}" to true"#,
            title
        );
        Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| format!("osascript: {e}"))?;

        Ok(vec![ContentBlock::Text {
            text: format!("Focused: {title}"),
        }])
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err("window_focus: currently Linux (wmctrl) and macOS only.".to_string())
    }
}

/// Get screen dimensions.
pub fn get_screen_size(_args: Value) -> Result<Vec<ContentBlock>, String> {
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("xrandr")
            .output()
            .map_err(|e| format!("xrandr not found: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(vec![ContentBlock::Text {
            text: format!("Screen info (xrandr):\n{text}"),
        }])
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("system_profiler")
            .args(["SPDisplaysDataType"])
            .output()
            .map_err(|e| format!("system_profiler: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(vec![ContentBlock::Text { text }])
    }
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args(["-Command", "Get-WmiObject win32_videocontroller | Select-Object Name, CurrentHorizontalResolution, CurrentVerticalResolution | ConvertTo-Json"])
            .output()
            .map_err(|e| format!("powershell: {e}"))?;
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(vec![ContentBlock::Text { text }])
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err("get_screen_size not supported on this OS".to_string())
    }
}
