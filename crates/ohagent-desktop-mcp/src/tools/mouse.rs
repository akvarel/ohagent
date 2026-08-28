//! Mouse control tools via enigo.

use crate::protocol::ContentBlock;
use enigo::{Coordinate, Direction, Enigo, Mouse, Settings};
use serde_json::Value;

fn mk_enigo() -> Result<Enigo, String> {
    Enigo::new(&Settings::default()).map_err(|e| format!("Failed to init enigo: {e}"))
}

/// Move mouse to absolute coordinates.
/// Params: `{ "x": 500, "y": 300 }`
pub fn mouse_move(args: Value) -> Result<Vec<ContentBlock>, String> {
    let x = args
        .get("x")
        .and_then(|v| v.as_i64())
        .ok_or("Missing 'x'")? as i32;
    let y = args
        .get("y")
        .and_then(|v| v.as_i64())
        .ok_or("Missing 'y'")? as i32;

    let mut enigo = mk_enigo()?;
    enigo
        .move_mouse(x, y, Coordinate::Abs)
        .map_err(|e| format!("move_mouse failed: {e}"))?;

    Ok(vec![ContentBlock::Text {
        text: format!("Mouse moved to ({x}, {y})"),
    }])
}

/// Click a mouse button, optionally at coordinates.
/// Params: `{ "button": "left", "x": 500, "y": 300 }`
/// Default button: left. If x/y given, moves first then clicks.
pub fn mouse_click(args: Value) -> Result<Vec<ContentBlock>, String> {
    let button_str = args
        .get("button")
        .and_then(|v| v.as_str())
        .unwrap_or("left");
    let button = match button_str {
        "left" => enigo::Button::Left,
        "right" => enigo::Button::Right,
        "middle" => enigo::Button::Middle,
        _ => {
            return Err(format!(
                "Unknown button: {button_str}. Use left/right/middle."
            ))
        }
    };

    let mut enigo = mk_enigo()?;

    if let (Some(x), Some(y)) = (
        args.get("x").and_then(|v| v.as_i64()),
        args.get("y").and_then(|v| v.as_i64()),
    ) {
        enigo
            .move_mouse(x as i32, y as i32, Coordinate::Abs)
            .map_err(|e| format!("move_mouse: {e}"))?;
    }

    enigo
        .button(button, Direction::Click)
        .map_err(|e| format!("button click: {e}"))?;

    Ok(vec![ContentBlock::Text {
        text: format!("Clicked {button_str}"),
    }])
}

/// Drag from (x1, y1) to (x2, y2).
/// Params: `{ "x1": 100, "y1": 100, "x2": 400, "y2": 400 }`
pub fn mouse_drag(args: Value) -> Result<Vec<ContentBlock>, String> {
    let x1 = args
        .get("x1")
        .and_then(|v| v.as_i64())
        .ok_or("Missing 'x1'")? as i32;
    let y1 = args
        .get("y1")
        .and_then(|v| v.as_i64())
        .ok_or("Missing 'y1'")? as i32;
    let x2 = args
        .get("x2")
        .and_then(|v| v.as_i64())
        .ok_or("Missing 'x2'")? as i32;
    let y2 = args
        .get("y2")
        .and_then(|v| v.as_i64())
        .ok_or("Missing 'y2'")? as i32;

    let mut enigo = mk_enigo()?;

    enigo
        .move_mouse(x1, y1, Coordinate::Abs)
        .map_err(|e| format!("move to start: {e}"))?;
    enigo
        .button(enigo::Button::Left, Direction::Press)
        .map_err(|e| format!("press: {e}"))?;
    enigo
        .move_mouse(x2, y2, Coordinate::Abs)
        .map_err(|e| format!("move to end: {e}"))?;
    enigo
        .button(enigo::Button::Left, Direction::Release)
        .map_err(|e| format!("release: {e}"))?;

    Ok(vec![ContentBlock::Text {
        text: format!("Dragged from ({x1},{y1}) to ({x2},{y2})"),
    }])
}
