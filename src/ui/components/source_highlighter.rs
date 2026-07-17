use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle, ScopeSelectors, StyleModifier, Theme as SyntectTheme,
    ThemeItem, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use two_face::syntax;
use unicode_width::UnicodeWidthChar;

use crate::ui::file_viewer::FileKind;

use super::theme::theme_revision;
use super::{current_theme, text_bright, text_primary, text_secondary, RawEventEntry};

#[derive(Clone, Copy)]
enum CodeSurfaceKind {
    MarkdownBlock,
    MarkdownInline,
    SourceFile,
}

struct CachedThemes {
    revision: u64,
    markdown_block: Arc<SyntectTheme>,
    markdown_inline: Arc<SyntectTheme>,
    source_file: Arc<SyntectTheme>,
}

pub fn highlight_source_lines(
    file_kind: FileKind,
    file_path: &Path,
    lines: &[String],
) -> Vec<Line<'static>> {
    let syntax_set = syntax_set();
    let syntax = resolve_file_syntax(file_kind, file_path, syntax_set);
    match highlight_lines_with_syntect(lines, syntax, CodeSurfaceKind::SourceFile) {
        Some(highlighted) => highlighted,
        None => lines
            .iter()
            .map(|line| Line::from(fallback_highlight_line(file_kind, line)))
            .collect(),
    }
}

pub fn highlight_markdown_code_block(
    language_hint: Option<&str>,
    code: &str,
) -> Vec<Line<'static>> {
    let lines: Vec<String> = if code.is_empty() {
        vec![String::new()]
    } else {
        code.lines().map(String::from).collect()
    };

    let syntax_set = syntax_set();
    let syntax = resolve_markdown_syntax(language_hint, syntax_set);
    let highlighted = highlight_lines_with_syntect(&lines, syntax, CodeSurfaceKind::MarkdownBlock)
        .unwrap_or_else(|| {
            lines
                .iter()
                .map(|line| Line::from(fallback_markdown_spans(line)))
                .collect()
        });

    highlighted.into_iter().map(pad_code_line).collect()
}

pub fn highlight_inline_code(code: &str) -> Vec<Span<'static>> {
    let syntax_set = syntax_set();
    let syntax = syntax_set.find_syntax_plain_text();
    let lines = vec![code.to_string()];

    let spans = highlight_lines_with_syntect(&lines, syntax, CodeSurfaceKind::MarkdownInline)
        .and_then(|mut lines| lines.pop().map(|line| line.spans))
        .unwrap_or_else(|| fallback_markdown_spans(code));

    let inline_style = Style::default().fg(text_secondary());
    let mut wrapped = Vec::with_capacity(spans.len() + 2);
    wrapped.push(Span::styled("`", inline_style));
    wrapped.extend(spans);
    wrapped.push(Span::styled("`", inline_style));
    wrapped
}

pub fn truncate_spans_with_ellipsis(
    spans: &[Span<'static>],
    max_width: usize,
) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let (full, truncated) = collect_spans_to_width(spans, max_width);
    if !truncated {
        return full;
    }

    if max_width == 1 {
        return vec![Span::styled("…", Style::default().fg(text_primary()))];
    }

    let (mut clipped, _) = collect_spans_to_width(spans, max_width.saturating_sub(1));
    push_char_with_style(&mut clipped, '…', Style::default().fg(text_primary()));
    clipped
}

fn highlight_lines_with_syntect(
    lines: &[String],
    syntax: &SyntaxReference,
    surface: CodeSurfaceKind,
) -> Option<Vec<Line<'static>>> {
    let syntax_set = syntax_set();
    let theme = theme_for(surface);
    let mut highlighter = HighlightLines::new(syntax, &theme);

    let mut highlighted = Vec::with_capacity(lines.len());
    for line in lines {
        let mut with_newline = String::with_capacity(line.len() + 1);
        with_newline.push_str(line);
        with_newline.push('\n');

        let ranges = highlighter.highlight_line(&with_newline, syntax_set).ok()?;
        let mut spans = Vec::new();
        for (style, text) in ranges {
            let text = text.strip_suffix('\n').unwrap_or(text);
            if text.is_empty() {
                continue;
            }
            spans.push(Span::styled(
                text.to_string(),
                style_to_ratatui(style, matches!(surface, CodeSurfaceKind::SourceFile)),
            ));
        }

        if spans.is_empty() {
            highlighted.push(Line::from(""));
        } else {
            highlighted.push(Line::from(spans));
        }
    }

    Some(highlighted)
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(syntax::extra_newlines)
}

fn theme_for(surface: CodeSurfaceKind) -> Arc<SyntectTheme> {
    let revision = theme_revision();
    let cache = theme_cache();
    {
        let read = cache.read();
        if let Some(cached) = read.as_ref() {
            if cached.revision == revision {
                return match surface {
                    CodeSurfaceKind::MarkdownBlock => Arc::clone(&cached.markdown_block),
                    CodeSurfaceKind::MarkdownInline => Arc::clone(&cached.markdown_inline),
                    CodeSurfaceKind::SourceFile => Arc::clone(&cached.source_file),
                };
            }
        }
    }

    let mut write = cache.write();
    if write.as_ref().map(|cached| cached.revision) != Some(revision) {
        *write = Some(build_cached_themes(revision));
    }

    let cached = write.as_ref().expect("theme cache initialized");
    match surface {
        CodeSurfaceKind::MarkdownBlock => Arc::clone(&cached.markdown_block),
        CodeSurfaceKind::MarkdownInline => Arc::clone(&cached.markdown_inline),
        CodeSurfaceKind::SourceFile => Arc::clone(&cached.source_file),
    }
}

fn theme_cache() -> &'static RwLock<Option<CachedThemes>> {
    static THEME_CACHE: OnceLock<RwLock<Option<CachedThemes>>> = OnceLock::new();
    THEME_CACHE.get_or_init(|| RwLock::new(None))
}

fn build_cached_themes(revision: u64) -> CachedThemes {
    let theme = current_theme().clone();
    CachedThemes {
        revision,
        markdown_block: Arc::new(build_syntect_theme(
            &theme,
            theme.bg_base,
            text_primary(),
            true,
        )),
        markdown_inline: Arc::new(build_syntect_theme(
            &theme,
            theme.bg_base,
            text_bright(),
            true,
        )),
        source_file: Arc::new(build_syntect_theme(
            &theme,
            theme.bg_base,
            theme.text_primary,
            false,
        )),
    }
}

fn build_syntect_theme(
    theme: &crate::ui::components::Theme,
    background: Color,
    foreground: Color,
    emphasize_functions: bool,
) -> SyntectTheme {
    SyntectTheme {
        name: Some(format!("{} syntax", theme.name)),
        settings: ThemeSettings {
            foreground: Some(to_syntect_color(foreground)),
            background: Some(to_syntect_color(background)),
            caret: Some(to_syntect_color(theme.accent_primary)),
            selection: Some(to_syntect_color(theme.bg_highlight)),
            selection_foreground: Some(to_syntect_color(theme.text_bright)),
            gutter: Some(to_syntect_color(theme.bg_surface)),
            gutter_foreground: Some(to_syntect_color(theme.text_muted)),
            line_highlight: Some(to_syntect_color(theme.bg_highlight)),
            accent: Some(to_syntect_color(theme.accent_secondary)),
            ..ThemeSettings::default()
        },
        scopes: vec![
            theme_item("comment, punctuation.definition.comment", theme.text_muted, None),
            theme_item(
                "keyword, keyword.control, keyword.operator.word, storage, storage.modifier",
                theme.accent_primary,
                Some(FontStyle::BOLD),
            ),
            theme_item(
                "storage.type, entity.name.type, entity.name.class, entity.name.struct, support.type, support.class",
                theme.accent_secondary,
                Some(FontStyle::BOLD),
            ),
            theme_item(
                "entity.name.function, meta.function-call, support.function, variable.function",
                if emphasize_functions {
                    theme.text_bright
                } else {
                    theme.accent_secondary
                },
                None,
            ),
            theme_item(
                "string, string.regexp, string.quoted, punctuation.definition.string",
                theme.accent_success,
                None,
            ),
            theme_item(
                "constant.numeric, constant.language, constant.character.escape, constant.other",
                theme.accent_warning,
                None,
            ),
            theme_item(
                "entity.name.tag, entity.other.attribute-name, support.type.property-name, variable.other.member",
                theme.accent_secondary,
                None,
            ),
            theme_item("variable.parameter", theme.text_bright, None),
            theme_item(
                "punctuation, meta.brace, meta.delimiter, meta.separator",
                theme.text_secondary,
                None,
            ),
            theme_item("invalid, invalid.illegal", theme.accent_error, Some(FontStyle::UNDERLINE)),
        ],
        ..SyntectTheme::default()
    }
}

fn theme_item(scope: &str, foreground: Color, font_style: Option<FontStyle>) -> ThemeItem {
    ThemeItem {
        scope: ScopeSelectors::from_str(scope).expect("valid static syntect selector"),
        style: StyleModifier {
            foreground: Some(to_syntect_color(foreground)),
            background: None,
            font_style,
        },
    }
}

fn resolve_markdown_syntax<'a>(
    language_hint: Option<&str>,
    syntax_set: &'a SyntaxSet,
) -> &'a SyntaxReference {
    language_hint
        .and_then(normalize_language_hint)
        .and_then(|hint| syntax_set.find_syntax_by_token(&hint))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text())
}

fn resolve_file_syntax<'a>(
    file_kind: FileKind,
    file_path: &Path,
    syntax_set: &'a SyntaxSet,
) -> &'a SyntaxReference {
    if let Some(ext) = file_path.extension().and_then(|ext| ext.to_str()) {
        if let Some(syntax) = syntax_set.find_syntax_by_extension(ext) {
            return syntax;
        }
    }

    let by_name = match file_kind {
        FileKind::Markdown => syntax_set.find_syntax_by_name("Markdown"),
        FileKind::Json => syntax_set.find_syntax_by_name("JSON"),
        FileKind::Rust => syntax_set.find_syntax_by_name("Rust"),
        FileKind::Toml => syntax_set.find_syntax_by_name("TOML"),
        FileKind::Yaml => syntax_set.find_syntax_by_name("YAML"),
        FileKind::PlainText => None,
    };

    by_name.unwrap_or_else(|| syntax_set.find_syntax_plain_text())
}

fn normalize_language_hint(language_hint: &str) -> Option<String> {
    let normalized = language_hint
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches('{')
        .trim_matches('}')
        .trim()
        .to_ascii_lowercase();

    if normalized.is_empty() {
        return None;
    }

    let normalized = match normalized.as_str() {
        "rs" => "rust",
        "js" => "javascript",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "jsx" => "javascript",
        "sh" | "shell" | "zsh" | "bash" | "console" => "bash",
        "yml" => "yaml",
        "md" => "markdown",
        "py" => "python",
        "rb" => "ruby",
        "docker" => "dockerfile",
        _ => normalized.as_str(),
    };

    Some(normalized.to_string())
}

fn style_to_ratatui(style: syntect::highlighting::Style, include_background: bool) -> Style {
    let mut mapped = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));

    if include_background {
        mapped = mapped.bg(Color::Rgb(
            style.background.r,
            style.background.g,
            style.background.b,
        ));
    }

    if style.font_style.contains(FontStyle::BOLD) {
        mapped = mapped.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        mapped = mapped.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        mapped = mapped.add_modifier(Modifier::UNDERLINED);
    }

    mapped
}

fn fallback_highlight_line(file_kind: FileKind, line: &str) -> Vec<Span<'static>> {
    match file_kind {
        FileKind::Json => RawEventEntry::highlight_json_line(line),
        _ => vec![Span::styled(
            line.to_string(),
            Style::default().fg(text_primary()),
        )],
    }
}

fn fallback_markdown_spans(line: &str) -> Vec<Span<'static>> {
    vec![Span::styled(
        line.to_string(),
        Style::default().fg(text_primary()),
    )]
}

fn pad_code_line(line: Line<'static>) -> Line<'static> {
    line
}

fn to_syntect_color(color: Color) -> SyntectColor {
    match color {
        Color::Rgb(r, g, b) => SyntectColor { r, g, b, a: 0xFF },
        Color::Black => SyntectColor {
            r: 0,
            g: 0,
            b: 0,
            a: 0xFF,
        },
        Color::Red => SyntectColor {
            r: 205,
            g: 49,
            b: 49,
            a: 0xFF,
        },
        Color::Green => SyntectColor {
            r: 13,
            g: 188,
            b: 121,
            a: 0xFF,
        },
        Color::Yellow => SyntectColor {
            r: 229,
            g: 229,
            b: 16,
            a: 0xFF,
        },
        Color::Blue => SyntectColor {
            r: 36,
            g: 114,
            b: 200,
            a: 0xFF,
        },
        Color::Magenta => SyntectColor {
            r: 188,
            g: 63,
            b: 188,
            a: 0xFF,
        },
        Color::Cyan => SyntectColor {
            r: 17,
            g: 168,
            b: 205,
            a: 0xFF,
        },
        Color::Gray => SyntectColor {
            r: 204,
            g: 204,
            b: 204,
            a: 0xFF,
        },
        Color::DarkGray => SyntectColor {
            r: 118,
            g: 118,
            b: 118,
            a: 0xFF,
        },
        Color::LightRed => SyntectColor {
            r: 241,
            g: 76,
            b: 76,
            a: 0xFF,
        },
        Color::LightGreen => SyntectColor {
            r: 35,
            g: 209,
            b: 139,
            a: 0xFF,
        },
        Color::LightYellow => SyntectColor {
            r: 245,
            g: 245,
            b: 67,
            a: 0xFF,
        },
        Color::LightBlue => SyntectColor {
            r: 59,
            g: 142,
            b: 234,
            a: 0xFF,
        },
        Color::LightMagenta => SyntectColor {
            r: 214,
            g: 112,
            b: 214,
            a: 0xFF,
        },
        Color::LightCyan => SyntectColor {
            r: 41,
            g: 184,
            b: 219,
            a: 0xFF,
        },
        Color::White => SyntectColor {
            r: 255,
            g: 255,
            b: 255,
            a: 0xFF,
        },
        Color::Indexed(value) => SyntectColor {
            r: value,
            g: value,
            b: value,
            a: 0xFF,
        },
        Color::Reset => SyntectColor {
            r: 220,
            g: 220,
            b: 230,
            a: 0xFF,
        },
    }
}

fn collect_spans_to_width(spans: &[Span<'static>], max_width: usize) -> (Vec<Span<'static>>, bool) {
    let mut out = Vec::new();
    let mut width = 0usize;

    for span in spans {
        for ch in span.content.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + ch_width > max_width {
                return (out, true);
            }
            push_char_with_style(&mut out, ch, span.style);
            width += ch_width;
        }
    }

    (out, false)
}

fn push_char_with_style(spans: &mut Vec<Span<'static>>, ch: char, style: Style) {
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push(ch);
            return;
        }
    }

    spans.push(Span::styled(ch.to_string(), style));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::{set_theme, Theme};

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

    #[test]
    fn test_highlight_source_lines_json_returns_styled_segments() {
        let lines = vec!["{\"key\": true, \"n\": 1}".to_string()];
        let highlighted = highlight_source_lines(FileKind::Json, Path::new("sample.json"), &lines);

        assert_eq!(highlighted.len(), 1);
        assert!(!highlighted[0].spans.is_empty());
        assert!(highlighted[0].spans.len() > 1);
    }

    #[test]
    fn test_highlight_source_lines_rust_returns_styled_segments() {
        let lines = vec!["pub fn main() { let x = 1; }".to_string()];
        let highlighted = highlight_source_lines(FileKind::Rust, Path::new("main.rs"), &lines);

        assert_eq!(highlighted.len(), 1);
        assert!(!highlighted[0].spans.is_empty());
    }

    #[test]
    fn test_highlight_markdown_code_block_sets_theme_background() {
        let lines = highlight_markdown_code_block(Some("rust"), "fn main() { let x = 1; }");

        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.iter().all(|span| span.style.bg.is_none()));
        assert!(lines[0].spans.len() > 2);
    }

    #[test]
    fn test_highlight_inline_code_wraps_with_inline_background() {
        let spans = highlight_inline_code("cargo check --all");

        assert!(spans.iter().all(|span| span.style.bg.is_none()));
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "`cargo check --all`");
    }

    #[test]
    fn test_normalize_language_hint_aliases() {
        assert_eq!(normalize_language_hint("rs"), Some("rust".to_string()));
        assert_eq!(normalize_language_hint(" yml "), Some("yaml".to_string()));
        assert_eq!(
            normalize_language_hint("tsx"),
            Some("typescriptreact".to_string())
        );
    }

    #[test]
    fn test_theme_cache_refreshes_on_theme_change() {
        let _reset = preserve_theme();
        set_theme(Theme::default_dark());
        let before = highlight_markdown_code_block(Some("rust"), "fn main() { let x = 1; }");
        let before_fg = before[0].spans[1].style.fg;

        set_theme(Theme::default_light());
        let after = highlight_markdown_code_block(Some("rust"), "fn main() { let x = 1; }");
        let after_fg = after[0].spans[1].style.fg;

        assert_ne!(before_fg, after_fg);
    }

    #[test]
    fn test_truncate_spans_adds_ellipsis() {
        let spans = vec![Span::styled(
            "very-long-content".to_string(),
            Style::default().fg(text_primary()),
        )];
        let truncated = truncate_spans_with_ellipsis(&spans, 6);
        let text: String = truncated.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.ends_with('…'));
    }
}
