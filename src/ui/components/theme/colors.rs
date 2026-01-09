//! Color manipulation utilities for theme derivation.
//!
//! These functions are used to derive colors when VS Code themes
//! don't provide all the colors we need.

use ratatui::style::Color;

/// Relative luminance for contrast calculations (WCAG 2.1).
pub fn relative_luminance(color: Color) -> Option<f64> {
    let (r, g, b) = to_rgb(color)?;
    let to_linear = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = to_linear(r);
    let g = to_linear(g);
    let b = to_linear(b);
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

/// Contrast ratio between two colors (WCAG 2.1).
pub fn contrast_ratio(a: Color, b: Color) -> Option<f64> {
    let la = relative_luminance(a)?;
    let lb = relative_luminance(b)?;
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    Some((lighter + 0.05) / (darker + 0.05))
}

/// Adjust a foreground color to meet a minimum contrast ratio against a background.
pub fn ensure_contrast_fg(fg: Color, bg: Color, min_ratio: f64) -> Color {
    if let Some(ratio) = contrast_ratio(fg, bg) {
        if ratio >= min_ratio {
            return fg;
        }
    } else {
        return fg;
    }

    let bg_luma = relative_luminance(bg).unwrap_or(0.5);
    let prefer_darken = bg_luma > 0.5;

    for step in 1..=10 {
        let amount = 0.06 * step as f64;
        let candidate = if prefer_darken {
            darken(fg, amount)
        } else {
            lighten(fg, amount)
        };
        if contrast_ratio(candidate, bg).unwrap_or(0.0) >= min_ratio {
            return candidate;
        }
    }

    // Fallback to black/white, whichever yields higher contrast.
    let black = Color::Rgb(0, 0, 0);
    let white = Color::Rgb(255, 255, 255);
    let black_ratio = contrast_ratio(black, bg).unwrap_or(0.0);
    let white_ratio = contrast_ratio(white, bg).unwrap_or(0.0);
    if black_ratio >= white_ratio {
        black
    } else {
        white
    }
}

/// Adjust a background color to meet a minimum contrast ratio against another background.
pub fn ensure_contrast_bg(bg: Color, against: Color, min_ratio: f64) -> Color {
    if let Some(ratio) = contrast_ratio(bg, against) {
        if ratio >= min_ratio {
            return bg;
        }
    } else {
        return bg;
    }

    let mut best = bg;
    let mut best_ratio = contrast_ratio(bg, against).unwrap_or(0.0);

    for step in 1..=10 {
        let amount = 0.06 * step as f64;
        let darker = darken(bg, amount);
        let lighter = lighten(bg, amount);
        let dark_ratio = contrast_ratio(darker, against).unwrap_or(0.0);
        let light_ratio = contrast_ratio(lighter, against).unwrap_or(0.0);

        if dark_ratio >= min_ratio || light_ratio >= min_ratio {
            return if dark_ratio >= light_ratio {
                darker
            } else {
                lighter
            };
        }

        if dark_ratio > best_ratio {
            best_ratio = dark_ratio;
            best = darker;
        }
        if light_ratio > best_ratio {
            best_ratio = light_ratio;
            best = lighter;
        }
    }

    best
}

/// Parse a hex color string to a Color.
///
/// Supports formats: "#RGB", "#RRGGBB", "#RRGGBBAA"
pub fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');

    match hex.len() {
        // #RGB format
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(Color::Rgb(r, g, b))
        }
        // #RRGGBB format
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        // #RRGGBBAA format (ignore alpha)
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// Extract RGB components from a Color.
/// Returns None for non-RGB colors.
fn to_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

/// Darken a color by a percentage (0.0-1.0).
pub fn darken(color: Color, amount: f64) -> Color {
    let Some((r, g, b)) = to_rgb(color) else {
        return color;
    };
    let factor = 1.0 - amount.clamp(0.0, 1.0);
    Color::Rgb(
        (r as f64 * factor) as u8,
        (g as f64 * factor) as u8,
        (b as f64 * factor) as u8,
    )
}

/// Lighten a color by a percentage (0.0-1.0).
pub fn lighten(color: Color, amount: f64) -> Color {
    let Some((r, g, b)) = to_rgb(color) else {
        return color;
    };
    let amount = amount.clamp(0.0, 1.0);
    Color::Rgb(
        (r as f64 + (255.0 - r as f64) * amount) as u8,
        (g as f64 + (255.0 - g as f64) * amount) as u8,
        (b as f64 + (255.0 - b as f64) * amount) as u8,
    )
}

/// Dim a color by reducing its brightness by a factor (0.0-1.0).
/// A factor of 0.5 means the color is 50% as bright.
pub fn dim(color: Color, factor: f64) -> Color {
    let Some((r, g, b)) = to_rgb(color) else {
        return color;
    };
    let factor = factor.clamp(0.0, 1.0);
    Color::Rgb(
        (r as f64 * factor) as u8,
        (g as f64 * factor) as u8,
        (b as f64 * factor) as u8,
    )
}

/// Boost color brightness by a factor (>1.0 brightens, <1.0 dims).
pub fn boost_brightness(color: Color, factor: f64) -> Color {
    let Some((r, g, b)) = to_rgb(color) else {
        return color;
    };
    Color::Rgb(
        ((r as f64 * factor).min(255.0)) as u8,
        ((g as f64 * factor).min(255.0)) as u8,
        ((b as f64 * factor).min(255.0)) as u8,
    )
}

/// Interpolate between two colors (t: 0.0-1.0).
pub fn interpolate(from: Color, to: Color, t: f64) -> Color {
    let Some((r1, g1, b1)) = to_rgb(from) else {
        return from;
    };
    let Some((r2, g2, b2)) = to_rgb(to) else {
        return from;
    };
    let t = t.clamp(0.0, 1.0);
    Color::Rgb(
        (r1 as f64 + (r2 as f64 - r1 as f64) * t) as u8,
        (g1 as f64 + (g2 as f64 - g1 as f64) * t) as u8,
        (b1 as f64 + (b2 as f64 - b1 as f64) * t) as u8,
    )
}

/// Convert RGB to HSL.
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;

    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }

    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if (max - r).abs() < f64::EPSILON {
        let mut h = (g - b) / d;
        if g < b {
            h += 6.0;
        }
        h
    } else if (max - g).abs() < f64::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };

    (h * 60.0, s, l)
}

/// Convert HSL to RGB.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    if s.abs() < f64::EPSILON {
        let v = (l * 255.0) as u8;
        return (v, v, v);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let h = h / 360.0;

    let hue_to_rgb = |p: f64, q: f64, mut t: f64| -> f64 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            return p + (q - p) * 6.0 * t;
        }
        if t < 1.0 / 2.0 {
            return q;
        }
        if t < 2.0 / 3.0 {
            return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
        }
        p
    };

    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);

    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Shift hue by degrees (0-360).
pub fn shift_hue(color: Color, degrees: f64) -> Color {
    let Some((r, g, b)) = to_rgb(color) else {
        return color;
    };
    let (mut h, s, l) = rgb_to_hsl(r, g, b);
    h = (h + degrees) % 360.0;
    if h < 0.0 {
        h += 360.0;
    }
    let (r, g, b) = hsl_to_rgb(h, s, l);
    Color::Rgb(r, g, b)
}

/// Desaturate a color by a percentage (0.0-1.0).
pub fn desaturate(color: Color, amount: f64) -> Color {
    let Some((r, g, b)) = to_rgb(color) else {
        return color;
    };
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let new_s = s * (1.0 - amount.clamp(0.0, 1.0));
    let (r, g, b) = hsl_to_rgb(h, new_s, l);
    Color::Rgb(r, g, b)
}

/// Saturate a color by a percentage (0.0-1.0).
pub fn saturate(color: Color, amount: f64) -> Color {
    let Some((r, g, b)) = to_rgb(color) else {
        return color;
    };
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let new_s = (s + (1.0 - s) * amount.clamp(0.0, 1.0)).min(1.0);
    let (r, g, b) = hsl_to_rgb(h, new_s, l);
    Color::Rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_hex_color("#000"), Some(Color::Rgb(0, 0, 0)));
        assert_eq!(parse_hex_color("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_hex_color("#00ff00"), Some(Color::Rgb(0, 255, 0)));
        assert_eq!(parse_hex_color("#0000ff"), Some(Color::Rgb(0, 0, 255)));
        assert_eq!(parse_hex_color("#1e1e2e"), Some(Color::Rgb(30, 30, 46)));
        assert_eq!(parse_hex_color("#1e1e2eff"), Some(Color::Rgb(30, 30, 46))); // With alpha
        assert_eq!(parse_hex_color("1e1e2e"), Some(Color::Rgb(30, 30, 46))); // Without #
    }

    #[test]
    fn test_contrast_ratio_basic() {
        let black = Color::Rgb(0, 0, 0);
        let white = Color::Rgb(255, 255, 255);
        let ratio = contrast_ratio(black, white).unwrap();
        assert!(ratio >= 21.0);
    }

    #[test]
    fn test_ensure_contrast_fg() {
        let bg = Color::Rgb(245, 245, 245);
        let fg = Color::Rgb(230, 230, 230);
        let adjusted = ensure_contrast_fg(fg, bg, 4.5);
        assert!(contrast_ratio(adjusted, bg).unwrap_or(0.0) >= 4.5);
    }

    #[test]
    fn test_ensure_contrast_bg() {
        let base = Color::Rgb(30, 30, 30);
        let bg = Color::Rgb(32, 32, 32);
        let adjusted = ensure_contrast_bg(bg, base, 2.0);
        assert!(contrast_ratio(adjusted, base).unwrap_or(0.0) >= 2.0);
    }

    #[test]
    fn test_darken() {
        let white = Color::Rgb(255, 255, 255);
        let darkened = darken(white, 0.5);
        assert_eq!(darkened, Color::Rgb(127, 127, 127));
    }

    #[test]
    fn test_lighten() {
        let black = Color::Rgb(0, 0, 0);
        let lightened = lighten(black, 0.5);
        assert_eq!(lightened, Color::Rgb(127, 127, 127));
    }

    #[test]
    fn test_interpolate() {
        let black = Color::Rgb(0, 0, 0);
        let white = Color::Rgb(255, 255, 255);
        let mid = interpolate(black, white, 0.5);
        assert_eq!(mid, Color::Rgb(127, 127, 127));
    }

    #[test]
    fn test_shift_hue() {
        // Red shifted by 120 degrees should be green-ish
        let red = Color::Rgb(255, 0, 0);
        let shifted = shift_hue(red, 120.0);
        // Should be lime/green
        if let Color::Rgb(r, g, b) = shifted {
            assert!(g > r && g > b);
        }
    }
}
