use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::process::Command;
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
#[cfg(not(target_os = "macos"))]
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
#[cfg(not(target_os = "macos"))]
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

#[cfg(target_os = "macos")]
pub fn paste_image_to_temp_png() -> Result<(PathBuf, PastedImageInfo), PasteImageError> {
    let tmp = Builder::new()
        .prefix("conduit-clipboard-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| PasteImageError::IoError(e.to_string()))?;

    // Get the path but don't persist yet - we'll keep the file only on success
    let path = tmp.path().to_path_buf();
    let path_str = path.to_string_lossy().replace('"', "\\\"");

    let script = format!(
        r#"set theFile to POSIX file "{path_str}"
try
  set pngData to the clipboard as «class PNGf»
  set fRef to open for access theFile with write permission
  set eof fRef to 0
  write pngData to fRef
  close access fRef
on error
  try
    close access theFile
  end try
  error "no image"
end try"#
    );

    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| PasteImageError::ClipboardUnavailable(e.to_string()))?;

    if !output.status.success() {
        // tmp will be auto-deleted when dropped since we haven't called keep()
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Distinguish "no image" from actual script errors for better debugging
        if stderr.is_empty() || stderr.to_lowercase().contains("no image") {
            return Err(PasteImageError::NoImage("no image on clipboard".to_string()));
        } else {
            return Err(PasteImageError::NoImage(format!(
                "clipboard error: {}",
                stderr.trim()
            )));
        }
    }

    // Success - now persist the file
    let (_file, path) = tmp
        .keep()
        .map_err(|e| PasteImageError::IoError(e.error.to_string()))?;

    let (width, height) =
        image::image_dimensions(&path).map_err(|e| PasteImageError::EncodeFailed(e.to_string()))?;

    Ok((path, PastedImageInfo { width, height }))
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

    // Handle file:// URLs with optional localhost
    // file:///path -> /path
    // file://localhost/path -> /path
    let path = if let Some(stripped) = trimmed.strip_prefix("file://localhost") {
        stripped
    } else if let Some(stripped) = trimmed.strip_prefix("file://") {
        stripped
    } else {
        trimmed
    };

    Some(PathBuf::from(percent_decode(path)))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded_bytes = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                decoded_bytes.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        decoded_bytes.push(bytes[i]);
        i += 1;
    }
    // Use from_utf8_lossy to handle multi-byte UTF-8 sequences correctly
    String::from_utf8_lossy(&decoded_bytes).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
