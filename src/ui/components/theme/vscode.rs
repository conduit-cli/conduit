//! VS Code theme parser and mapper.
//!
//! Parses VS Code theme JSON files and maps their color keys to our
//! 51 semantic TUI colors using fallback chains and derivation.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use ratatui::style::Color;
use serde::Deserialize;

use super::colors::{
    boost_brightness, darken, desaturate, dim, interpolate, lighten, parse_hex_color, shift_hue,
};
use super::types::Theme;

/// VS Code theme JSON structure.
#[derive(Debug, Deserialize)]
pub struct VsCodeTheme {
    /// Theme name
    pub name: Option<String>,

    /// Theme type: "dark", "light", "hc", "hcLight"
    #[serde(rename = "type")]
    pub theme_type: Option<String>,

    /// Workbench color definitions
    #[serde(default)]
    pub colors: HashMap<String, String>,
    // tokenColors intentionally omitted to avoid parsing a large, unused payload.
}

impl VsCodeTheme {
    /// Load a VS Code theme from a JSON file.
    pub fn load_from_file(path: &Path) -> Result<Self, VsCodeThemeError> {
        let read_start = Instant::now();
        let content = fs::read_to_string(path).map_err(VsCodeThemeError::Io)?;
        let read_ms = read_start.elapsed().as_millis();
        let size = content.len();
        let parse_start = Instant::now();
        let theme = Self::load_from_str(&content)?;
        let parse_ms = parse_start.elapsed().as_millis();
        tracing::info!(
            path = %path.display(),
            bytes = size,
            read_ms,
            parse_ms,
            "Loaded VS Code theme file"
        );
        Ok(theme)
    }

    /// Load a VS Code theme from a JSON string.
    pub fn load_from_str(content: &str) -> Result<Self, VsCodeThemeError> {
        // VS Code themes may have comments and trailing commas
        // Try strict JSON first, then fall back to JSON5 for JSONC-style files.
        match serde_json::from_str(content) {
            Ok(theme) => Ok(theme),
            Err(json_err) => {
                tracing::debug!(error = %json_err, "Strict JSON parse failed, retrying with JSON5");
                match json5::from_str(content) {
                    Ok(theme) => {
                        tracing::debug!("Parsed theme with JSON5 fallback");
                        Ok(theme)
                    }
                    Err(json5_err) => Err(VsCodeThemeError::Parse {
                        json: json_err,
                        json5: json5_err,
                    }),
                }
            }
        }
    }

    /// Convert this VS Code theme to our Theme format.
    pub fn to_theme(&self) -> Theme {
        let mapper = VsCodeMapper::new(self);
        mapper.build_theme()
    }

    /// Check if this is a light theme.
    pub fn is_light(&self) -> bool {
        matches!(self.theme_type.as_deref(), Some("light") | Some("hcLight"))
    }
}

/// Error types for VS Code theme loading.
#[derive(Debug)]
pub enum VsCodeThemeError {
    Io(std::io::Error),
    Parse {
        json: serde_json::Error,
        json5: json5::Error,
    },
}

impl std::fmt::Display for VsCodeThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VsCodeThemeError::Io(e) => write!(f, "IO error: {}", e),
            VsCodeThemeError::Parse { json, json5 } => write!(
                f,
                "JSON parse failed: {}. JSON5 parse failed: {}",
                json, json5
            ),
        }
    }
}

impl std::error::Error for VsCodeThemeError {}

/// Maps VS Code theme colors to our semantic TUI colors.
struct VsCodeMapper<'a> {
    vscode: &'a VsCodeTheme,
    /// Cached parsed colors
    colors: HashMap<&'a str, Color>,
}

impl<'a> VsCodeMapper<'a> {
    fn new(vscode: &'a VsCodeTheme) -> Self {
        let mut colors = HashMap::new();

        // Pre-parse all colors
        for (key, value) in &vscode.colors {
            if let Some(color) = parse_hex_color(value) {
                colors.insert(key.as_str(), color);
            }
        }

        Self { vscode, colors }
    }

    /// Get a color by VS Code key.
    fn get(&self, key: &str) -> Option<Color> {
        self.colors.get(key).copied()
    }

    /// Get a color with fallback chain.
    fn get_with_fallback(&self, keys: &[&str]) -> Option<Color> {
        for key in keys {
            if let Some(color) = self.get(key) {
                return Some(color);
            }
        }
        None
    }

    /// Build the complete Theme from VS Code colors.
    fn build_theme(&self) -> Theme {
        let name = self
            .vscode
            .name
            .clone()
            .unwrap_or_else(|| "VS Code Theme".to_string());
        let is_light = self.vscode.is_light();

        // =====================================================================
        // Background Layers
        // =====================================================================
        let bg_base = self
            .get_with_fallback(&["editor.background", "sideBar.background"])
            .unwrap_or({
                if is_light {
                    Color::Rgb(250, 250, 252)
                } else {
                    Color::Rgb(22, 22, 30)
                }
            });

        let bg_terminal = self
            .get_with_fallback(&["terminal.background", "editor.background"])
            .unwrap_or_else(|| darken(bg_base, 0.15));

        let bg_surface = self
            .get_with_fallback(&["sideBar.background", "panel.background"])
            .unwrap_or_else(|| {
                if is_light {
                    darken(bg_base, 0.04)
                } else {
                    lighten(bg_base, 0.10)
                }
            });

        let bg_elevated = self
            .get_with_fallback(&["editorWidget.background", "dropdown.background"])
            .unwrap_or_else(|| {
                if is_light {
                    darken(bg_surface, 0.04)
                } else {
                    lighten(bg_surface, 0.12)
                }
            });

        let bg_highlight = self
            .get_with_fallback(&[
                "editor.selectionBackground",
                "list.activeSelectionBackground",
            ])
            .unwrap_or_else(|| {
                if is_light {
                    darken(bg_elevated, 0.08)
                } else {
                    lighten(bg_elevated, 0.10)
                }
            });

        let markdown_code_bg = self
            .get_with_fallback(&[
                "textBlockQuote.background",
                "editor.lineHighlightBackground",
            ])
            .unwrap_or_else(|| {
                if is_light {
                    darken(bg_base, 0.03)
                } else {
                    darken(bg_base, 0.05)
                }
            });

        let markdown_inline_code_bg = self
            .get_with_fallback(&["textCodeBlock.background", "badge.background"])
            .unwrap_or_else(|| {
                if is_light {
                    darken(markdown_code_bg, 0.04)
                } else {
                    lighten(markdown_code_bg, 0.08)
                }
            });

        // =====================================================================
        // Text Hierarchy
        // =====================================================================
        let text_primary = self
            .get_with_fallback(&["editor.foreground", "foreground"])
            .unwrap_or({
                if is_light {
                    Color::Rgb(35, 35, 45)
                } else {
                    Color::Rgb(220, 220, 230)
                }
            });

        let text_bright = self
            .get("editor.foreground")
            .map(|c| boost_brightness(c, 1.15))
            .unwrap_or_else(|| {
                if is_light {
                    Color::Rgb(15, 15, 20)
                } else {
                    Color::Rgb(250, 250, 255)
                }
            });

        let text_secondary = self
            .get_with_fallback(&["descriptionForeground", "sideBar.foreground"])
            .unwrap_or_else(|| {
                if is_light {
                    lighten(text_primary, 0.35)
                } else {
                    dim(text_primary, 0.75)
                }
            });

        let text_muted = self
            .get_with_fallback(&["input.placeholderForeground", "editorLineNumber.foreground"])
            .unwrap_or_else(|| {
                if is_light {
                    lighten(text_primary, 0.55)
                } else {
                    dim(text_primary, 0.45)
                }
            });

        let text_faint = self
            .get_with_fallback(&["editorIndentGuide.background", "tree.indentGuidesStroke"])
            .unwrap_or_else(|| {
                if is_light {
                    lighten(text_primary, 0.70)
                } else {
                    dim(text_primary, 0.30)
                }
            });

        // =====================================================================
        // Accent Colors
        // =====================================================================
        let accent_primary = self
            .get_with_fallback(&["focusBorder", "button.background", "textLink.foreground"])
            .unwrap_or({
                if is_light {
                    Color::Rgb(60, 120, 220)
                } else {
                    Color::Rgb(130, 170, 255)
                }
            });

        let accent_secondary = self
            .get("activityBarBadge.background")
            .unwrap_or_else(|| shift_hue(accent_primary, 60.0));

        let accent_success = self
            .get_with_fallback(&[
                "editorGutter.addedBackground",
                "gitDecoration.addedResourceForeground",
                "terminal.ansiGreen",
            ])
            .unwrap_or({
                if is_light {
                    Color::Rgb(40, 160, 60)
                } else {
                    Color::Rgb(130, 200, 140)
                }
            });

        let accent_warning = self
            .get_with_fallback(&[
                "editorWarning.foreground",
                "gitDecoration.modifiedResourceForeground",
                "terminal.ansiYellow",
            ])
            .unwrap_or({
                if is_light {
                    Color::Rgb(200, 140, 30)
                } else {
                    Color::Rgb(230, 180, 100)
                }
            });

        let accent_error = self
            .get_with_fallback(&[
                "editorError.foreground",
                "errorForeground",
                "terminal.ansiRed",
            ])
            .unwrap_or({
                if is_light {
                    Color::Rgb(200, 60, 60)
                } else {
                    Color::Rgb(230, 120, 120)
                }
            });

        // =====================================================================
        // Agent Colors
        // =====================================================================
        let agent_claude = self
            .get("terminal.ansiCyan")
            .unwrap_or_else(|| desaturate(accent_primary, 0.20));

        let agent_codex = self
            .get("terminal.ansiMagenta")
            .unwrap_or_else(|| shift_hue(agent_claude, 60.0));

        // =====================================================================
        // PR State Colors
        // =====================================================================
        let pr_open_bg = self
            .get_with_fallback(&[
                "gitDecoration.untrackedResourceForeground",
                "terminal.ansiGreen",
            ])
            .unwrap_or(accent_success);

        let pr_merged_bg = self.get("terminal.ansiMagenta").unwrap_or(accent_secondary);

        let pr_closed_bg = self
            .get_with_fallback(&[
                "gitDecoration.deletedResourceForeground",
                "terminal.ansiRed",
            ])
            .unwrap_or(accent_error);

        let pr_draft_bg = self
            .get("gitDecoration.ignoredResourceForeground")
            .unwrap_or(text_muted);

        let pr_unknown_bg = self
            .get("badge.background")
            .unwrap_or_else(|| dim(bg_elevated, 0.90));

        // =====================================================================
        // Spinner Colors (derived from accent_primary)
        // =====================================================================
        let spinner_active = accent_primary;
        let spinner_trail_1 = dim(accent_primary, 0.85);
        let spinner_trail_2 = dim(accent_primary, 0.70);
        let spinner_trail_3 = dim(accent_primary, 0.55);
        let spinner_trail_4 = dim(accent_primary, 0.40);
        let spinner_trail_5 = dim(accent_primary, 0.28);
        let spinner_inactive = text_muted;

        // =====================================================================
        // Border Colors
        // =====================================================================
        let border_default = self
            .get_with_fallback(&["panel.border", "sideBar.border"])
            .unwrap_or_else(|| {
                if is_light {
                    darken(bg_base, 0.15)
                } else {
                    lighten(bg_base, 0.20)
                }
            });

        let border_focused = self.get("focusBorder").unwrap_or(accent_primary);

        let border_dimmed = self
            .get_with_fallback(&["editorGroup.border", "tab.border"])
            .unwrap_or_else(|| dim(border_default, 0.70));

        // =====================================================================
        // Logo Shine Colors (derived from text hierarchy)
        // =====================================================================
        let shine_edge = interpolate(text_muted, text_secondary, 0.30);
        let shine_mid = interpolate(text_secondary, text_primary, 0.50);
        let shine_center = interpolate(text_primary, text_bright, 0.70);
        let shine_peak = Color::Rgb(255, 255, 255);

        // =====================================================================
        // Tool Block Colors
        // =====================================================================
        let tool_block_bg = self
            .get("peekViewEditor.background")
            .unwrap_or_else(|| darken(bg_base, 0.05));

        let tool_comment = self
            .get("editorLineNumber.foreground")
            .unwrap_or(text_muted);

        let tool_command = self.get("terminal.foreground").unwrap_or(text_primary);

        let tool_output = self.get("descriptionForeground").unwrap_or(text_secondary);

        let diff_add = self
            .get_with_fallback(&[
                "diffEditor.insertedTextBackground",
                "editorGutter.addedBackground",
            ])
            .unwrap_or(accent_success);

        let diff_remove = self
            .get_with_fallback(&[
                "diffEditor.removedTextBackground",
                "editorGutter.deletedBackground",
            ])
            .unwrap_or(accent_error);

        Theme {
            name,
            is_light,

            // Background
            bg_terminal,
            bg_base,
            bg_surface,
            bg_elevated,
            bg_highlight,
            markdown_code_bg,
            markdown_inline_code_bg,

            // Text
            text_bright,
            text_primary,
            text_secondary,
            text_muted,
            text_faint,

            // Accent
            accent_primary,
            accent_secondary,
            accent_success,
            accent_warning,
            accent_error,

            // Agent
            agent_claude,
            agent_codex,

            // PR State
            pr_open_bg,
            pr_merged_bg,
            pr_closed_bg,
            pr_draft_bg,
            pr_unknown_bg,

            // Spinner
            spinner_active,
            spinner_trail_1,
            spinner_trail_2,
            spinner_trail_3,
            spinner_trail_4,
            spinner_trail_5,
            spinner_inactive,

            // Border
            border_default,
            border_focused,
            border_dimmed,

            // Shine
            shine_edge,
            shine_mid,
            shine_center,
            shine_peak,

            // Tool Block
            tool_block_bg,
            tool_comment,
            tool_command,
            tool_output,
            diff_add,
            diff_remove,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vscode_theme() {
        let json = r##"{
            "name": "Test Theme",
            "type": "dark",
            "colors": {
                "editor.background": "#1e1e2e",
                "editor.foreground": "#cdd6f4",
                "focusBorder": "#89b4fa"
            }
        }"##;

        let vscode = VsCodeTheme::load_from_str(json).unwrap();
        assert_eq!(vscode.name, Some("Test Theme".to_string()));
        assert_eq!(vscode.theme_type, Some("dark".to_string()));
        assert!(!vscode.is_light());

        let theme = vscode.to_theme();
        assert_eq!(theme.name, "Test Theme");
        assert!(!theme.is_light);
        assert_eq!(theme.bg_base, Color::Rgb(30, 30, 46));
    }

    #[test]
    fn test_light_theme() {
        let json = r##"{
            "name": "Light Theme",
            "type": "light",
            "colors": {
                "editor.background": "#ffffff"
            }
        }"##;

        let vscode = VsCodeTheme::load_from_str(json).unwrap();
        assert!(vscode.is_light());

        let theme = vscode.to_theme();
        assert!(theme.is_light);
    }
}
