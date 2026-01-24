//! Theme type definitions.
//!
//! Defines the `Theme` struct with all semantic color fields.

use ratatui::style::Color;

/// Complete theme definition with all semantic colors.
///
/// Note: This struct intentionally doesn't derive Serialize/Deserialize because
/// ratatui::Color doesn't implement those traits. Theme selection is persisted
/// by name in config, not by serializing the full Theme struct.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme display name
    pub name: String,

    /// Whether this is a light theme
    pub is_light: bool,

    // =========================================================================
    // Background Layers (7 colors)
    // =========================================================================
    /// Terminal's actual background - deepest layer
    pub bg_terminal: Color,
    /// Main app background
    pub bg_base: Color,
    /// Panels, cards, sidebar - slightly elevated
    pub bg_surface: Color,
    /// Modals, dropdowns, hover states - clearly elevated
    pub bg_elevated: Color,
    /// Selection background, highlights
    pub bg_highlight: Color,
    /// Markdown code block background
    pub markdown_code_bg: Color,
    /// Markdown inline code background
    pub markdown_inline_code_bg: Color,

    // =========================================================================
    // Text Hierarchy (5 colors)
    // =========================================================================
    /// Brightest text - for emphasis
    pub text_bright: Color,
    /// Main content text
    pub text_primary: Color,
    /// Labels, metadata, secondary info
    pub text_secondary: Color,
    /// Hints, disabled text, placeholders
    pub text_muted: Color,
    /// Decorative elements, separators
    pub text_faint: Color,

    // =========================================================================
    // Accent Colors (5 colors)
    // =========================================================================
    /// Focus, selection, primary actions
    pub accent_primary: Color,
    /// Secondary highlights
    pub accent_secondary: Color,
    /// Success states, confirmations
    pub accent_success: Color,
    /// Warnings, processing states
    pub accent_warning: Color,
    /// Errors, destructive actions
    pub accent_error: Color,

    // =========================================================================
    // Agent Colors (2 colors)
    // =========================================================================
    /// Claude agent color
    pub agent_claude: Color,
    /// Codex agent color
    pub agent_codex: Color,
    /// OpenCode agent color
    pub agent_opencode: Color,

    // =========================================================================
    // PR State Colors (5 colors)
    // =========================================================================
    /// Open PR background
    pub pr_open_bg: Color,
    /// Merged PR background
    pub pr_merged_bg: Color,
    /// Closed PR background
    pub pr_closed_bg: Color,
    /// Draft PR background
    pub pr_draft_bg: Color,
    /// Unknown PR state background
    pub pr_unknown_bg: Color,

    // =========================================================================
    // Spinner Colors (7 colors)
    // =========================================================================
    /// Spinner active position - brightest
    pub spinner_active: Color,
    /// Spinner trail position 1
    pub spinner_trail_1: Color,
    /// Spinner trail position 2
    pub spinner_trail_2: Color,
    /// Spinner trail position 3
    pub spinner_trail_3: Color,
    /// Spinner trail position 4
    pub spinner_trail_4: Color,
    /// Spinner trail position 5
    pub spinner_trail_5: Color,
    /// Spinner inactive position
    pub spinner_inactive: Color,

    // =========================================================================
    // Border Colors (3 colors)
    // =========================================================================
    /// Default subtle border
    pub border_default: Color,
    /// Focused element border
    pub border_focused: Color,
    /// Very subtle decorative border
    pub border_dimmed: Color,

    // =========================================================================
    // Logo Shine Animation Colors (4 colors)
    // =========================================================================
    /// Shine edge - subtle brightening
    pub shine_edge: Color,
    /// Shine mid - medium brightness
    pub shine_mid: Color,
    /// Shine center - bright
    pub shine_center: Color,
    /// Shine peak - white center
    pub shine_peak: Color,

    // =========================================================================
    // Tool Block Colors (6 colors)
    // =========================================================================
    /// Tool block background
    pub tool_block_bg: Color,
    /// Tool comment text
    pub tool_comment: Color,
    /// Tool command text
    pub tool_command: Color,
    /// Tool output text
    pub tool_output: Color,
    /// Diff add lines
    pub diff_add: Color,
    /// Diff remove lines
    pub diff_remove: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_dark()
    }
}

impl Theme {
    /// Create the default dark theme (matches current hardcoded values).
    pub fn default_dark() -> Self {
        Self {
            name: "Default".to_string(),
            is_light: false,

            // Background Layers
            bg_terminal: Color::Rgb(16, 16, 16),
            bg_base: Color::Rgb(22, 22, 30),
            bg_surface: Color::Rgb(30, 30, 40),
            bg_elevated: Color::Rgb(40, 40, 52),
            bg_highlight: Color::Rgb(50, 55, 70),
            markdown_code_bg: Color::Rgb(30, 30, 30),
            markdown_inline_code_bg: Color::Rgb(40, 40, 40),

            // Text Hierarchy
            text_bright: Color::Rgb(250, 250, 255),
            text_primary: Color::Rgb(220, 220, 230),
            text_secondary: Color::Rgb(160, 160, 180),
            text_muted: Color::Rgb(100, 100, 120),
            text_faint: Color::Rgb(70, 70, 85),

            // Accent Colors
            accent_primary: Color::Rgb(130, 170, 255),
            accent_secondary: Color::Rgb(180, 140, 255),
            accent_success: Color::Rgb(130, 200, 140),
            accent_warning: Color::Rgb(230, 180, 100),
            accent_error: Color::Rgb(230, 120, 120),

            // Agent Colors
            agent_claude: Color::Rgb(130, 180, 220),
            agent_codex: Color::Rgb(180, 140, 200),
            agent_opencode: Color::Rgb(120, 200, 190),

            // PR State Colors
            pr_open_bg: Color::Rgb(35, 134, 54),
            pr_merged_bg: Color::Rgb(130, 80, 223),
            pr_closed_bg: Color::Rgb(218, 54, 51),
            pr_draft_bg: Color::Rgb(110, 118, 129),
            pr_unknown_bg: Color::Rgb(80, 80, 90),

            // Spinner Colors
            spinner_active: Color::Rgb(130, 170, 255),
            spinner_trail_1: Color::Rgb(110, 145, 220),
            spinner_trail_2: Color::Rgb(90, 120, 185),
            spinner_trail_3: Color::Rgb(70, 95, 150),
            spinner_trail_4: Color::Rgb(50, 70, 115),
            spinner_trail_5: Color::Rgb(35, 50, 85),
            spinner_inactive: Color::Rgb(100, 100, 120),

            // Border Colors
            border_default: Color::Rgb(50, 50, 65),
            border_focused: Color::Rgb(130, 170, 255),
            border_dimmed: Color::Rgb(35, 35, 45),

            // Logo Shine Colors
            shine_edge: Color::Rgb(130, 130, 150),
            shine_mid: Color::Rgb(180, 180, 200),
            shine_center: Color::Rgb(230, 230, 245),
            shine_peak: Color::Rgb(255, 255, 255),

            // Tool Block Colors
            tool_block_bg: Color::Rgb(24, 25, 32),
            tool_comment: Color::Rgb(120, 120, 130),
            tool_command: Color::Rgb(200, 200, 210),
            tool_output: Color::Rgb(160, 160, 170),
            diff_add: Color::Rgb(130, 200, 140),
            diff_remove: Color::Rgb(230, 120, 120),
        }
    }

    /// Create a default light theme.
    pub fn default_light() -> Self {
        Self {
            name: "Default Light".to_string(),
            is_light: true,

            // Background Layers (inverted for light)
            bg_terminal: Color::Rgb(255, 255, 255),
            bg_base: Color::Rgb(250, 250, 252),
            bg_surface: Color::Rgb(240, 240, 245),
            bg_elevated: Color::Rgb(230, 230, 238),
            bg_highlight: Color::Rgb(210, 215, 225),
            markdown_code_bg: Color::Rgb(235, 235, 240),
            markdown_inline_code_bg: Color::Rgb(225, 225, 232),

            // Text Hierarchy (inverted for light)
            text_bright: Color::Rgb(15, 15, 20),
            text_primary: Color::Rgb(35, 35, 45),
            text_secondary: Color::Rgb(90, 90, 105),
            text_muted: Color::Rgb(140, 140, 155),
            text_faint: Color::Rgb(180, 180, 190),

            // Accent Colors (adjusted for light background)
            accent_primary: Color::Rgb(60, 120, 220),
            accent_secondary: Color::Rgb(130, 80, 200),
            accent_success: Color::Rgb(40, 160, 60),
            accent_warning: Color::Rgb(200, 140, 30),
            accent_error: Color::Rgb(200, 60, 60),

            // Agent Colors
            agent_claude: Color::Rgb(50, 130, 180),
            agent_codex: Color::Rgb(130, 80, 160),
            agent_opencode: Color::Rgb(70, 160, 170),

            // PR State Colors (same as dark - good contrast)
            pr_open_bg: Color::Rgb(35, 134, 54),
            pr_merged_bg: Color::Rgb(130, 80, 223),
            pr_closed_bg: Color::Rgb(218, 54, 51),
            pr_draft_bg: Color::Rgb(110, 118, 129),
            pr_unknown_bg: Color::Rgb(130, 130, 140),

            // Spinner Colors (adjusted for light)
            spinner_active: Color::Rgb(60, 120, 220),
            spinner_trail_1: Color::Rgb(80, 135, 210),
            spinner_trail_2: Color::Rgb(100, 150, 200),
            spinner_trail_3: Color::Rgb(130, 165, 195),
            spinner_trail_4: Color::Rgb(160, 180, 200),
            spinner_trail_5: Color::Rgb(190, 200, 215),
            spinner_inactive: Color::Rgb(140, 140, 155),

            // Border Colors (adjusted for light)
            border_default: Color::Rgb(200, 200, 210),
            border_focused: Color::Rgb(60, 120, 220),
            border_dimmed: Color::Rgb(220, 220, 228),

            // Logo Shine Colors (inverted for light)
            shine_edge: Color::Rgb(180, 180, 190),
            shine_mid: Color::Rgb(140, 140, 155),
            shine_center: Color::Rgb(80, 80, 100),
            shine_peak: Color::Rgb(30, 30, 40),

            // Tool Block Colors (adjusted for light)
            tool_block_bg: Color::Rgb(242, 243, 248),
            tool_comment: Color::Rgb(120, 120, 135),
            tool_command: Color::Rgb(50, 50, 65),
            tool_output: Color::Rgb(80, 80, 95),
            diff_add: Color::Rgb(40, 160, 60),
            diff_remove: Color::Rgb(200, 60, 60),
        }
    }
}

/// Information about an available theme.
#[derive(Debug, Clone)]
pub struct ThemeInfo {
    /// Theme identifier (used for loading)
    pub name: String,
    /// Display name shown to user
    pub display_name: String,
    /// Where the theme comes from
    pub source: ThemeSource,
    /// Whether this is a light theme
    pub is_light: bool,
}

/// Source of a theme.
#[derive(Debug, Clone)]
pub enum ThemeSource {
    /// Built into the binary
    Builtin,
    /// Discovered from VS Code extensions
    VsCodeExtension {
        /// Path to the theme JSON file
        path: std::path::PathBuf,
    },
    /// Conduit native TOML theme
    ConduitToml {
        /// Path to the theme TOML file
        path: std::path::PathBuf,
    },
    /// Custom path specified by user
    CustomPath {
        /// Path to the theme file (JSON or TOML)
        path: std::path::PathBuf,
    },
}
