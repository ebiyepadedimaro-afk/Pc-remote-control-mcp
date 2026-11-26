//! Screen tools: screenshots and screen state.

use super::{arg_bool, arg_str, arg_u32, input_note, ToolOutput};
use crate::platform;
use crate::state::AppState;
use image::{DynamicImage, ImageEncoder};
use serde_json::json;
use std::sync::Arc;

pub struct ShotOpts {
    pub format: String,
    pub quality: u8,
    pub max_width: u32,
    pub max_height: u32,
    pub match_coordinates: bool,
}

pub fn shot_opts(state: &AppState, args: &serde_json::Value) -> ShotOpts {
    let cfg = state.config.get();
    let mut format = arg_str(args, "format").unwrap_or(&cfg.screenshot.format).to_ascii_lowercase();
    if format == "jpg" {
        format = "jpeg".into();
    }
    if format != "png" && format != "jpeg" {
        format = "png".into();
    }
    let native_scale = arg_bool(args, "native_scale", false);
    ShotOpts {
        format,
        quality: arg_u32(args, "quality").map(|q| q.clamp(1, 100) as u8).unwrap_or(cfg.screenshot.jpeg_quality),
        max_width: arg_u32(args, "max_width").unwrap_or(cfg.screenshot.max_width),
        max_height: arg_u32(args, "max_height").unwrap_or(cfg.screenshot.max_height),
        match_coordinates: cfg.screenshot.match_input_coordinates && !native_scale,
    }
}

/// Resizes the capture to session coordinates (HiDPI) and applies caps.
pub fn prepare(captured: platform::Captured, opts: &ShotOpts) -> (DynamicImage, serde_json::Value) {
    let native_w = captured.width;
    let native_h = captured.height;
    let mut scale = captured.scale;
    let mut img = captured.image;
    if opts.match_coordinates && scale > 1.01 {
        let w = ((native_w as f32 / scale).round() as u32).max(1);
        let h = ((native_h as f32 / scale).round() as u32).max(1);
        img = img.resize(w, h, image::imageops::FilterType::Triangle);
    } else if !opts.match_coordinates {
        scale = 1.0;
    }
    let (mut w, mut h) = (img.width(), img.height());
    if (opts.max_width > 0 && w > opts.max_width) || (opts.max_height > 0 && h > opts.max_height) {
        let sx = if opts.max_width > 0 { opts.max_width as f32 / w as f32 } else { 1.0 };
        let sy = if opts.max_height > 0 { opts.max_height as f32 / h as f32 } else { 1.0 };
        let s = sx.min(sy).min(1.0);
        w = ((w as f32 * s).round() as u32).max(1);
        h = ((h as f32 * s).round() as u32).max(1);
        img = img.resize(w, h, image::imageops::FilterType::Triangle);
    }
    let meta = json!({
        "width": img.width(),
        "height": img.height(),
        "native_width": native_w,
        "native_height": native_h,
        "scale": scale,
        "backend": captured.backend,
    });
    (img, meta)
}

pub fn encode(img: &DynamicImage, opts: &ShotOpts) -> Result<(String, Vec<u8>), String> {
    let mut buf = Vec::new();
    if opts.format == "jpeg" {
        let rgb = img.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, opts.quality)
            .write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
            .map_err(|e| format!("JPEG encoding failed: {e}"))?;
        Ok(("image/jpeg".into(), buf))
    } else {
        let rgba = img.to_rgba8();
        image::codecs::png::PngEncoder::new(&mut buf)
            .write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
            .map_err(|e| format!("PNG encoding failed: {e}"))?;
        Ok(("image/png".into(), buf))
    }
}

pub async fn screenshot(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let opts = shot_opts(&state, &args);
    let captured = platform::screenshot().await?;
    let (img, meta) = prepare(captured, &opts);
    let (mime, bytes) = encode(&img, &opts)?;
    let mut summary = meta;
    summary["format"] = json!(opts.format);
    summary["bytes"] = json!(bytes.len());
    if let Some(dest) = arg_str(&args, "save_to") {
        if !dest.is_empty() {
            let path = super::expand_path(dest);
            let p = std::path::PathBuf::from(&path);
            if !p.is_absolute() {
                return Err("save_to must be an absolute path".into());
            }
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {parent:?}: {e}"))?;
            }
            std::fs::write(&p, &bytes).map_err(|e| format!("cannot write {path}: {e}"))?;
            summary["saved_to"] = json!(path);
        }
    }
    Ok(ToolOutput::image(&mime, &bytes).with_text(summary.to_string()))
}

pub async fn get_screen_state(state: Arc<AppState>, args: serde_json::Value) -> Result<ToolOutput, String> {
    let include_img = arg_bool(&args, "include_screenshot", true);
    let include_windows = arg_bool(&args, "include_windows", false);
    let opts = shot_opts(&state, &args);

    let mut state_json = json!({
        "session": platform::session_info(),
        "displays": platform::displays(),
        "cursor": platform::cursor_position().map(|(x, y)| json!({"x": x, "y": y})),
        "input": {
            "backend": platform::backends_report().get("input").cloned().unwrap_or(json!("unknown")),
            "warning": input_note(&state),
        },
    });

    let mut image: Option<(String, Vec<u8>)> = None;
    match platform::screenshot().await {
        Ok(captured) => {
            let (img, meta) = prepare(captured, &opts);
            if include_img {
                let (mime, bytes) = encode(&img, &opts)?;
                image = Some((mime, bytes));
            }
            state_json["screenshot"] = meta;
        }
        Err(e) => {
            state_json["screenshot_error"] = json!(e);
        }
    }

    if include_windows {
        match platform::list_windows() {
            Ok(mut windows) => {
                windows.truncate(200);
                state_json["windows"] = json!(windows);
                if platform::session_info().get("wayland").and_then(|w| w.as_bool()).unwrap_or(false) {
                    state_json["windows_note"] =
                        json!("On Wayland only XWayland windows are visible to the X11 window list.");
                }
            }
            Err(e) => state_json["windows_error"] = json!(e),
        }
    }

    let mut out = ToolOutput::text(state_json.to_string());
    if let Some((mime, bytes)) = image {
        out.content.push(super::Content::Image {
            data: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes),
            mime_type: mime,
        });
    }
    Ok(out)
}
