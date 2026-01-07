//! Diagonal shine animation for the Conduit logo.
//!
//! Creates a periodic "metallic shine" effect that sweeps diagonally
//! across the ASCII logo from top-left to bottom-right.

use rand::Rng;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use super::{SHINE_CENTER, SHINE_EDGE, SHINE_MID, SHINE_PEAK, TEXT_MUTED};

/// The Conduit logo as an array of strings
const LOGO_LINES: [&str; 7] = [
    "  ░██████                               ░██            ░██   ░██   ",
    " ░██   ░██                              ░██                  ░██   ",
    "░██         ░███████  ░████████   ░████████ ░██    ░██ ░██░████████",
    "░██        ░██    ░██ ░██    ░██ ░██    ░██ ░██    ░██ ░██   ░██   ",
    "░██        ░██    ░██ ░██    ░██ ░██    ░██ ░██    ░██ ░██   ░██   ",
    " ░██   ░██ ░██    ░██ ░██    ░██ ░██   ░███ ░██   ░███ ░██   ░██   ",
    "  ░██████   ░███████  ░██    ░██  ░█████░██  ░█████░██ ░██    ░████",
];

/// Easing type for the shine animation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EasingType {
    /// Constant speed
    #[default]
    Linear,
    /// Starts slow, accelerates
    EaseIn,
    /// Starts fast, decelerates
    EaseOut,
    /// Slow start and end, fast middle
    EaseInOut,
}

/// Diagonal shine animation state
pub struct LogoShineAnimation {
    /// Current frame in the animation cycle
    frame: usize,
    /// Total frames for one complete sweep
    sweep_frames: usize,
    /// Current interval frames (randomized between min/max)
    interval_frames: usize,
    /// Minimum interval between shines (in frames)
    min_interval: usize,
    /// Maximum interval between shines (in frames)
    max_interval: usize,
    /// Width of the shine band (in diagonal units)
    band_width: usize,
    /// Base color for non-shining characters
    base_color: Color,
    /// Easing type for animation
    easing: EasingType,
    /// Logo width (max line length)
    logo_width: usize,
    /// Logo height (number of lines)
    logo_height: usize,
}

impl LogoShineAnimation {
    /// Create a new logo shine animation with default settings
    pub fn new() -> Self {
        let logo_width = LOGO_LINES
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(70);
        let logo_height = LOGO_LINES.len();

        // Diagonal range is 0 to (width + height - 1)
        // Add band_width to ensure shine fully exits
        let band_width = 5;

        // Speed: move 4 diagonal units per frame for a fast sweep
        // Total diagonal distance ~82, at 4 per frame = ~20 frames = ~1 second sweep
        let speed = 4;
        let total_diagonal = logo_width + logo_height + band_width;
        let sweep_frames = total_diagonal.div_ceil(speed);

        // Randomize initial interval
        let min_interval = 180; // ~3 seconds at 60fps with tick every 3 frames
        let max_interval = 300; // ~5 seconds
        let interval_frames = rand::rng().random_range(min_interval..=max_interval);

        // Start with a short delay (~1 second) before first shine so users see it quickly
        let initial_delay = 20; // ~1 second at 50ms per tick

        Self {
            frame: sweep_frames + interval_frames - initial_delay, // Start near end of interval
            sweep_frames,
            interval_frames,
            min_interval,
            max_interval,
            band_width,
            base_color: TEXT_MUTED,
            easing: EasingType::Linear,
            logo_width,
            logo_height,
        }
    }

    /// Set the easing type for the animation
    #[allow(dead_code)]
    pub fn with_easing(mut self, easing: EasingType) -> Self {
        self.easing = easing;
        self
    }

    /// Reset the animation to show shine after ~1 second delay
    /// Call this when transitioning back to the splash screen
    pub fn reset(&mut self) {
        let initial_delay = 20; // ~1 second at 50ms per tick
        self.interval_frames = rand::rng().random_range(self.min_interval..=self.max_interval);
        self.frame = self.sweep_frames + self.interval_frames - initial_delay;
    }

    /// Advance the animation by one tick
    pub fn tick(&mut self) {
        let total_frames = self.sweep_frames + self.interval_frames;
        self.frame = (self.frame + 1) % total_frames;

        // When cycle completes, randomize the next interval
        if self.frame == 0 {
            self.interval_frames = rand::rng().random_range(self.min_interval..=self.max_interval);
        }
    }

    /// Get the current shine position with easing applied
    fn shine_position(&self) -> Option<f64> {
        if self.frame >= self.sweep_frames {
            // In interval phase, no shine visible
            return None;
        }

        let progress = self.frame as f64 / self.sweep_frames as f64;
        let eased_progress = self.apply_easing(progress);

        // Map progress to diagonal position
        let max_diagonal = (self.logo_width + self.logo_height) as f64;
        Some(eased_progress * max_diagonal)
    }

    /// Apply easing function to progress (0.0 to 1.0)
    fn apply_easing(&self, t: f64) -> f64 {
        match self.easing {
            EasingType::Linear => t,
            EasingType::EaseIn => t * t,
            EasingType::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            EasingType::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
        }
    }

    /// Get the color for a character at position (x, y)
    fn color_at(&self, x: usize, y: usize, ch: char) -> Color {
        // Space characters don't get shine effect
        if ch == ' ' {
            return self.base_color;
        }

        let Some(shine_pos) = self.shine_position() else {
            return self.base_color;
        };

        // Calculate diagonal coordinate (top-left to bottom-right)
        let diagonal = (x + y) as f64;
        let distance = (diagonal - shine_pos).abs();

        // Map distance to color based on band width
        let band_width = self.band_width as f64;

        if distance > band_width {
            self.base_color
        } else if distance < 1.0 {
            SHINE_PEAK
        } else if distance < 2.0 {
            SHINE_CENTER
        } else if distance < 3.0 {
            SHINE_MID
        } else {
            SHINE_EDGE
        }
    }

    /// Render the logo with the current shine effect
    pub fn render_logo_lines(&self) -> Vec<Line<'static>> {
        LOGO_LINES
            .iter()
            .enumerate()
            .map(|(y, line)| self.render_line(y, line))
            .collect()
    }

    /// Render a single line with shine effect
    ///
    /// Note: Creates a Span per character for gradient coloring. This allocates
    /// frequently but is acceptable for the small logo (~70 chars × 7 lines).
    fn render_line(&self, y: usize, line: &str) -> Line<'static> {
        let spans: Vec<Span> = line
            .chars()
            .enumerate()
            .map(|(x, ch)| {
                let color = self.color_at(x, y, ch);
                Span::styled(ch.to_string(), Style::default().fg(color))
            })
            .collect();

        Line::from(spans)
    }
}

impl Default for LogoShineAnimation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_animation_creation() {
        let anim = LogoShineAnimation::new();
        assert_eq!(anim.logo_height, 7);
        assert!(anim.logo_width > 60);
        assert_eq!(anim.band_width, 5);
    }

    #[test]
    fn test_tick_advances_frame() {
        let mut anim = LogoShineAnimation::new();
        let initial_frame = anim.frame;
        anim.tick();
        assert_eq!(
            anim.frame,
            (initial_frame + 1) % (anim.sweep_frames + anim.interval_frames)
        );
    }

    #[test]
    fn test_render_produces_correct_line_count() {
        let anim = LogoShineAnimation::new();
        let lines = anim.render_logo_lines();
        assert_eq!(lines.len(), 7);
    }

    #[test]
    fn test_shine_position_during_sweep() {
        let mut anim = LogoShineAnimation::new();
        anim.frame = 0; // Start of sweep
        assert!(anim.shine_position().is_some());
        assert!((anim.shine_position().unwrap() - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_shine_position_during_interval() {
        let mut anim = LogoShineAnimation::new();
        anim.frame = anim.sweep_frames + 10; // In interval
        assert!(anim.shine_position().is_none());
    }

    #[test]
    fn test_easing_linear() {
        let anim = LogoShineAnimation::new();
        assert!((anim.apply_easing(0.5) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_easing_ease_in() {
        let anim = LogoShineAnimation::new().with_easing(EasingType::EaseIn);
        assert!(anim.apply_easing(0.5) < 0.5); // Should be slower at start
    }
}
