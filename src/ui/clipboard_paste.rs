use std::path::PathBuf;

use tempfile::Builder;

#[derive(Debug, Clone)]
pub enum PasteImageError {
    ClipboardUnavailable(String),
    NoImage(String),
    EncodeFailed(String),
    IoError(String),
}

impl std::fmt::Display for PasteImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasteImageError::ClipboardUnavailable(msg) => write!(f, "clipboard unavailable: {msg}"),
            PasteImageError::NoImage(msg) => write!(f, "no image on clipboard: {msg}"),
            PasteImageError::EncodeFailed(msg) => write!(f, "could not encode image: {msg}"),
            PasteImageError::IoError(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for PasteImageError {}

#[derive(Debug, Clone)]
pub struct PastedImageInfo {
    pub width: u32,
    pub height: u32,
}

/// Capture image from system clipboard, encode to PNG, and return bytes + info.
pub fn paste_image_as_png() -> Result<(Vec<u8>, PastedImageInfo), PasteImageError> {
    let mut cb = arboard::Clipboard::new()
        .map_err(|e| PasteImageError::ClipboardUnavailable(e.to_string()))?;
    let img = cb
        .get_image()
        .map_err(|e| PasteImageError::NoImage(e.to_string()))?;

    let width = img.width as u32;
    let height = img.height as u32;
    let bytes = img.bytes.into_owned();

    let Some(rgba_img) = image::RgbaImage::from_raw(width, height, bytes) else {
        return Err(PasteImageError::EncodeFailed(
            "invalid RGBA buffer".to_string(),
        ));
    };

    let dyn_img = image::DynamicImage::ImageRgba8(rgba_img);
    let mut png: Vec<u8> = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png);
        dyn_img
            .write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| PasteImageError::EncodeFailed(e.to_string()))?;
    }

    Ok((png, PastedImageInfo { width, height }))
}

/// Convenience: write to a temp file and return its path + info.
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let (png, info) = paste_image_as_png()?;
    let tmp = Builder::new()
        .prefix("conduit-clipboard-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| PasteImageError::IoError(e.to_string()))?;
    std::fs::write(tmp.path(), &png).map_err(|e| PasteImageError::IoError(e.to_string()))?;
    let (_file, path) = tmp
        .keep()
        .map_err(|e| PasteImageError::IoError(e.error.to_string()))?;
    Ok((path, info))
}

/// Normalize pasted text that may represent a filesystem path.
pub fn normalize_pasted_path(pasted: &str) -> Option<PathBuf> {
    let trimmed = pasted.trim();
    if trimmed.is_empty() {
        return None;
    }

    let trimmed = trimmed.trim_matches('"').trim_matches('\'').trim();

    if trimmed.is_empty() {
        return None;
    }

    let mut path = if let Some(stripped) = trimmed.strip_prefix("file://") {
        stripped
    } else {
        trimmed
    };

    if let Some(stripped) = path.strip_prefix("localhost/") {
        path = stripped;
    }

    Some(PathBuf::from(percent_decode(path)))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push(char::from(h * 16 + l));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
