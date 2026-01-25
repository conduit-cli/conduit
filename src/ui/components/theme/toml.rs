//! TOML theme parser and builder.
//!
//! Parses Conduit's native TOML theme format with support for:
//! - Color definitions (hex, RGB)
//! - Palette references ($name)
//! - Cross-section references ($section.name)
//! - Color derivation functions (darken, lighten, etc.)
//! - Theme inheritance

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use ratatui::style::Color;
use serde::Deserialize;

use super::builtin::get_builtin;
use super::colors::{
    boost_brightness, darken, desaturate, dim, interpolate, lighten, parse_hex_color, saturate,
    shift_hue,
};
use super::types::Theme;

/// TOML theme file structure.
#[derive(Debug, Deserialize)]
pub struct TomlTheme {
    /// Theme metadata
    pub meta: TomlMeta,

    /// Reusable color palette
    #[serde(default)]
    pub palette: HashMap<String, String>,

    /// Background colors
    #[serde(default)]
    pub background: Option<TomlBackground>,

    /// Text colors
    #[serde(default)]
    pub text: Option<TomlText>,

    /// Accent colors
    #[serde(default)]
    pub accent: Option<TomlAccent>,

    /// Agent colors
    #[serde(default)]
    pub agent: Option<TomlAgent>,

    /// PR state colors
    #[serde(default)]
    pub pr: Option<TomlPr>,

    /// Spinner colors
    #[serde(default)]
    pub spinner: Option<TomlSpinner>,

    /// Border colors
    #[serde(default)]
    pub border: Option<TomlBorder>,

    /// Shine animation colors
    #[serde(default)]
    pub shine: Option<TomlShine>,

    /// Tool block colors
    #[serde(default)]
    pub tool: Option<TomlTool>,
}

/// Theme metadata.
#[derive(Debug, Deserialize)]
pub struct TomlMeta {
    /// Display name
    pub name: String,

    /// Theme type: "dark" or "light"
    #[serde(rename = "type")]
    pub theme_type: String,

    /// Optional author
    #[serde(default)]
    pub author: Option<String>,

    /// Optional version
    #[serde(default)]
    pub version: Option<String>,

    /// Optional description
    #[serde(default)]
    pub description: Option<String>,

    /// Base theme to inherit from
    #[serde(default)]
    pub inherits: Option<String>,
}

/// Background section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlBackground {
    pub terminal: Option<String>,
    pub base: Option<String>,
    pub surface: Option<String>,
    pub elevated: Option<String>,
    pub highlight: Option<String>,
    pub markdown_code: Option<String>,
    pub markdown_inline_code: Option<String>,
}

/// Text section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlText {
    pub bright: Option<String>,
    pub primary: Option<String>,
    pub secondary: Option<String>,
    pub muted: Option<String>,
    pub faint: Option<String>,
}

/// Accent section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlAccent {
    pub primary: Option<String>,
    pub secondary: Option<String>,
    pub success: Option<String>,
    pub warning: Option<String>,
    pub error: Option<String>,
}

/// Agent section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlAgent {
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub opencode: Option<String>,
}

/// PR state section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlPr {
    pub open: Option<String>,
    pub merged: Option<String>,
    pub closed: Option<String>,
    pub draft: Option<String>,
    pub unknown: Option<String>,
}

/// Spinner section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlSpinner {
    pub active: Option<String>,
    pub trail_1: Option<String>,
    pub trail_2: Option<String>,
    pub trail_3: Option<String>,
    pub trail_4: Option<String>,
    pub trail_5: Option<String>,
    pub inactive: Option<String>,
}

/// Border section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlBorder {
    pub default: Option<String>,
    pub focused: Option<String>,
    pub dimmed: Option<String>,
}

/// Shine section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlShine {
    pub edge: Option<String>,
    pub mid: Option<String>,
    pub center: Option<String>,
    pub peak: Option<String>,
}

/// Tool section.
#[derive(Debug, Default, Deserialize)]
pub struct TomlTool {
    pub background: Option<String>,
    pub comment: Option<String>,
    pub command: Option<String>,
    pub output: Option<String>,
    pub diff_add: Option<String>,
    pub diff_remove: Option<String>,
}

/// Error types for TOML theme loading.
#[derive(Debug)]
pub enum TomlThemeError {
    /// IO error reading file
    Io(std::io::Error),
    /// TOML parse error
    Parse(toml::de::Error),
    /// Invalid theme type
    InvalidType(String),
    /// Color parse error
    InvalidColor { field: String, value: String },
    /// Reference error
    InvalidReference { field: String, reference: String },
    /// Circular reference
    CircularReference { field: String },
    /// Base theme not found
    BaseNotFound(String),
    /// Maximum inheritance depth exceeded
    MaxInheritanceDepth,
}

impl std::fmt::Display for TomlThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TomlThemeError::Io(e) => write!(f, "IO error: {}", e),
            TomlThemeError::Parse(e) => write!(f, "TOML parse error: {}", e),
            TomlThemeError::InvalidType(t) => {
                write!(f, "Invalid theme type '{}', expected 'dark' or 'light'", t)
            }
            TomlThemeError::InvalidColor { field, value } => {
                write!(f, "Invalid color '{}' for field '{}'", value, field)
            }
            TomlThemeError::InvalidReference { field, reference } => {
                write!(f, "Invalid reference '{}' in field '{}'", reference, field)
            }
            TomlThemeError::CircularReference { field } => {
                write!(f, "Circular reference detected in field '{}'", field)
            }
            TomlThemeError::BaseNotFound(name) => write!(f, "Base theme '{}' not found", name),
            TomlThemeError::MaxInheritanceDepth => {
                write!(f, "Maximum inheritance depth (5) exceeded")
            }
        }
    }
}

impl std::error::Error for TomlThemeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TomlThemeError::Io(e) => Some(e),
            TomlThemeError::Parse(e) => Some(e),
            _ => None,
        }
    }
}

impl TomlTheme {
    /// Load a TOML theme from a file.
    pub fn load_from_file(path: &Path) -> Result<Self, TomlThemeError> {
        let content = fs::read_to_string(path).map_err(TomlThemeError::Io)?;
        Self::load_from_str(&content)
    }

    /// Load a TOML theme from a string.
    pub fn load_from_str(content: &str) -> Result<Self, TomlThemeError> {
        toml::from_str(content).map_err(TomlThemeError::Parse)
    }

    /// Check if this is a light theme.
    pub fn is_light(&self) -> bool {
        self.meta.theme_type.eq_ignore_ascii_case("light")
    }

    /// Convert this TOML theme to our Theme format.
    pub fn to_theme(&self) -> Result<Theme, TomlThemeError> {
        self.to_theme_with_depth(0)
    }

    /// Convert with inheritance depth tracking.
    fn to_theme_with_depth(&self, depth: usize) -> Result<Theme, TomlThemeError> {
        if depth > 5 {
            return Err(TomlThemeError::MaxInheritanceDepth);
        }

        // Validate theme type
        if !self.meta.theme_type.eq_ignore_ascii_case("dark")
            && !self.meta.theme_type.eq_ignore_ascii_case("light")
        {
            return Err(TomlThemeError::InvalidType(self.meta.theme_type.clone()));
        }

        // Load base theme if inheriting
        let base = if let Some(ref inherits) = self.meta.inherits {
            // Try built-in first
            if let Some(theme) = get_builtin(inherits) {
                Some(theme)
            } else {
                // Could extend to load from path in the future
                return Err(TomlThemeError::BaseNotFound(inherits.clone()));
            }
        } else {
            None
        };

        let builder = ThemeBuilder::new(self, base);
        builder.build()
    }
}

/// Builds a Theme from TOML definitions.
struct ThemeBuilder<'a> {
    toml: &'a TomlTheme,
    base: Option<Theme>,
    /// Resolved colors cache
    resolved: HashMap<String, Color>,
    /// Currently resolving (for cycle detection)
    resolving: Vec<String>,
}

impl<'a> ThemeBuilder<'a> {
    fn new(toml: &'a TomlTheme, base: Option<Theme>) -> Self {
        Self {
            toml,
            base,
            resolved: HashMap::new(),
            resolving: Vec::new(),
        }
    }

    /// Build the complete theme.
    fn build(mut self) -> Result<Theme, TomlThemeError> {
        let is_light = self.toml.is_light();
        let default = if is_light {
            Theme::default_light()
        } else {
            Theme::default_dark()
        };

        // Use base theme or default as starting point
        let base = self.base.take().unwrap_or(default);

        // Helper macro to resolve a color field
        macro_rules! resolve_color {
            ($section:ident, $field:ident, $base_field:ident) => {
                if let Some(ref section) = self.toml.$section {
                    if let Some(ref value) = section.$field {
                        self.resolve_color(
                            &format!("{}.{}", stringify!($section), stringify!($field)),
                            value,
                        )?
                    } else {
                        base.$base_field
                    }
                } else {
                    base.$base_field
                }
            };
        }

        Ok(Theme {
            name: self.toml.meta.name.clone(),
            is_light,

            // Background
            bg_terminal: resolve_color!(background, terminal, bg_terminal),
            bg_base: resolve_color!(background, base, bg_base),
            bg_surface: resolve_color!(background, surface, bg_surface),
            bg_elevated: resolve_color!(background, elevated, bg_elevated),
            bg_highlight: resolve_color!(background, highlight, bg_highlight),
            markdown_code_bg: resolve_color!(background, markdown_code, markdown_code_bg),
            markdown_inline_code_bg: resolve_color!(
                background,
                markdown_inline_code,
                markdown_inline_code_bg
            ),

            // Text
            text_bright: resolve_color!(text, bright, text_bright),
            text_primary: resolve_color!(text, primary, text_primary),
            text_secondary: resolve_color!(text, secondary, text_secondary),
            text_muted: resolve_color!(text, muted, text_muted),
            text_faint: resolve_color!(text, faint, text_faint),

            // Accent
            accent_primary: resolve_color!(accent, primary, accent_primary),
            accent_secondary: resolve_color!(accent, secondary, accent_secondary),
            accent_success: resolve_color!(accent, success, accent_success),
            accent_warning: resolve_color!(accent, warning, accent_warning),
            accent_error: resolve_color!(accent, error, accent_error),

            // Agent
            agent_claude: resolve_color!(agent, claude, agent_claude),
            agent_codex: resolve_color!(agent, codex, agent_codex),
            agent_opencode: resolve_color!(agent, opencode, agent_opencode),

            // PR
            pr_open_bg: resolve_color!(pr, open, pr_open_bg),
            pr_merged_bg: resolve_color!(pr, merged, pr_merged_bg),
            pr_closed_bg: resolve_color!(pr, closed, pr_closed_bg),
            pr_draft_bg: resolve_color!(pr, draft, pr_draft_bg),
            pr_unknown_bg: resolve_color!(pr, unknown, pr_unknown_bg),

            // Spinner
            spinner_active: resolve_color!(spinner, active, spinner_active),
            spinner_trail_1: resolve_color!(spinner, trail_1, spinner_trail_1),
            spinner_trail_2: resolve_color!(spinner, trail_2, spinner_trail_2),
            spinner_trail_3: resolve_color!(spinner, trail_3, spinner_trail_3),
            spinner_trail_4: resolve_color!(spinner, trail_4, spinner_trail_4),
            spinner_trail_5: resolve_color!(spinner, trail_5, spinner_trail_5),
            spinner_inactive: resolve_color!(spinner, inactive, spinner_inactive),

            // Border
            border_default: resolve_color!(border, default, border_default),
            border_focused: resolve_color!(border, focused, border_focused),
            border_dimmed: resolve_color!(border, dimmed, border_dimmed),

            // Shine
            shine_edge: resolve_color!(shine, edge, shine_edge),
            shine_mid: resolve_color!(shine, mid, shine_mid),
            shine_center: resolve_color!(shine, center, shine_center),
            shine_peak: resolve_color!(shine, peak, shine_peak),

            // Tool
            tool_block_bg: resolve_color!(tool, background, tool_block_bg),
            tool_comment: resolve_color!(tool, comment, tool_comment),
            tool_command: resolve_color!(tool, command, tool_command),
            tool_output: resolve_color!(tool, output, tool_output),
            diff_add: resolve_color!(tool, diff_add, diff_add),
            diff_remove: resolve_color!(tool, diff_remove, diff_remove),
        })
    }

    /// Resolve a color value (hex, RGB, reference, or function).
    fn resolve_color(&mut self, field: &str, value: &str) -> Result<Color, TomlThemeError> {
        let value = value.trim();

        // Check cache
        if let Some(&color) = self.resolved.get(value) {
            return Ok(color);
        }

        // Check for circular reference using the value, not field name
        // This prevents detecting a false cycle when resolving references
        if self.resolving.contains(&value.to_string()) {
            return Err(TomlThemeError::CircularReference {
                field: field.to_string(),
            });
        }
        self.resolving.push(value.to_string());

        let result = self.resolve_color_inner(field, value);

        self.resolving.pop();

        if let Ok(color) = result {
            self.resolved.insert(value.to_string(), color);
        }

        result
    }

    /// Inner color resolution logic.
    fn resolve_color_inner(&mut self, field: &str, value: &str) -> Result<Color, TomlThemeError> {
        // Try hex color
        if value.starts_with('#') || value.chars().all(|c| c.is_ascii_hexdigit()) {
            return parse_hex_color(value).ok_or_else(|| TomlThemeError::InvalidColor {
                field: field.to_string(),
                value: value.to_string(),
            });
        }

        // Try RGB notation
        if value.starts_with("rgb(") {
            return self.parse_rgb(field, value);
        }

        // Try palette/section reference
        if let Some(stripped) = value.strip_prefix('$') {
            return self.resolve_reference(field, stripped);
        }

        // Try function call
        if let Some(open_paren) = value.find('(') {
            let func_name = &value[..open_paren];
            let args_str = &value[open_paren + 1..value.len() - 1]; // Remove trailing )
            return self.evaluate_function(field, func_name, args_str);
        }

        Err(TomlThemeError::InvalidColor {
            field: field.to_string(),
            value: value.to_string(),
        })
    }

    /// Parse RGB notation like "rgb(130, 170, 255)".
    fn parse_rgb(&self, field: &str, value: &str) -> Result<Color, TomlThemeError> {
        let inner = value
            .strip_prefix("rgb(")
            .and_then(|s| s.strip_suffix(')'))
            .ok_or_else(|| TomlThemeError::InvalidColor {
                field: field.to_string(),
                value: value.to_string(),
            })?;

        let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() != 3 {
            return Err(TomlThemeError::InvalidColor {
                field: field.to_string(),
                value: value.to_string(),
            });
        }

        let r: u8 = parts[0].parse().map_err(|_| TomlThemeError::InvalidColor {
            field: field.to_string(),
            value: value.to_string(),
        })?;
        let g: u8 = parts[1].parse().map_err(|_| TomlThemeError::InvalidColor {
            field: field.to_string(),
            value: value.to_string(),
        })?;
        let b: u8 = parts[2].parse().map_err(|_| TomlThemeError::InvalidColor {
            field: field.to_string(),
            value: value.to_string(),
        })?;

        Ok(Color::Rgb(r, g, b))
    }

    /// Resolve a reference like "blue" (palette) or "accent.primary" (section).
    fn resolve_reference(&mut self, field: &str, reference: &str) -> Result<Color, TomlThemeError> {
        // Check palette first
        if let Some(palette_value) = self.toml.palette.get(reference) {
            return self.resolve_color(field, palette_value);
        }

        // Check section.field reference (e.g., "accent.primary")
        if let Some(dot_pos) = reference.find('.') {
            let section = &reference[..dot_pos];
            let section_field = &reference[dot_pos + 1..];

            let value = self.get_section_field(section, section_field);
            if let Some(v) = value {
                return self.resolve_color(field, &v);
            }
        }

        Err(TomlThemeError::InvalidReference {
            field: field.to_string(),
            reference: reference.to_string(),
        })
    }

    /// Get a value from a section.
    fn get_section_field(&self, section: &str, field: &str) -> Option<String> {
        match section {
            "background" => self.toml.background.as_ref().and_then(|s| match field {
                "terminal" => s.terminal.clone(),
                "base" => s.base.clone(),
                "surface" => s.surface.clone(),
                "elevated" => s.elevated.clone(),
                "highlight" => s.highlight.clone(),
                "markdown_code" => s.markdown_code.clone(),
                "markdown_inline_code" => s.markdown_inline_code.clone(),
                _ => None,
            }),
            "text" => self.toml.text.as_ref().and_then(|s| match field {
                "bright" => s.bright.clone(),
                "primary" => s.primary.clone(),
                "secondary" => s.secondary.clone(),
                "muted" => s.muted.clone(),
                "faint" => s.faint.clone(),
                _ => None,
            }),
            "accent" => self.toml.accent.as_ref().and_then(|s| match field {
                "primary" => s.primary.clone(),
                "secondary" => s.secondary.clone(),
                "success" => s.success.clone(),
                "warning" => s.warning.clone(),
                "error" => s.error.clone(),
                _ => None,
            }),
            "agent" => self.toml.agent.as_ref().and_then(|s| match field {
                "claude" => s.claude.clone(),
                "codex" => s.codex.clone(),
                "opencode" => s.opencode.clone(),
                _ => None,
            }),
            "pr" => self.toml.pr.as_ref().and_then(|s| match field {
                "open" => s.open.clone(),
                "merged" => s.merged.clone(),
                "closed" => s.closed.clone(),
                "draft" => s.draft.clone(),
                "unknown" => s.unknown.clone(),
                _ => None,
            }),
            "spinner" => self.toml.spinner.as_ref().and_then(|s| match field {
                "active" => s.active.clone(),
                "trail_1" => s.trail_1.clone(),
                "trail_2" => s.trail_2.clone(),
                "trail_3" => s.trail_3.clone(),
                "trail_4" => s.trail_4.clone(),
                "trail_5" => s.trail_5.clone(),
                "inactive" => s.inactive.clone(),
                _ => None,
            }),
            "border" => self.toml.border.as_ref().and_then(|s| match field {
                "default" => s.default.clone(),
                "focused" => s.focused.clone(),
                "dimmed" => s.dimmed.clone(),
                _ => None,
            }),
            "shine" => self.toml.shine.as_ref().and_then(|s| match field {
                "edge" => s.edge.clone(),
                "mid" => s.mid.clone(),
                "center" => s.center.clone(),
                "peak" => s.peak.clone(),
                _ => None,
            }),
            "tool" => self.toml.tool.as_ref().and_then(|s| match field {
                "background" => s.background.clone(),
                "comment" => s.comment.clone(),
                "command" => s.command.clone(),
                "output" => s.output.clone(),
                "diff_add" => s.diff_add.clone(),
                "diff_remove" => s.diff_remove.clone(),
                _ => None,
            }),
            _ => None,
        }
    }

    /// Evaluate a color function.
    fn evaluate_function(
        &mut self,
        field: &str,
        func_name: &str,
        args_str: &str,
    ) -> Result<Color, TomlThemeError> {
        let args = self.parse_function_args(args_str);

        match func_name {
            "darken" => {
                if args.len() != 2 {
                    return Err(TomlThemeError::InvalidColor {
                        field: field.to_string(),
                        value: format!("darken requires 2 arguments, got {}", args.len()),
                    });
                }
                let color = self.resolve_color(field, &args[0])?;
                let amount = self.parse_float(&args[1])?;
                Ok(darken(color, amount))
            }
            "lighten" => {
                if args.len() != 2 {
                    return Err(TomlThemeError::InvalidColor {
                        field: field.to_string(),
                        value: format!("lighten requires 2 arguments, got {}", args.len()),
                    });
                }
                let color = self.resolve_color(field, &args[0])?;
                let amount = self.parse_float(&args[1])?;
                Ok(lighten(color, amount))
            }
            "dim" => {
                if args.len() != 2 {
                    return Err(TomlThemeError::InvalidColor {
                        field: field.to_string(),
                        value: format!("dim requires 2 arguments, got {}", args.len()),
                    });
                }
                let color = self.resolve_color(field, &args[0])?;
                let factor = self.parse_float(&args[1])?;
                Ok(dim(color, factor))
            }
            "boost" => {
                if args.len() != 2 {
                    return Err(TomlThemeError::InvalidColor {
                        field: field.to_string(),
                        value: format!("boost requires 2 arguments, got {}", args.len()),
                    });
                }
                let color = self.resolve_color(field, &args[0])?;
                let factor = self.parse_float(&args[1])?;
                Ok(boost_brightness(color, factor))
            }
            "mix" => {
                if args.len() != 3 {
                    return Err(TomlThemeError::InvalidColor {
                        field: field.to_string(),
                        value: format!("mix requires 3 arguments, got {}", args.len()),
                    });
                }
                let color1 = self.resolve_color(field, &args[0])?;
                let color2 = self.resolve_color(field, &args[1])?;
                let t = self.parse_float(&args[2])?;
                Ok(interpolate(color1, color2, t))
            }
            "shift_hue" => {
                if args.len() != 2 {
                    return Err(TomlThemeError::InvalidColor {
                        field: field.to_string(),
                        value: format!("shift_hue requires 2 arguments, got {}", args.len()),
                    });
                }
                let color = self.resolve_color(field, &args[0])?;
                let degrees = self.parse_float(&args[1])?;
                Ok(shift_hue(color, degrees))
            }
            "saturate" => {
                if args.len() != 2 {
                    return Err(TomlThemeError::InvalidColor {
                        field: field.to_string(),
                        value: format!("saturate requires 2 arguments, got {}", args.len()),
                    });
                }
                let color = self.resolve_color(field, &args[0])?;
                let amount = self.parse_float(&args[1])?;
                Ok(saturate(color, amount))
            }
            "desaturate" => {
                if args.len() != 2 {
                    return Err(TomlThemeError::InvalidColor {
                        field: field.to_string(),
                        value: format!("desaturate requires 2 arguments, got {}", args.len()),
                    });
                }
                let color = self.resolve_color(field, &args[0])?;
                let amount = self.parse_float(&args[1])?;
                Ok(desaturate(color, amount))
            }
            _ => Err(TomlThemeError::InvalidColor {
                field: field.to_string(),
                value: format!("Unknown function '{}'", func_name),
            }),
        }
    }

    /// Parse function arguments, handling nested function calls.
    fn parse_function_args(&self, args_str: &str) -> Vec<String> {
        let mut args = Vec::new();
        let mut current = String::new();
        let mut depth = 0;

        for c in args_str.chars() {
            match c {
                '(' => {
                    depth += 1;
                    current.push(c);
                }
                ')' => {
                    depth -= 1;
                    current.push(c);
                }
                ',' if depth == 0 => {
                    args.push(current.trim().to_string());
                    current = String::new();
                }
                _ => current.push(c),
            }
        }

        if !current.trim().is_empty() {
            args.push(current.trim().to_string());
        }

        args
    }

    /// Parse a float value.
    fn parse_float(&self, s: &str) -> Result<f64, TomlThemeError> {
        s.trim().parse().map_err(|_| TomlThemeError::InvalidColor {
            field: "function argument".to_string(),
            value: s.to_string(),
        })
    }
}

/// Format a Color as a hex string.
pub fn color_to_hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{:02x}{:02x}{:02x}", r, g, b),
        _ => "#000000".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_theme() {
        let toml = r##"
[meta]
name = "Test Theme"
type = "dark"

[background]
base = "#1e1e2e"
"##;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        assert_eq!(theme.meta.name, "Test Theme");
        assert!(!theme.is_light());

        let built = theme.to_theme().unwrap();
        assert_eq!(built.name, "Test Theme");
        assert_eq!(built.bg_base, Color::Rgb(30, 30, 46));
    }

    #[test]
    fn test_palette_reference() {
        let toml = r##"
[meta]
name = "Palette Test"
type = "dark"

[palette]
blue = "#89b4fa"

[accent]
primary = "$blue"
"##;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        let built = theme.to_theme().unwrap();
        assert_eq!(built.accent_primary, Color::Rgb(137, 180, 250));
    }

    #[test]
    fn test_rgb_notation() {
        let toml = r#"
[meta]
name = "RGB Test"
type = "dark"

[text]
primary = "rgb(220, 220, 230)"
"#;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        let built = theme.to_theme().unwrap();
        assert_eq!(built.text_primary, Color::Rgb(220, 220, 230));
    }

    #[test]
    fn test_darken_function() {
        let toml = r##"
[meta]
name = "Function Test"
type = "dark"

[palette]
base = "#ffffff"

[background]
surface = "darken($base, 0.5)"
"##;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        let built = theme.to_theme().unwrap();
        // 255 * 0.5 = 127
        assert_eq!(built.bg_surface, Color::Rgb(127, 127, 127));
    }

    #[test]
    fn test_inheritance() {
        let toml = r##"
[meta]
name = "Child Theme"
type = "dark"
inherits = "default-dark"

[accent]
primary = "#ff0000"
"##;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        let built = theme.to_theme().unwrap();
        assert_eq!(built.accent_primary, Color::Rgb(255, 0, 0));
        // Other colors should come from default-dark
        assert_eq!(built.bg_base, Theme::default_dark().bg_base);
    }

    #[test]
    fn test_section_reference() {
        let toml = r##"
[meta]
name = "Cross Ref Test"
type = "dark"

[accent]
primary = "#82aaff"

[border]
focused = "$accent.primary"
"##;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        let built = theme.to_theme().unwrap();
        assert_eq!(built.border_focused, Color::Rgb(130, 170, 255));
    }

    #[test]
    fn test_nested_functions() {
        let toml = r##"
[meta]
name = "Nested Function Test"
type = "dark"

[palette]
blue = "#82aaff"

[spinner]
trail_1 = "dim($blue, 0.85)"
trail_2 = "dim($blue, 0.70)"
"##;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        let built = theme.to_theme().unwrap();
        // dim(130, 170, 255) by 0.85
        assert_eq!(built.spinner_trail_1, Color::Rgb(110, 144, 216));
    }

    #[test]
    fn test_light_theme() {
        let toml = r#"
[meta]
name = "Light Test"
type = "light"
"#;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        assert!(theme.is_light());

        let built = theme.to_theme().unwrap();
        assert!(built.is_light);
    }

    #[test]
    fn test_invalid_type() {
        let toml = r#"
[meta]
name = "Invalid"
type = "neon"
"#;

        let theme = TomlTheme::load_from_str(toml).unwrap();
        let result = theme.to_theme();
        assert!(matches!(result, Err(TomlThemeError::InvalidType(_))));
    }

    #[test]
    fn test_color_to_hex() {
        assert_eq!(color_to_hex(Color::Rgb(255, 0, 0)), "#ff0000");
        assert_eq!(color_to_hex(Color::Rgb(30, 30, 46)), "#1e1e2e");
    }
}
