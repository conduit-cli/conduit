//! VSCode theme to Conduit TOML migration.
//!
//! Converts VSCode JSON themes to the native Conduit TOML format.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use ratatui::style::Color;

use super::toml::color_to_hex;
use super::types::Theme;
use super::vscode::VsCodeTheme;

/// Migration options.
#[derive(Debug, Default)]
pub struct MigrateOptions {
    /// Extract common colors into a palette section
    pub extract_palette: bool,
    /// Include all colors (verbose mode)
    pub verbose: bool,
}

/// Migration result.
pub struct MigrateResult {
    /// The generated TOML content
    pub toml: String,
    /// The theme name extracted from the source
    pub name: String,
    /// Whether it's a light theme
    pub is_light: bool,
}

/// Error types for migration.
#[derive(Debug)]
pub enum MigrateError {
    /// IO error
    Io(std::io::Error),
    /// Theme loading error
    LoadError(String),
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrateError::Io(e) => write!(f, "IO error: {}", e),
            MigrateError::LoadError(msg) => write!(f, "Load error: {}", msg),
        }
    }
}

impl std::error::Error for MigrateError {}

/// Migrate a VSCode theme file to Conduit TOML format.
pub fn migrate_vscode_theme(
    input_path: &Path,
    options: &MigrateOptions,
) -> Result<MigrateResult, MigrateError> {
    // Load the VSCode theme
    let vscode = VsCodeTheme::load_from_file(input_path)
        .map_err(|e| MigrateError::LoadError(e.to_string()))?;

    // Convert to our Theme format
    let theme = vscode.to_theme();
    let is_light = vscode.is_light();
    let name = vscode
        .name
        .clone()
        .unwrap_or_else(|| "Migrated Theme".to_string());

    // Generate TOML
    let toml = generate_toml(&theme, &name, is_light, input_path, options);

    Ok(MigrateResult {
        toml,
        name,
        is_light,
    })
}

/// Generate TOML content from a theme.
fn generate_toml(
    theme: &Theme,
    name: &str,
    is_light: bool,
    source_path: &Path,
    options: &MigrateOptions,
) -> String {
    let mut output = String::new();

    // Header comment
    output.push_str(&format!("# Conduit Theme: {}\n", name.replace('\n', " ")));
    output.push_str(&format!("# Migrated from: {}\n", source_path.display()));
    output.push_str("#\n");
    output.push_str("# This theme was auto-generated from a VSCode theme.\n");
    output.push_str("# You can customize it by editing the values below.\n");
    output.push_str("# Colors support: hex (#rrggbb), rgb(r, g, b), references ($name),\n");
    output.push_str("# and functions: darken(), lighten(), dim(), boost(), mix(), shift_hue()\n\n");

    // Metadata
    output.push_str("[meta]\n");
    output.push_str(&format!("name = \"{}\"\n", escape_toml_string(name)));
    output.push_str(&format!(
        "type = \"{}\"\n",
        if is_light { "light" } else { "dark" }
    ));
    output.push_str("version = \"1.0.0\"\n");
    output.push('\n');

    // Extract palette if requested
    let palette = if options.extract_palette {
        extract_palette(theme)
    } else {
        HashMap::new()
    };

    if !palette.is_empty() {
        output.push_str("[palette]\n");
        for (name, hex) in &palette {
            output.push_str(&format!("{} = \"{}\"\n", name, hex));
        }
        output.push('\n');
    }

    // Helper to get color value (palette ref or hex)
    let color_value = |color: Color| -> String {
        let hex = color_to_hex(color);
        if let Some(name) = palette.iter().find(|(_, v)| **v == hex).map(|(k, _)| k) {
            format!("${}", name)
        } else {
            hex
        }
    };

    // Background section
    output.push_str("[background]\n");
    output.push_str(&format!(
        "terminal = \"{}\"\n",
        color_value(theme.bg_terminal)
    ));
    output.push_str(&format!("base = \"{}\"\n", color_value(theme.bg_base)));
    output.push_str(&format!(
        "surface = \"{}\"\n",
        color_value(theme.bg_surface)
    ));
    output.push_str(&format!(
        "elevated = \"{}\"\n",
        color_value(theme.bg_elevated)
    ));
    output.push_str(&format!(
        "highlight = \"{}\"\n",
        color_value(theme.bg_highlight)
    ));
    output.push_str(&format!(
        "markdown_code = \"{}\"\n",
        color_value(theme.markdown_code_bg)
    ));
    output.push_str(&format!(
        "markdown_inline_code = \"{}\"\n",
        color_value(theme.markdown_inline_code_bg)
    ));
    output.push('\n');

    // Text section
    output.push_str("[text]\n");
    output.push_str(&format!(
        "bright = \"{}\"\n",
        color_value(theme.text_bright)
    ));
    output.push_str(&format!(
        "primary = \"{}\"\n",
        color_value(theme.text_primary)
    ));
    output.push_str(&format!(
        "secondary = \"{}\"\n",
        color_value(theme.text_secondary)
    ));
    output.push_str(&format!("muted = \"{}\"\n", color_value(theme.text_muted)));
    output.push_str(&format!("faint = \"{}\"\n", color_value(theme.text_faint)));
    output.push('\n');

    // Accent section
    output.push_str("[accent]\n");
    output.push_str(&format!(
        "primary = \"{}\"\n",
        color_value(theme.accent_primary)
    ));
    output.push_str(&format!(
        "secondary = \"{}\"\n",
        color_value(theme.accent_secondary)
    ));
    output.push_str(&format!(
        "success = \"{}\"\n",
        color_value(theme.accent_success)
    ));
    output.push_str(&format!(
        "warning = \"{}\"\n",
        color_value(theme.accent_warning)
    ));
    output.push_str(&format!(
        "error = \"{}\"\n",
        color_value(theme.accent_error)
    ));
    output.push('\n');

    // Agent section
    output.push_str("[agent]\n");
    output.push_str(&format!(
        "claude = \"{}\"\n",
        color_value(theme.agent_claude)
    ));
    output.push_str(&format!("codex = \"{}\"\n", color_value(theme.agent_codex)));
    output.push_str(&format!(
        "opencode = \"{}\"\n",
        color_value(theme.agent_opencode)
    ));
    output.push('\n');

    // PR section
    output.push_str("[pr]\n");
    output.push_str(&format!("open = \"{}\"\n", color_value(theme.pr_open_bg)));
    output.push_str(&format!(
        "merged = \"{}\"\n",
        color_value(theme.pr_merged_bg)
    ));
    output.push_str(&format!(
        "closed = \"{}\"\n",
        color_value(theme.pr_closed_bg)
    ));
    output.push_str(&format!("draft = \"{}\"\n", color_value(theme.pr_draft_bg)));
    output.push_str(&format!(
        "unknown = \"{}\"\n",
        color_value(theme.pr_unknown_bg)
    ));
    output.push('\n');

    // Spinner section
    output.push_str("[spinner]\n");
    output.push_str(&format!(
        "active = \"{}\"\n",
        color_value(theme.spinner_active)
    ));
    output.push_str(&format!(
        "trail_1 = \"{}\"\n",
        color_value(theme.spinner_trail_1)
    ));
    output.push_str(&format!(
        "trail_2 = \"{}\"\n",
        color_value(theme.spinner_trail_2)
    ));
    output.push_str(&format!(
        "trail_3 = \"{}\"\n",
        color_value(theme.spinner_trail_3)
    ));
    output.push_str(&format!(
        "trail_4 = \"{}\"\n",
        color_value(theme.spinner_trail_4)
    ));
    output.push_str(&format!(
        "trail_5 = \"{}\"\n",
        color_value(theme.spinner_trail_5)
    ));
    output.push_str(&format!(
        "inactive = \"{}\"\n",
        color_value(theme.spinner_inactive)
    ));
    output.push('\n');

    // Border section
    output.push_str("[border]\n");
    output.push_str(&format!(
        "default = \"{}\"\n",
        color_value(theme.border_default)
    ));
    output.push_str(&format!(
        "focused = \"{}\"\n",
        color_value(theme.border_focused)
    ));
    output.push_str(&format!(
        "dimmed = \"{}\"\n",
        color_value(theme.border_dimmed)
    ));
    output.push('\n');

    // Shine section
    output.push_str("[shine]\n");
    output.push_str(&format!("edge = \"{}\"\n", color_value(theme.shine_edge)));
    output.push_str(&format!("mid = \"{}\"\n", color_value(theme.shine_mid)));
    output.push_str(&format!(
        "center = \"{}\"\n",
        color_value(theme.shine_center)
    ));
    output.push_str(&format!("peak = \"{}\"\n", color_value(theme.shine_peak)));
    output.push('\n');

    // Tool section
    output.push_str("[tool]\n");
    output.push_str(&format!(
        "background = \"{}\"\n",
        color_value(theme.tool_block_bg)
    ));
    output.push_str(&format!(
        "comment = \"{}\"\n",
        color_value(theme.tool_comment)
    ));
    output.push_str(&format!(
        "command = \"{}\"\n",
        color_value(theme.tool_command)
    ));
    output.push_str(&format!(
        "output = \"{}\"\n",
        color_value(theme.tool_output)
    ));
    output.push_str(&format!("diff_add = \"{}\"\n", color_value(theme.diff_add)));
    output.push_str(&format!(
        "diff_remove = \"{}\"\n",
        color_value(theme.diff_remove)
    ));

    output
}

/// Extract frequently used colors into a palette.
fn extract_palette(theme: &Theme) -> HashMap<String, String> {
    let mut color_counts: HashMap<String, usize> = HashMap::new();

    // Count all colors
    let all_colors = [
        theme.bg_terminal,
        theme.bg_base,
        theme.bg_surface,
        theme.bg_elevated,
        theme.bg_highlight,
        theme.markdown_code_bg,
        theme.markdown_inline_code_bg,
        theme.text_bright,
        theme.text_primary,
        theme.text_secondary,
        theme.text_muted,
        theme.text_faint,
        theme.accent_primary,
        theme.accent_secondary,
        theme.accent_success,
        theme.accent_warning,
        theme.accent_error,
        theme.agent_claude,
        theme.agent_codex,
        theme.agent_opencode,
        theme.pr_open_bg,
        theme.pr_merged_bg,
        theme.pr_closed_bg,
        theme.pr_draft_bg,
        theme.pr_unknown_bg,
        theme.spinner_active,
        theme.spinner_trail_1,
        theme.spinner_trail_2,
        theme.spinner_trail_3,
        theme.spinner_trail_4,
        theme.spinner_trail_5,
        theme.spinner_inactive,
        theme.border_default,
        theme.border_focused,
        theme.border_dimmed,
        theme.shine_edge,
        theme.shine_mid,
        theme.shine_center,
        theme.shine_peak,
        theme.tool_block_bg,
        theme.tool_comment,
        theme.tool_command,
        theme.tool_output,
        theme.diff_add,
        theme.diff_remove,
    ];

    for color in all_colors {
        let hex = color_to_hex(color);
        *color_counts.entry(hex).or_insert(0) += 1;
    }

    // Only include colors used more than once
    let mut palette = HashMap::new();

    // Sort by count (descending) for consistent naming
    let mut sorted: Vec<_> = color_counts.into_iter().filter(|(_, c)| *c > 1).collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    for (color_index, (hex, _)) in sorted.into_iter().enumerate() {
        let name = match color_index {
            0 => "color_a".to_string(),
            1 => "color_b".to_string(),
            2 => "color_c".to_string(),
            3 => "color_d".to_string(),
            4 => "color_e".to_string(),
            5 => "color_f".to_string(),
            _ => format!("color_{}", color_index),
        };
        palette.insert(name, hex);
    }

    palette
}

/// Escape a string for TOML.
fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Write a migrated theme to a file.
pub fn write_theme_file(output_path: &Path, content: &str) -> Result<(), MigrateError> {
    // Create parent directories if needed
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(MigrateError::Io)?;
    }

    fs::write(output_path, content).map_err(MigrateError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_toml_string() {
        assert_eq!(escape_toml_string("Hello"), "Hello");
        assert_eq!(escape_toml_string("Hello\"World"), "Hello\\\"World");
        assert_eq!(escape_toml_string("Line1\nLine2"), "Line1\\nLine2");
    }

    #[test]
    fn test_generate_toml_basic() {
        let theme = Theme::default_dark();
        let options = MigrateOptions::default();
        let toml = generate_toml(
            &theme,
            "Test Theme",
            false,
            Path::new("/test/theme.json"),
            &options,
        );

        assert!(toml.contains("[meta]"));
        assert!(toml.contains("name = \"Test Theme\""));
        assert!(toml.contains("type = \"dark\""));
        assert!(toml.contains("[background]"));
        assert!(toml.contains("[text]"));
        assert!(toml.contains("[accent]"));
    }

    #[test]
    fn test_extract_palette() {
        let theme = Theme::default_dark();
        let palette = extract_palette(&theme);

        // Default theme has some repeated colors (accent_primary = spinner_active, etc.)
        assert!(!palette.is_empty());
    }
}
