//! Keyboard control tools via enigo.

use crate::protocol::ContentBlock;
use enigo::{Enigo, Keyboard, Settings};
use serde_json::Value;

fn mk_enigo() -> Result<Enigo, String> {
    Enigo::new(&Settings::default()).map_err(|e| format!("Failed to init enigo: {e}"))
}

/// Type a text string (simulates keystrokes, not paste).
/// Params: `{ "text": "Hello world" }`
pub fn keyboard_type(args: Value) -> Result<Vec<ContentBlock>, String> {
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'text' parameter")?;

    let mut enigo = mk_enigo()?;
    enigo
        .text(text)
        .map_err(|e| format!("keyboard text: {e}"))?;

    Ok(vec![ContentBlock::Text {
        text: format!("Typed: {text}"),
    }])
}

/// Parse a key string like "Return", "Tab", "a" into an enigo Key.
fn parse_key(s: &str) -> Result<enigo::Key, String> {
    use enigo::Key;
    match s.to_lowercase().as_str() {
        "return" | "enter" => Ok(Key::Return),
        "tab" => Ok(Key::Tab),
        "space" => Ok(Key::Space),
        "backspace" => Ok(Key::Backspace),
        "delete" => Ok(Key::Delete),
        "escape" | "esc" => Ok(Key::Escape),
        "up" => Ok(Key::UpArrow),
        "down" => Ok(Key::DownArrow),
        "left" => Ok(Key::LeftArrow),
        "right" => Ok(Key::RightArrow),
        "home" => Ok(Key::Home),
        "end" => Ok(Key::End),
        "pageup" => Ok(Key::PageUp),
        "pagedown" => Ok(Key::PageDown),
        "capslock" => Ok(Key::CapsLock),
        "f1" => Ok(Key::F1),
        "f2" => Ok(Key::F2),
        "f3" => Ok(Key::F3),
        "f4" => Ok(Key::F4),
        "f5" => Ok(Key::F5),
        "f6" => Ok(Key::F6),
        "f7" => Ok(Key::F7),
        "f8" => Ok(Key::F8),
        "f9" => Ok(Key::F9),
        "f10" => Ok(Key::F10),
        "f11" => Ok(Key::F11),
        "f12" => Ok(Key::F12),
        "control" | "ctrl" => Ok(Key::Control),
        "alt" => Ok(Key::Alt),
        "shift" => Ok(Key::Shift),
        "meta" | "super" | "windows" | "command" | "cmd" => Ok(Key::Meta),
        c if c.len() == 1 => Ok(Key::Unicode(c.chars().next().unwrap())),
        _ => Err(format!("Unknown key: {s}. Use standard key names (return, tab, escape, f1-f12, etc.) or single characters.")),
    }
}

/// Press a key combination.
/// Params: `{ "keys": ["ctrl", "c"] }` for Ctrl+C.
/// First keys in the list are held down, last is pressed.
pub fn keyboard_press(args: Value) -> Result<Vec<ContentBlock>, String> {
    let keys: Vec<String> = args
        .get("keys")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .ok_or("Missing 'keys' array")?;

    if keys.is_empty() {
        return Err("'keys' array is empty".to_string());
    }

    let parsed: Vec<enigo::Key> = keys
        .iter()
        .map(|k| parse_key(k))
        .collect::<Result<_, _>>()?;

    let mut enigo = mk_enigo()?;

    // Hold all modifiers
    for key in &parsed {
        enigo
            .key(*key, enigo::Direction::Press)
            .map_err(|e| format!("key press: {e}"))?;
    }
    // Release in reverse order
    for key in parsed.iter().rev() {
        enigo
            .key(*key, enigo::Direction::Release)
            .map_err(|e| format!("key release: {e}"))?;
    }

    let combo = keys.join("+");
    Ok(vec![ContentBlock::Text {
        text: format!("Pressed: {combo}"),
    }])
}
