//! Runtime-switchable theme system for TUI colors.
//!
//! Provides lock-free reads for the common case (main thread rendering)
//! with thread-safe writes for theme switching.
//!
//! # Usage
//!
//! ```rust,ignore
//! use crate::ui::components::theme::{text_primary, bg_base, set_theme};
//!
//! // Read colors (fast, called thousands of times per frame)
//! let style = Style::default().fg(text_primary()).bg(bg_base());
//!
//! // Change theme (rare, user-initiated)
//! set_theme(Theme::default_light());
//! ```

mod builtin;
mod colors;
pub mod migrate;
mod registry;
pub mod toml;
mod types;
mod vscode;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use parking_lot::RwLock;
use ratatui::style::Color;

pub use colors::{
    boost_brightness, contrast_ratio, darken, desaturate, dim, ensure_contrast_bg,
    ensure_contrast_fg, interpolate, lighten, parse_hex_color, relative_luminance, saturate,
    shift_hue,
};
pub use registry::ThemeRegistry;
pub use types::{Theme, ThemeInfo, ThemeSource};
// VsCodeTheme and VsCodeThemeError are available but typically used internally

// =============================================================================
// Global Theme Storage
// =============================================================================

/// Global theme storage with fast read access.
static THEME: OnceLock<RwLock<Theme>> = OnceLock::new();
static THEME_REVISION: AtomicU64 = AtomicU64::new(0);

/// Global theme registry for discovery.
static REGISTRY: OnceLock<RwLock<ThemeRegistry>> = OnceLock::new();

fn theme_lock() -> &'static RwLock<Theme> {
    THEME.get_or_init(|| RwLock::new(Theme::default()))
}

fn registry_lock() -> &'static RwLock<ThemeRegistry> {
    REGISTRY.get_or_init(|| RwLock::new(ThemeRegistry::new()))
}

// =============================================================================
// Theme Management API
// =============================================================================

/// Get a read guard to the current theme.
///
/// This is extremely fast for uncontended reads (just an atomic load).
#[inline]
pub fn current_theme() -> parking_lot::RwLockReadGuard<'static, Theme> {
    theme_lock().read()
}

/// Set a new theme. Takes effect on the next render.
pub fn set_theme(theme: Theme) {
    let theme = normalize_theme(theme);
    *theme_lock().write() = theme;
    THEME_REVISION.fetch_add(1, Ordering::Relaxed);
}

/// Initialize the theme system with a theme name from config.
///
/// Should be called once at startup.
pub fn init_theme(name: Option<&str>, custom_path: Option<&Path>) {
    let registry = registry_lock().read();

    let theme = if let Some(path) = custom_path {
        // Try custom path first
        registry.load_from_path(path)
    } else if let Some(name) = name {
        // Try loading by name
        registry.load_theme(name)
    } else {
        None
    };

    // Fall back to default dark
    let theme = theme.unwrap_or_else(Theme::default_dark);
    set_theme(theme);
}

/// Load and apply a theme by name.
pub fn load_theme_by_name(name: &str) -> bool {
    let start = Instant::now();
    let registry = registry_lock().read();
    if let Some(theme) = registry.load_theme(name) {
        let theme_name = theme.name.clone();
        let is_light = theme.is_light;
        set_theme(theme);
        tracing::info!(
            requested = name,
            applied = %theme_name,
            is_light,
            elapsed_ms = start.elapsed().as_millis(),
            "Theme applied by name"
        );
        true
    } else {
        tracing::info!(
            requested = name,
            elapsed_ms = start.elapsed().as_millis(),
            "Theme not found"
        );
        false
    }
}

/// Load and apply a theme from a file path.
pub fn load_theme_from_path(path: &Path) -> bool {
    let start = Instant::now();
    let registry = registry_lock().read();
    if let Some(theme) = registry.load_from_path(path) {
        let theme_name = theme.name.clone();
        let is_light = theme.is_light;
        set_theme(theme);
        tracing::info!(
            path = %path.display(),
            applied = %theme_name,
            is_light,
            elapsed_ms = start.elapsed().as_millis(),
            "Theme applied from path"
        );
        true
    } else {
        tracing::info!(
            path = %path.display(),
            elapsed_ms = start.elapsed().as_millis(),
            "Theme not found at path"
        );
        false
    }
}

/// Toggle between light and dark themes.
pub fn toggle_theme() {
    let is_light = current_theme().is_light;
    if is_light {
        set_theme(Theme::default_dark());
    } else {
        set_theme(Theme::default_light());
    }
}

/// List all available themes.
pub fn list_themes() -> Vec<ThemeInfo> {
    registry_lock().read().list_themes()
}

/// Get the current theme name.
pub fn current_theme_name() -> String {
    current_theme().name.clone()
}

/// Returns a monotonically increasing revision for theme changes.
pub fn theme_revision() -> u64 {
    THEME_REVISION.load(Ordering::Relaxed)
}

fn normalize_theme(mut theme: Theme) -> Theme {
    let base = theme.bg_base;
    let is_light = theme.is_light;

    // Text contrast against base background.
    let text_muted_min = if is_light { 3.6 } else { 3.0 };
    let text_faint_min = if is_light { 2.8 } else { 2.2 };
    theme.text_bright = ensure_contrast_fg(theme.text_bright, base, 4.5);
    theme.text_primary = ensure_contrast_fg(theme.text_primary, base, 4.5);
    theme.text_secondary = ensure_contrast_fg(theme.text_secondary, base, 3.0);
    theme.text_muted = ensure_contrast_fg(theme.text_muted, base, text_muted_min);
    theme.text_faint = ensure_contrast_fg(theme.text_faint, base, text_faint_min);

    // Layered backgrounds should separate enough to be visible.
    let highlight_min = if is_light { 2.6 } else { 2.0 };
    theme.bg_surface = ensure_contrast_bg(theme.bg_surface, base, 1.2);
    theme.bg_elevated = ensure_contrast_bg(theme.bg_elevated, theme.bg_surface, 1.2);
    theme.bg_highlight = ensure_contrast_bg(theme.bg_highlight, base, highlight_min);

    // Light themes need extra separation for code/tool surfaces.
    if is_light {
        theme.tool_block_bg = ensure_contrast_bg(theme.tool_block_bg, base, 2.4);
        theme.markdown_code_bg = ensure_contrast_bg(theme.markdown_code_bg, base, 2.0);
        theme.markdown_inline_code_bg =
            ensure_contrast_bg(theme.markdown_inline_code_bg, base, 2.0);
    }

    // Border contrast for outlines and separators.
    theme.border_default = ensure_contrast_fg(theme.border_default, base, 1.8);
    theme.border_dimmed = ensure_contrast_fg(theme.border_dimmed, base, 1.5);

    theme
}

/// Refresh theme discovery (re-scan VS Code extensions).
pub fn refresh_themes() {
    registry_lock().write().discover_vscode_themes();
}

// =============================================================================
// Color Accessor Functions
// =============================================================================
// These functions replace the old constants. They have the same semantic
// meaning but now read from the current theme.

// Background Layers
#[inline]
pub fn bg_terminal() -> Color {
    current_theme().bg_terminal
}
#[inline]
pub fn bg_base() -> Color {
    current_theme().bg_base
}
#[inline]
pub fn bg_surface() -> Color {
    current_theme().bg_surface
}
#[inline]
pub fn bg_elevated() -> Color {
    current_theme().bg_elevated
}
#[inline]
pub fn bg_highlight() -> Color {
    current_theme().bg_highlight
}
#[inline]
pub fn markdown_code_bg() -> Color {
    current_theme().markdown_code_bg
}
#[inline]
pub fn markdown_inline_code_bg() -> Color {
    current_theme().markdown_inline_code_bg
}

// Text Hierarchy
#[inline]
pub fn text_bright() -> Color {
    current_theme().text_bright
}
#[inline]
pub fn text_primary() -> Color {
    current_theme().text_primary
}
#[inline]
pub fn text_secondary() -> Color {
    current_theme().text_secondary
}
#[inline]
pub fn text_muted() -> Color {
    current_theme().text_muted
}
#[inline]
pub fn text_faint() -> Color {
    current_theme().text_faint
}

// Accent Colors
#[inline]
pub fn accent_primary() -> Color {
    current_theme().accent_primary
}
#[inline]
pub fn accent_secondary() -> Color {
    current_theme().accent_secondary
}
#[inline]
pub fn accent_success() -> Color {
    current_theme().accent_success
}
#[inline]
pub fn accent_warning() -> Color {
    current_theme().accent_warning
}
#[inline]
pub fn accent_error() -> Color {
    current_theme().accent_error
}

// Agent Colors
#[inline]
pub fn agent_claude() -> Color {
    current_theme().agent_claude
}
#[inline]
pub fn agent_codex() -> Color {
    current_theme().agent_codex
}
#[inline]
pub fn agent_gemini() -> Color {
    current_theme().agent_codex
}

// PR State Colors
#[inline]
pub fn pr_open_bg() -> Color {
    current_theme().pr_open_bg
}
#[inline]
pub fn pr_merged_bg() -> Color {
    current_theme().pr_merged_bg
}
#[inline]
pub fn pr_closed_bg() -> Color {
    current_theme().pr_closed_bg
}
#[inline]
pub fn pr_draft_bg() -> Color {
    current_theme().pr_draft_bg
}
#[inline]
pub fn pr_unknown_bg() -> Color {
    current_theme().pr_unknown_bg
}

// Spinner Colors
#[inline]
pub fn spinner_active() -> Color {
    current_theme().spinner_active
}
#[inline]
pub fn spinner_trail_1() -> Color {
    current_theme().spinner_trail_1
}
#[inline]
pub fn spinner_trail_2() -> Color {
    current_theme().spinner_trail_2
}
#[inline]
pub fn spinner_trail_3() -> Color {
    current_theme().spinner_trail_3
}
#[inline]
pub fn spinner_trail_4() -> Color {
    current_theme().spinner_trail_4
}
#[inline]
pub fn spinner_trail_5() -> Color {
    current_theme().spinner_trail_5
}
#[inline]
pub fn spinner_inactive() -> Color {
    current_theme().spinner_inactive
}

// Border Colors
#[inline]
pub fn border_default() -> Color {
    current_theme().border_default
}
#[inline]
pub fn border_focused() -> Color {
    current_theme().border_focused
}
#[inline]
pub fn border_dimmed() -> Color {
    current_theme().border_dimmed
}

// Logo Shine Colors
#[inline]
pub fn shine_edge() -> Color {
    current_theme().shine_edge
}
#[inline]
pub fn shine_mid() -> Color {
    current_theme().shine_mid
}
#[inline]
pub fn shine_center() -> Color {
    current_theme().shine_center
}
#[inline]
pub fn shine_peak() -> Color {
    current_theme().shine_peak
}

// Tool Block Colors
#[inline]
pub fn tool_block_bg() -> Color {
    current_theme().tool_block_bg
}
#[inline]
pub fn tool_comment() -> Color {
    current_theme().tool_comment
}
#[inline]
pub fn tool_command() -> Color {
    current_theme().tool_command
}
#[inline]
pub fn tool_output() -> Color {
    current_theme().tool_output
}
#[inline]
pub fn diff_add() -> Color {
    current_theme().diff_add
}
#[inline]
pub fn diff_remove() -> Color {
    current_theme().diff_remove
}

// =============================================================================
// Legacy Aliases (backward compatibility)
// =============================================================================

/// Selection background (focused) - maps to bg_highlight
#[inline]
pub fn selected_bg() -> Color {
    bg_highlight()
}

/// Selection background (unfocused) - maps to bg_elevated
#[inline]
pub fn selected_bg_dim() -> Color {
    bg_elevated()
}

/// Tab bar background - maps to bg_surface
#[inline]
pub fn tab_bar_bg() -> Color {
    bg_surface()
}

/// Status bar background - maps to bg_surface
#[inline]
pub fn status_bar_bg() -> Color {
    bg_surface()
}

/// Sidebar background - prefers bg_surface, with a subtle fallback if equal to bg_base.
#[inline]
pub fn sidebar_bg() -> Color {
    let base = bg_base();
    let surface = bg_surface();
    if surface == base {
        if current_theme().is_light {
            darken(base, 0.04)
        } else {
            lighten(base, 0.08)
        }
    } else {
        surface
    }
}

/// Dialog background - maps to bg_elevated
#[inline]
pub fn dialog_bg() -> Color {
    bg_elevated()
}

/// Footer background - maps to bg_base
#[inline]
pub fn footer_bg() -> Color {
    bg_base()
}

/// Key hint background - maps to bg_elevated
#[inline]
pub fn key_hint_bg() -> Color {
    bg_elevated()
}

/// Input box background - maps to bg_surface
#[inline]
pub fn input_bg() -> Color {
    bg_surface()
}
