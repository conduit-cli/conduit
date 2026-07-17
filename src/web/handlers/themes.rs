//! Theme handlers for the Conduit web API.

use axum::Json;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::ui::components::theme::{
    current_theme, list_themes, load_theme_by_name, ThemeInfo, ThemeSource,
};
use crate::web::error::WebError;

#[derive(Debug, Clone, Copy)]
enum SyntaxSurfaceKind {
    MarkdownBlock,
    MarkdownInline,
    SourceFile,
}

/// Convert a ratatui Color to a CSS hex color string.
fn color_to_hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{:02x}{:02x}{:02x}", r, g, b),
        Color::Indexed(idx) => {
            // Convert ANSI 256-color index to approximate RGB
            let (r, g, b) = ansi_to_rgb(idx);
            format!("#{:02x}{:02x}{:02x}", r, g, b)
        }
        Color::Black => "#000000".to_string(),
        Color::Red => "#ff0000".to_string(),
        Color::Green => "#00ff00".to_string(),
        Color::Yellow => "#ffff00".to_string(),
        Color::Blue => "#0000ff".to_string(),
        Color::Magenta => "#ff00ff".to_string(),
        Color::Cyan => "#00ffff".to_string(),
        Color::Gray => "#808080".to_string(),
        Color::DarkGray => "#404040".to_string(),
        Color::LightRed => "#ff8080".to_string(),
        Color::LightGreen => "#80ff80".to_string(),
        Color::LightYellow => "#ffff80".to_string(),
        Color::LightBlue => "#8080ff".to_string(),
        Color::LightMagenta => "#ff80ff".to_string(),
        Color::LightCyan => "#80ffff".to_string(),
        Color::White => "#ffffff".to_string(),
        Color::Reset => "#ffffff".to_string(),
    }
}

/// Convert ANSI 256-color index to RGB.
fn ansi_to_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0..=15 => {
            // Standard colors
            match idx {
                0 => (0, 0, 0),
                1 => (128, 0, 0),
                2 => (0, 128, 0),
                3 => (128, 128, 0),
                4 => (0, 0, 128),
                5 => (128, 0, 128),
                6 => (0, 128, 128),
                7 => (192, 192, 192),
                8 => (128, 128, 128),
                9 => (255, 0, 0),
                10 => (0, 255, 0),
                11 => (255, 255, 0),
                12 => (0, 0, 255),
                13 => (255, 0, 255),
                14 => (0, 255, 255),
                15 => (255, 255, 255),
                _ => (0, 0, 0),
            }
        }
        16..=231 => {
            // 6x6x6 color cube
            let idx = idx - 16;
            let r = (idx / 36) % 6;
            let g = (idx / 6) % 6;
            let b = idx % 6;
            let to_val = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (to_val(r), to_val(g), to_val(b))
        }
        232..=255 => {
            // Grayscale
            let gray = 8 + (idx - 232) * 10;
            (gray, gray, gray)
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxTokenColorsResponse {
    pub comment: String,
    pub keyword: String,
    pub r#type: String,
    pub function: String,
    pub string: String,
    pub constant: String,
    pub property: String,
    pub parameter: String,
    pub punctuation: String,
    pub invalid: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxFontStylesResponse {
    pub keyword_bold: bool,
    pub type_bold: bool,
    pub invalid_underline: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxSurfaceResponse {
    pub foreground: String,
    pub background: String,
    pub caret: String,
    pub selection: String,
    pub selection_foreground: String,
    pub gutter: String,
    pub gutter_foreground: String,
    pub line_highlight: String,
    pub accent: String,
    pub tokens: SyntaxTokenColorsResponse,
    pub font_styles: SyntaxFontStylesResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSyntaxResponse {
    pub markdown_block: SyntaxSurfaceResponse,
    pub markdown_inline: SyntaxSurfaceResponse,
    pub source_file: SyntaxSurfaceResponse,
}

/// Response for theme colors.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColorsResponse {
    // Background layers
    pub bg_terminal: String,
    pub bg_base: String,
    pub bg_surface: String,
    pub bg_elevated: String,
    pub bg_highlight: String,
    pub markdown_code_bg: String,
    pub markdown_inline_code_bg: String,

    // Text hierarchy
    pub text_bright: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub text_muted: String,
    pub text_faint: String,

    // Accent colors
    pub accent_primary: String,
    pub accent_secondary: String,
    pub accent_success: String,
    pub accent_warning: String,
    pub accent_error: String,

    // Agent colors
    pub agent_claude: String,
    pub agent_codex: String,

    // PR state colors
    pub pr_open_bg: String,
    pub pr_merged_bg: String,
    pub pr_closed_bg: String,
    pub pr_draft_bg: String,
    pub pr_unknown_bg: String,

    // Border colors
    pub border_default: String,
    pub border_focused: String,
    pub border_dimmed: String,

    // Diff colors
    pub diff_add: String,
    pub diff_remove: String,
}

/// Response for a single theme.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeResponse {
    pub name: String,
    pub display_name: String,
    pub is_light: bool,
    pub colors: ThemeColorsResponse,
    pub syntax: ThemeSyntaxResponse,
}

/// Response for theme info (without colors).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeInfoResponse {
    pub name: String,
    pub display_name: String,
    pub is_light: bool,
    pub source: String,
}

impl From<ThemeInfo> for ThemeInfoResponse {
    fn from(info: ThemeInfo) -> Self {
        let source = match info.source {
            ThemeSource::Builtin => "builtin".to_string(),
            ThemeSource::VsCodeExtension { .. } => "vscode".to_string(),
            ThemeSource::ConduitToml { .. } => "toml".to_string(),
            ThemeSource::CustomPath { .. } => "custom".to_string(),
        };
        Self {
            name: info.name,
            display_name: info.display_name,
            is_light: info.is_light,
            source,
        }
    }
}

/// Response for listing themes.
#[derive(Debug, Serialize)]
pub struct ListThemesResponse {
    pub themes: Vec<ThemeInfoResponse>,
    pub current: String,
}

/// Request to set the current theme.
#[derive(Debug, Deserialize)]
pub struct SetThemeRequest {
    pub name: String,
}

/// List all available themes.
pub async fn list_available_themes() -> Json<ListThemesResponse> {
    let themes = list_themes();
    let current = current_theme().name.clone();

    Json(ListThemesResponse {
        themes: themes.into_iter().map(ThemeInfoResponse::from).collect(),
        current,
    })
}

/// Get the current theme with all colors.
pub async fn get_current_theme() -> Json<ThemeResponse> {
    let theme = current_theme();

    Json(ThemeResponse {
        name: theme.name.clone(),
        display_name: theme.name.clone(),
        is_light: theme.is_light,
        colors: ThemeColorsResponse {
            bg_terminal: color_to_hex(theme.bg_terminal),
            bg_base: color_to_hex(theme.bg_base),
            bg_surface: color_to_hex(theme.bg_surface),
            bg_elevated: color_to_hex(theme.bg_elevated),
            bg_highlight: color_to_hex(theme.bg_highlight),
            markdown_code_bg: color_to_hex(theme.markdown_code_bg),
            markdown_inline_code_bg: color_to_hex(theme.markdown_inline_code_bg),

            text_bright: color_to_hex(theme.text_bright),
            text_primary: color_to_hex(theme.text_primary),
            text_secondary: color_to_hex(theme.text_secondary),
            text_muted: color_to_hex(theme.text_muted),
            text_faint: color_to_hex(theme.text_faint),

            accent_primary: color_to_hex(theme.accent_primary),
            accent_secondary: color_to_hex(theme.accent_secondary),
            accent_success: color_to_hex(theme.accent_success),
            accent_warning: color_to_hex(theme.accent_warning),
            accent_error: color_to_hex(theme.accent_error),

            agent_claude: color_to_hex(theme.agent_claude),
            agent_codex: color_to_hex(theme.agent_codex),

            pr_open_bg: color_to_hex(theme.pr_open_bg),
            pr_merged_bg: color_to_hex(theme.pr_merged_bg),
            pr_closed_bg: color_to_hex(theme.pr_closed_bg),
            pr_draft_bg: color_to_hex(theme.pr_draft_bg),
            pr_unknown_bg: color_to_hex(theme.pr_unknown_bg),

            border_default: color_to_hex(theme.border_default),
            border_focused: color_to_hex(theme.border_focused),
            border_dimmed: color_to_hex(theme.border_dimmed),

            diff_add: color_to_hex(theme.diff_add),
            diff_remove: color_to_hex(theme.diff_remove),
        },
        syntax: ThemeSyntaxResponse {
            markdown_block: build_syntax_surface(&theme, SyntaxSurfaceKind::MarkdownBlock),
            markdown_inline: build_syntax_surface(&theme, SyntaxSurfaceKind::MarkdownInline),
            source_file: build_syntax_surface(&theme, SyntaxSurfaceKind::SourceFile),
        },
    })
}

/// Set the current theme by name.
pub async fn set_current_theme(
    Json(req): Json<SetThemeRequest>,
) -> Result<Json<ThemeResponse>, WebError> {
    if !load_theme_by_name(&req.name) {
        return Err(WebError::NotFound(format!(
            "Theme '{}' not found",
            req.name
        )));
    }

    Ok(get_current_theme().await)
}

fn build_syntax_surface(
    theme: &crate::ui::components::Theme,
    surface: SyntaxSurfaceKind,
) -> SyntaxSurfaceResponse {
    let (foreground, emphasize_functions) = match surface {
        SyntaxSurfaceKind::MarkdownBlock => (theme.text_primary, true),
        SyntaxSurfaceKind::MarkdownInline => (theme.text_bright, true),
        SyntaxSurfaceKind::SourceFile => (theme.text_primary, false),
    };

    SyntaxSurfaceResponse {
        foreground: color_to_hex(foreground),
        background: color_to_hex(theme.bg_base),
        caret: color_to_hex(theme.accent_primary),
        selection: color_to_hex(theme.bg_highlight),
        selection_foreground: color_to_hex(theme.text_bright),
        gutter: color_to_hex(theme.bg_surface),
        gutter_foreground: color_to_hex(theme.text_muted),
        line_highlight: color_to_hex(theme.bg_highlight),
        accent: color_to_hex(theme.accent_secondary),
        tokens: SyntaxTokenColorsResponse {
            comment: color_to_hex(theme.text_muted),
            keyword: color_to_hex(theme.accent_primary),
            r#type: color_to_hex(theme.accent_secondary),
            function: color_to_hex(if emphasize_functions {
                theme.text_bright
            } else {
                theme.accent_secondary
            }),
            string: color_to_hex(theme.accent_success),
            constant: color_to_hex(theme.accent_warning),
            property: color_to_hex(theme.accent_secondary),
            parameter: color_to_hex(theme.text_bright),
            punctuation: color_to_hex(theme.text_secondary),
            invalid: color_to_hex(theme.accent_error),
        },
        font_styles: SyntaxFontStylesResponse {
            keyword_bold: true,
            type_bold: true,
            invalid_underline: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::theme::{set_theme, Theme};

    struct ThemeReset(Theme, #[allow(dead_code)] parking_lot::MutexGuard<'static, ()>);

    impl Drop for ThemeReset {
        fn drop(&mut self) {
            set_theme(self.0.clone());
        }
    }

    fn preserve_theme() -> ThemeReset {
        // Hold the shared guard so theme-mutating tests don't race on global state.
        let guard = crate::ui::components::theme::THEME_TEST_GUARD.lock();
        ThemeReset(current_theme().clone(), guard)
    }

    #[tokio::test]
    async fn get_current_theme_includes_syntax_surfaces() {
        let _reset = preserve_theme();
        set_theme(Theme::default_dark());

        let Json(theme) = get_current_theme().await;

        assert_eq!(
            theme.syntax.markdown_block.tokens.keyword,
            color_to_hex(Theme::default_dark().accent_primary)
        );
        assert_eq!(
            theme.syntax.markdown_inline.tokens.function,
            color_to_hex(Theme::default_dark().text_bright)
        );
        assert_eq!(
            theme.syntax.source_file.tokens.function,
            color_to_hex(Theme::default_dark().accent_secondary)
        );
    }

    #[tokio::test]
    async fn set_current_theme_returns_syntax_payload() {
        let _reset = preserve_theme();

        let Json(theme) = set_current_theme(Json(SetThemeRequest {
            name: "default-light".to_string(),
        }))
        .await
        .expect("theme should load");

        let expected = Theme::default_light();
        assert_eq!(theme.name, expected.name);
        assert_eq!(
            theme.syntax.markdown_block.background,
            color_to_hex(expected.bg_base)
        );
        assert!(theme.syntax.source_file.font_styles.type_bold);
    }
}
