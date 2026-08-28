//! Accessibility tree tool (Linux via AT-SPI).
//!
//! Uses Python3 + AT-SPI dbus bindings to dump the accessibility tree.
//! Falls back gracefully on non-Linux platforms.

use crate::protocol::ContentBlock;
use serde_json::Value;
use std::process::Command;

/// Python script to dump AT-SPI accessibility tree.
const ATSPI_SCRIPT: &str = r#"
import sys, json
try:
    import gi
    gi.require_version('Atspi', '2.0')
    from gi.repository import Atspi
except ImportError:
    print(json.dumps({"error": "python3-atspi not installed. Run: pip install pyatspi"}))
    sys.exit(0)

def collect(node, depth, max_depth, app_filter):
    try:
        name = node.get_name() or ''
        role = node.get_role_name() or 'unknown'
    except Exception:
        return None

    if app_filter and app_filter.lower() not in name.lower() and app_filter.lower() not in role.lower():
        return None

    desc = ''
    try:
        desc = node.get_description() or ''
    except Exception:
        pass

    pos = None
    try:
        extents = node.get_extents(Atspi.CoordType.SCREEN)
        if extents:
            pos = {"x": extents.x, "y": extents.y, "width": extents.width, "height": extents.height}
    except Exception:
        pass

    result = {"name": name, "role": role, "description": desc}
    if pos:
        result["position"] = pos

    if depth < max_depth:
        try:
            children = []
            for i in range(node.get_child_count()):
                child = node.get_child_at_index(i)
                c = collect(child, depth + 1, max_depth, app_filter)
                if c:
                    children.append(c)
            if children:
                result["children"] = children
        except Exception:
            pass

    return result

max_depth = int(sys.argv[1]) if len(sys.argv) > 1 else 3
app_filter = sys.argv[2] if len(sys.argv) > 2 else None

try:
    desktop = Atspi.get_desktop(0)
    roots = []
    for i in range(desktop.get_child_count()):
        child = desktop.get_child_at_index(i)
        node = collect(child, 0, max_depth, app_filter)
        if node:
            roots.append(node)
    print(json.dumps(roots))
except Exception as e:
    print(json.dumps({"error": str(e)}))
"#;

/// Dump the accessibility tree. Linux-only (AT-SPI via Python3).
/// Params: `{ "max_depth": 3, "app_name": "firefox" }` (optional filters)
pub fn accessibility_tree(args: Value) -> Result<Vec<ContentBlock>, String> {
    let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3);
    let app_filter = args.get("app_name").and_then(|v| v.as_str()).unwrap_or("");

    let output = Command::new("python3")
        .arg("-c")
        .arg(ATSPI_SCRIPT)
        .arg(max_depth.to_string())
        .arg(app_filter)
        .output();

    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let err_text = String::from_utf8_lossy(&out.stderr).trim().to_string();

            if !err_text.is_empty() {
                return Ok(vec![ContentBlock::Text {
                    text: format!("AT-SPI stderr:\n{err_text}\n\nstdout:\n{text}"),
                }]);
            }

            // Try to parse as JSON, fall back to raw text
            match serde_json::from_str::<Value>(&text) {
                Ok(v) => {
                    if let Some(error) = v.get("error") {
                        return Ok(vec![ContentBlock::Text {
                            text: format!(
                                "AT-SPI error: {}\n\nInstall dependencies:\n  pip install pyatspi\n  sudo apt install python3-gi gir1.2-atspi-2.0",
                                error.as_str().unwrap_or(&text),
                            ),
                        }]);
                    }
                    Ok(vec![ContentBlock::Text {
                        text: format!(
                            "Accessibility tree (max_depth={max_depth}):\n{}",
                            serde_json::to_string_pretty(&v).unwrap_or(text)
                        ),
                    }])
                }
                Err(_) => Ok(vec![ContentBlock::Text {
                    text: format!("AT-SPI output (max_depth={max_depth}):\n{text}"),
                }]),
            }
        }
        Err(e) => {
            // Python3 or AT-SPI not available
            Ok(vec![ContentBlock::Text {
                text: format!(
                    "Accessibility tree not available.\n\
                     Error: {e}\n\n\
                     Install dependencies:\n\
                       pip install pyatspi\n\
                       sudo apt install python3-gi gir1.2-atspi-2.0\n\
                     Or use the 'screenshot' tool for visual inspection."
                ),
            }])
        }
    }
}
