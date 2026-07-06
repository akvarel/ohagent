//! Screenshot tool — captures the primary monitor and returns a base64 PNG.
//!
//! Linux: uses `import` (ImageMagick) or `maim`.
//! macOS: uses `screencapture`.
//! Windows: uses PowerShell.

use crate::protocol::ContentBlock;
use base64::Engine;
use serde_json::Value;
use std::io::Cursor;
use std::process::Command;

/// Take a screenshot. Returns base64 PNG.
/// Params: `{ "monitor": 0, "x": 0, "y": 0, "width": 1920, "height": 1080 }`
pub fn screenshot(args: Value) -> Result<Vec<ContentBlock>, String> {
    let png_bytes = capture_screenshot(&args)?;

    // If crop region specified, crop the image
    let final_bytes = if let (Some(x), Some(y), Some(w), Some(h)) = (
        args.get("x").and_then(|v| v.as_u64()),
        args.get("y").and_then(|v| v.as_u64()),
        args.get("width").and_then(|v| v.as_u64()),
        args.get("height").and_then(|v| v.as_u64()),
    ) {
        let img = image::load_from_memory(&png_bytes)
            .map_err(|e| format!("Failed to decode PNG: {e}"))?;
        let rgba = img.to_rgba8();
        let cropped = image::imageops::crop_imm(
            &rgba,
            x as u32,
            y as u32,
            w as u32,
            h as u32,
        );
        let mut buf = Cursor::new(Vec::new());
        cropped
            .to_image()
            .write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| format!("Failed to encode cropped PNG: {e}"))?;
        buf.into_inner()
    } else {
        png_bytes
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&final_bytes);

    // Get dimensions
    let (w, h) = if let Ok(img) = image::load_from_memory(&final_bytes) {
        (img.width(), img.height())
    } else {
        (0, 0)
    };

    Ok(vec![
        ContentBlock::Image {
            data: b64,
            mime_type: "image/png".to_string(),
        },
        ContentBlock::Text {
            text: format!("Screenshot: {w}x{h} PNG"),
        },
    ])
}

fn capture_screenshot(args: &Value) -> Result<Vec<u8>, String> {
    let monitor_index = args.get("monitor").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    #[cfg(target_os = "linux")]
    {
        // Try maim first (better multi-monitor), then import (ImageMagick)
        if monitor_index > 0 {
            // maim supports --display and region selection
            match Command::new("maim")
                .arg("-u") // hide cursor
                .output()
            {
                Ok(out) if out.status.success() => return Ok(out.stdout),
                _ => {}
            }
        }

        // Fallback: import from ImageMagick
        match Command::new("import")
            .arg("-window")
            .arg("root")
            .arg("-silent")
            .arg("png:-")
            .output()
        {
            Ok(out) if out.status.success() => Ok(out.stdout),
            Ok(out) => Err(format!(
                "import failed: {}. Install ImageMagick: sudo apt install imagemagick",
                String::from_utf8_lossy(&out.stderr)
            )),
            Err(_) => Err(
                "No screenshot tool found. Install maim or imagemagick:\n  sudo apt install maim imagemagick"
                    .to_string(),
            ),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = monitor_index;
        let output = Command::new("screencapture")
            .arg("-x") // no sound
            .arg("-t")
            .arg("png")
            .arg("-") // stdout
            .output()
            .map_err(|e| format!("screencapture not found: {e}"))?;

        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(format!(
                "screencapture failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = monitor_index;
        // PowerShell: capture screen to memory, output as PNG bytes
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$screen = [System.Windows.Forms.Screen]::PrimaryScreen
$bmp = New-Object System.Drawing.Bitmap($screen.Bounds.Width, $screen.Bounds.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($screen.Bounds.X, $screen.Bounds.Y, 0, 0, $bmp.Size)
$ms = New-Object System.IO.MemoryStream
$bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
[Console]::OpenStandardOutput().Write($ms.ToArray(), 0, $ms.Length)
$g.Dispose(); $bmp.Dispose(); $ms.Dispose()
"#;
        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output()
            .map_err(|e| format!("powershell: {e}"))?;

        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(format!(
                "PowerShell screenshot failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err("Screenshot not supported on this OS".to_string())
    }
}
