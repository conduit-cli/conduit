//! Built-in themes embedded in the binary.
//!
//! These themes are always available regardless of VS Code installation.

use ratatui::style::Color;

use super::types::Theme;

/// Get all built-in themes.
pub fn builtin_themes() -> Vec<(&'static str, Theme)> {
    vec![
        ("default-dark", Theme::default_dark()),
        ("default-light", Theme::default_light()),
        ("catppuccin-mocha", catppuccin_mocha()),
        ("catppuccin-latte", catppuccin_latte()),
        ("tokyo-night", tokyo_night()),
        ("dracula", dracula()),
    ]
}

/// Get a built-in theme by name.
pub fn get_builtin(name: &str) -> Option<Theme> {
    match name {
        "default" => Some(Theme::default_dark()),
        "default-dark" => Some(Theme::default_dark()),
        "default-light" => Some(Theme::default_light()),
        "catppuccin-mocha" => Some(catppuccin_mocha()),
        "catppuccin-latte" => Some(catppuccin_latte()),
        "tokyo-night" => Some(tokyo_night()),
        "dracula" => Some(dracula()),
        _ => None,
    }
}

/// Catppuccin Mocha theme.
pub fn catppuccin_mocha() -> Theme {
    // Catppuccin Mocha palette
    // https://github.com/catppuccin/catppuccin
    let crust = Color::Rgb(17, 17, 27);
    let mantle = Color::Rgb(24, 24, 37);
    let base = Color::Rgb(30, 30, 46);
    let surface0 = Color::Rgb(49, 50, 68);
    let surface1 = Color::Rgb(69, 71, 90);
    let surface2 = Color::Rgb(88, 91, 112);
    let overlay0 = Color::Rgb(108, 112, 134);
    let overlay1 = Color::Rgb(127, 132, 156);
    let _overlay2 = Color::Rgb(147, 153, 178);
    let subtext0 = Color::Rgb(166, 173, 200);
    let subtext1 = Color::Rgb(186, 194, 222);
    let text = Color::Rgb(205, 214, 244);

    let blue = Color::Rgb(137, 180, 250);
    let mauve = Color::Rgb(203, 166, 247);
    let green = Color::Rgb(166, 227, 161);
    let yellow = Color::Rgb(249, 226, 175);
    let red = Color::Rgb(243, 139, 168);
    let sky = Color::Rgb(137, 220, 235);

    Theme {
        name: "Catppuccin Mocha".to_string(),
        is_light: false,

        bg_terminal: crust,
        bg_base: base,
        bg_surface: surface0,
        bg_elevated: surface1,
        bg_highlight: surface2,
        markdown_code_bg: mantle,
        markdown_inline_code_bg: surface0,

        text_bright: text,
        text_primary: subtext1,
        text_secondary: subtext0,
        text_muted: overlay1,
        text_faint: overlay0,

        accent_primary: blue,
        accent_secondary: mauve,
        accent_success: green,
        accent_warning: yellow,
        accent_error: red,

        agent_claude: sky,
        agent_codex: mauve,
        agent_opencode: blue,

        pr_open_bg: green,
        pr_merged_bg: mauve,
        pr_closed_bg: red,
        pr_draft_bg: overlay1,
        pr_unknown_bg: surface2,

        spinner_active: blue,
        spinner_trail_1: Color::Rgb(120, 160, 220),
        spinner_trail_2: Color::Rgb(100, 140, 190),
        spinner_trail_3: Color::Rgb(80, 115, 160),
        spinner_trail_4: Color::Rgb(60, 90, 130),
        spinner_trail_5: Color::Rgb(45, 70, 100),
        spinner_inactive: overlay1,

        border_default: surface1,
        border_focused: blue,
        border_dimmed: surface0,

        shine_edge: overlay1,
        shine_mid: subtext0,
        shine_center: subtext1,
        shine_peak: text,

        tool_block_bg: mantle,
        tool_comment: overlay1,
        tool_command: subtext1,
        tool_output: subtext0,
        diff_add: green,
        diff_remove: red,
    }
}

/// Catppuccin Latte theme (light).
pub fn catppuccin_latte() -> Theme {
    // Catppuccin Latte palette (light)
    let base = Color::Rgb(239, 241, 245);
    let mantle = Color::Rgb(230, 233, 239);
    let crust = Color::Rgb(220, 224, 232);
    let surface0 = Color::Rgb(204, 208, 218);
    let surface1 = Color::Rgb(188, 192, 204);
    let surface2 = Color::Rgb(172, 176, 190);
    let overlay0 = Color::Rgb(156, 160, 176);
    let overlay1 = Color::Rgb(140, 143, 161);
    let _overlay2 = Color::Rgb(124, 127, 147);
    let subtext0 = Color::Rgb(108, 111, 133);
    let subtext1 = Color::Rgb(92, 95, 119);
    let text = Color::Rgb(76, 79, 105);

    let blue = Color::Rgb(30, 102, 245);
    let mauve = Color::Rgb(136, 57, 239);
    let green = Color::Rgb(64, 160, 43);
    let yellow = Color::Rgb(223, 142, 29);
    let red = Color::Rgb(210, 15, 57);
    let sky = Color::Rgb(4, 165, 229);

    Theme {
        name: "Catppuccin Latte".to_string(),
        is_light: true,

        bg_terminal: base,
        bg_base: base,
        bg_surface: mantle,
        bg_elevated: crust,
        bg_highlight: surface0,
        markdown_code_bg: mantle,
        markdown_inline_code_bg: crust,

        text_bright: text,
        text_primary: subtext1,
        text_secondary: subtext0,
        text_muted: overlay1,
        text_faint: overlay0,

        accent_primary: blue,
        accent_secondary: mauve,
        accent_success: green,
        accent_warning: yellow,
        accent_error: red,

        agent_claude: sky,
        agent_codex: mauve,
        agent_opencode: blue,

        pr_open_bg: green,
        pr_merged_bg: mauve,
        pr_closed_bg: red,
        pr_draft_bg: overlay1,
        pr_unknown_bg: surface2,

        spinner_active: blue,
        spinner_trail_1: Color::Rgb(60, 130, 240),
        spinner_trail_2: Color::Rgb(90, 150, 235),
        spinner_trail_3: Color::Rgb(120, 170, 230),
        spinner_trail_4: Color::Rgb(150, 190, 225),
        spinner_trail_5: Color::Rgb(180, 205, 220),
        spinner_inactive: overlay1,

        border_default: surface1,
        border_focused: blue,
        border_dimmed: surface0,

        shine_edge: overlay0,
        shine_mid: subtext0,
        shine_center: subtext1,
        shine_peak: text,

        tool_block_bg: mantle,
        tool_comment: overlay1,
        tool_command: subtext1,
        tool_output: subtext0,
        diff_add: green,
        diff_remove: red,
    }
}

/// Tokyo Night theme.
pub fn tokyo_night() -> Theme {
    // Tokyo Night palette
    let bg = Color::Rgb(26, 27, 38);
    let bg_dark = Color::Rgb(22, 22, 30);
    let bg_highlight = Color::Rgb(41, 46, 66);
    let terminal_black = Color::Rgb(65, 72, 104);
    let fg = Color::Rgb(192, 202, 245);
    let fg_dark = Color::Rgb(169, 177, 214);
    let fg_gutter = Color::Rgb(59, 66, 97);
    let dark3 = Color::Rgb(68, 75, 106);
    let comment = Color::Rgb(86, 95, 137);
    let dark5 = Color::Rgb(115, 125, 174);
    let blue0 = Color::Rgb(61, 89, 161);
    let blue = Color::Rgb(122, 162, 247);
    let cyan = Color::Rgb(125, 207, 255);
    let magenta = Color::Rgb(187, 154, 247);
    let green = Color::Rgb(158, 206, 106);
    let yellow = Color::Rgb(224, 175, 104);
    let red = Color::Rgb(247, 118, 142);

    Theme {
        name: "Tokyo Night".to_string(),
        is_light: false,

        bg_terminal: bg_dark,
        bg_base: bg,
        bg_surface: Color::Rgb(36, 40, 59),
        bg_elevated: bg_highlight,
        bg_highlight: Color::Rgb(51, 59, 91),
        markdown_code_bg: bg_dark,
        markdown_inline_code_bg: Color::Rgb(36, 40, 59),

        text_bright: fg,
        text_primary: fg_dark,
        text_secondary: dark5,
        text_muted: comment,
        text_faint: fg_gutter,

        accent_primary: blue,
        accent_secondary: magenta,
        accent_success: green,
        accent_warning: yellow,
        accent_error: red,

        agent_claude: cyan,
        agent_codex: magenta,
        agent_opencode: blue,

        pr_open_bg: green,
        pr_merged_bg: magenta,
        pr_closed_bg: red,
        pr_draft_bg: comment,
        pr_unknown_bg: dark3,

        spinner_active: blue,
        spinner_trail_1: Color::Rgb(105, 145, 220),
        spinner_trail_2: Color::Rgb(85, 125, 195),
        spinner_trail_3: Color::Rgb(70, 105, 170),
        spinner_trail_4: Color::Rgb(55, 85, 145),
        spinner_trail_5: Color::Rgb(45, 70, 120),
        spinner_inactive: comment,

        border_default: terminal_black,
        border_focused: blue0,
        border_dimmed: dark3,

        shine_edge: comment,
        shine_mid: dark5,
        shine_center: fg_dark,
        shine_peak: fg,

        tool_block_bg: bg_dark,
        tool_comment: comment,
        tool_command: fg_dark,
        tool_output: dark5,
        diff_add: green,
        diff_remove: red,
    }
}

/// Dracula theme.
pub fn dracula() -> Theme {
    // Dracula palette
    let background = Color::Rgb(40, 42, 54);
    let current_line = Color::Rgb(68, 71, 90);
    let foreground = Color::Rgb(248, 248, 242);
    let comment = Color::Rgb(98, 114, 164);
    let cyan = Color::Rgb(139, 233, 253);
    let green = Color::Rgb(80, 250, 123);
    let orange = Color::Rgb(255, 184, 108);
    let pink = Color::Rgb(255, 121, 198);
    let purple = Color::Rgb(189, 147, 249);
    let red = Color::Rgb(255, 85, 85);
    let _yellow = Color::Rgb(241, 250, 140); // Available if needed

    Theme {
        name: "Dracula".to_string(),
        is_light: false,

        bg_terminal: Color::Rgb(30, 32, 44),
        bg_base: background,
        bg_surface: current_line,
        bg_elevated: Color::Rgb(78, 81, 100),
        bg_highlight: Color::Rgb(88, 91, 110),
        markdown_code_bg: Color::Rgb(35, 37, 49),
        markdown_inline_code_bg: current_line,

        text_bright: foreground,
        text_primary: Color::Rgb(235, 235, 230),
        text_secondary: Color::Rgb(180, 185, 200),
        text_muted: comment,
        text_faint: Color::Rgb(75, 85, 120),

        accent_primary: purple,
        accent_secondary: pink,
        accent_success: green,
        accent_warning: orange,
        accent_error: red,

        agent_claude: cyan,
        agent_codex: pink,
        agent_opencode: purple,

        pr_open_bg: green,
        pr_merged_bg: purple,
        pr_closed_bg: red,
        pr_draft_bg: comment,
        pr_unknown_bg: current_line,

        spinner_active: purple,
        spinner_trail_1: Color::Rgb(165, 130, 225),
        spinner_trail_2: Color::Rgb(140, 115, 200),
        spinner_trail_3: Color::Rgb(115, 100, 175),
        spinner_trail_4: Color::Rgb(90, 85, 150),
        spinner_trail_5: Color::Rgb(70, 75, 125),
        spinner_inactive: comment,

        border_default: comment,
        border_focused: purple,
        border_dimmed: Color::Rgb(60, 65, 95),

        shine_edge: comment,
        shine_mid: Color::Rgb(150, 155, 180),
        shine_center: Color::Rgb(200, 200, 210),
        shine_peak: foreground,

        tool_block_bg: Color::Rgb(35, 37, 49),
        tool_comment: comment,
        tool_command: foreground,
        tool_output: Color::Rgb(180, 185, 200),
        diff_add: green,
        diff_remove: red,
    }
}
