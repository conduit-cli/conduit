//! Shared UI color constants.
//!
//! Modern TUI color palette with layered backgrounds and restrained accents.
//! Design principle: 80% neutrals, 15% structure, 5% accent.

use ratatui::style::Color;

// =============================================================================
// Background Layers (dark to light for depth)
// =============================================================================

/// Deepest background - main app background
pub const BG_BASE: Color = Color::Rgb(22, 22, 30);
/// Panels, cards, sidebar - slightly elevated
pub const BG_SURFACE: Color = Color::Rgb(30, 30, 40);
/// Modals, dropdowns, hover states - clearly elevated
pub const BG_ELEVATED: Color = Color::Rgb(40, 40, 52);
/// Selection background, highlights
pub const BG_HIGHLIGHT: Color = Color::Rgb(50, 55, 70);

// =============================================================================
// Text Hierarchy
// =============================================================================

/// Brightest text - ~98% cool white, for emphasis
pub const TEXT_BRIGHT: Color = Color::Rgb(250, 250, 255);
/// Main content text - 87% white
pub const TEXT_PRIMARY: Color = Color::Rgb(220, 220, 230);
/// Labels, metadata, secondary info
pub const TEXT_SECONDARY: Color = Color::Rgb(160, 160, 180);
/// Hints, disabled text, placeholders
pub const TEXT_MUTED: Color = Color::Rgb(100, 100, 120);
/// Decorative elements, separators
pub const TEXT_FAINT: Color = Color::Rgb(70, 70, 85);

// =============================================================================
// Accent Colors (use sparingly - 5% of UI)
// =============================================================================

/// Focus, selection, primary actions - soft blue
pub const ACCENT_PRIMARY: Color = Color::Rgb(130, 170, 255);
/// Secondary highlights - purple
pub const ACCENT_SECONDARY: Color = Color::Rgb(180, 140, 255);
/// Success states, confirmations
pub const ACCENT_SUCCESS: Color = Color::Rgb(130, 200, 140);
/// Warnings, processing states
pub const ACCENT_WARNING: Color = Color::Rgb(230, 180, 100);
/// Errors, destructive actions
pub const ACCENT_ERROR: Color = Color::Rgb(230, 120, 120);

// =============================================================================
// Agent-Specific Colors (brand identity)
// =============================================================================

/// Claude agent - softer cyan
pub const AGENT_CLAUDE: Color = Color::Rgb(130, 180, 220);
/// Codex agent - softer magenta
pub const AGENT_CODEX: Color = Color::Rgb(180, 140, 200);

// =============================================================================
// Border Colors
// =============================================================================

/// Default subtle border
pub const BORDER_DEFAULT: Color = Color::Rgb(50, 50, 65);
/// Focused element border
pub const BORDER_FOCUSED: Color = Color::Rgb(130, 170, 255);
/// Very subtle decorative border
pub const BORDER_DIMMED: Color = Color::Rgb(35, 35, 45);

// =============================================================================
// Legacy Aliases (for backward compatibility during migration)
// =============================================================================

/// Selection background (focused) - maps to BG_HIGHLIGHT
pub const SELECTED_BG: Color = BG_HIGHLIGHT;
/// Selection background (unfocused) - maps to BG_ELEVATED
pub const SELECTED_BG_DIM: Color = BG_ELEVATED;

/// Tab bar background - maps to BG_SURFACE
pub const TAB_BAR_BG: Color = BG_SURFACE;
/// Status bar background - maps to BG_SURFACE
pub const STATUS_BAR_BG: Color = BG_SURFACE;
/// Footer background - maps to BG_BASE
pub const FOOTER_BG: Color = BG_BASE;

/// Key hint background - maps to BG_ELEVATED
pub const KEY_HINT_BG: Color = BG_ELEVATED;
/// Input box background - maps to BG_SURFACE
pub const INPUT_BG: Color = BG_SURFACE;
