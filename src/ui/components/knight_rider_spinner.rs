//! Knight Rider style bidirectional scanner animation.
//!
//! A scanning bar animation with gradient trail that bounces back and forth.
//! Based on opencode's spinner.ts implementation.

use ratatui::{style::Color, style::Style, text::Span};

use super::{
    spinner_active, spinner_inactive, spinner_trail_1, spinner_trail_2, spinner_trail_3,
    spinner_trail_4, spinner_trail_5,
};

/// Animation phase for the Knight Rider scanner
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Moving forward (left to right)
    MovingForward,
    /// Holding at right end
    HoldEnd,
    /// Moving backward (right to left)
    MovingBackward,
    /// Holding at left start
    HoldStart,
}

/// Knight Rider style bidirectional scanner animation
pub struct KnightRiderSpinner {
    /// Current frame index in the animation cycle
    frame: usize,
    /// Bar width (number of characters)
    width: usize,
    /// Frames to hold at end (right side)
    hold_end_frames: usize,
    /// Frames to hold at start (left side)
    hold_start_frames: usize,
    /// Trail length (how many positions the trail extends)
    trail_length: usize,
    /// Total frames in one complete cycle
    total_frames: usize,
    /// Minimum alpha for fading (0.3 = 30%, never goes below this)
    min_alpha: f64,
    /// Inactive factor - brightness of inactive squares (0.6 = 60%)
    inactive_factor: f64,
}

impl KnightRiderSpinner {
    /// Create a new spinner with default width of 8
    pub fn new() -> Self {
        Self::with_width(8)
    }

    /// Create a new spinner with specified width
    pub fn with_width(width: usize) -> Self {
        let width = width.max(3); // Minimum width of 3
        let hold_end_frames = 9; // Match opencode defaults
        let hold_start_frames = 30;
        let trail_length = 6;
        // Cycle: Forward (width) + Hold End + Backward (width-1) + Hold Start
        let total_frames = width + hold_end_frames + (width - 1) + hold_start_frames;

        Self {
            frame: 0,
            width,
            hold_end_frames,
            hold_start_frames,
            trail_length,
            total_frames,
            min_alpha: 0.7,       // Higher min to prevent too much fading
            inactive_factor: 0.8, // Inactive at 80% brightness base
        }
    }

    /// Advance animation by one tick
    pub fn tick(&mut self) {
        self.frame = (self.frame + 1) % self.total_frames;
    }

    /// Get current animation phase and progress within that phase
    fn get_state(&self) -> (Phase, usize, usize, usize) {
        let forward_frames = self.width;
        let hold_end_start = forward_frames;
        let backward_start = hold_end_start + self.hold_end_frames;
        let backward_frames = self.width - 1;
        let hold_start_start = backward_start + backward_frames;

        if self.frame < forward_frames {
            // Moving forward: position goes 0 to width-1
            let position = self.frame;
            let progress = self.frame;
            let total = forward_frames;
            (Phase::MovingForward, position, progress, total)
        } else if self.frame < backward_start {
            // Holding at end
            let position = self.width - 1;
            let progress = self.frame - hold_end_start;
            let total = self.hold_end_frames;
            (Phase::HoldEnd, position, progress, total)
        } else if self.frame < hold_start_start {
            // Moving backward: position goes width-2 to 0
            let backward_index = self.frame - backward_start;
            let position = self.width - 2 - backward_index;
            let progress = backward_index;
            let total = backward_frames;
            (Phase::MovingBackward, position, progress, total)
        } else {
            // Holding at start
            let position = 0;
            let progress = self.frame - hold_start_start;
            let total = self.hold_start_frames;
            (Phase::HoldStart, position, progress, total)
        }
    }

    /// Get the base color for a trail position (0 = brightest, higher = dimmer)
    fn color_for_trail_index(&self, index: usize) -> Color {
        match index {
            0 => spinner_active(),
            1 => spinner_trail_1(),
            2 => spinner_trail_2(),
            3 => spinner_trail_3(),
            4 => spinner_trail_4(),
            5 => spinner_trail_5(),
            _ => spinner_inactive(),
        }
    }

    /// Dim a color by a factor (0.0 = black, 1.0 = original)
    fn dim_color(&self, color: Color, factor: f64) -> Color {
        match color {
            Color::Rgb(r, g, b) => Color::Rgb(
                (r as f64 * factor) as u8,
                (g as f64 * factor) as u8,
                (b as f64 * factor) as u8,
            ),
            _ => color,
        }
    }

    /// Calculate fade factor for the current state
    /// During hold: fades OUT (1.0 → min_alpha)
    /// During movement: fades IN (min_alpha → 1.0)
    fn fade_factor(&self, phase: Phase, progress: usize, total: usize) -> f64 {
        if total == 0 {
            return 1.0;
        }

        let progress_ratio = (progress as f64) / (total.max(1) as f64);

        match phase {
            Phase::HoldStart | Phase::HoldEnd => {
                // Fade out: start at 1.0, end at min_alpha
                let fade = 1.0 - progress_ratio * (1.0 - self.min_alpha);
                fade.max(self.min_alpha)
            }
            Phase::MovingForward | Phase::MovingBackward => {
                // Fade in: start at min_alpha, end at 1.0
                self.min_alpha + progress_ratio * (1.0 - self.min_alpha)
            }
        }
    }

    /// Render the spinner as a vector of styled spans
    pub fn render(&self) -> Vec<Span<'static>> {
        let active_char = "■";
        let inactive_char = "⬝";

        let (phase, active_position, progress, total) = self.get_state();
        let is_moving_forward = matches!(phase, Phase::MovingForward | Phase::HoldEnd);
        let is_holding = matches!(phase, Phase::HoldStart | Phase::HoldEnd);
        let hold_progress = if is_holding { progress } else { 0 };
        let fade = self.fade_factor(phase, progress, total);

        let mut spans = Vec::with_capacity(self.width);

        for char_index in 0..self.width {
            // Calculate directional distance (positive = trailing behind)
            let directional_distance = if is_moving_forward {
                active_position as isize - char_index as isize
            } else {
                char_index as isize - active_position as isize
            };

            // Calculate color index for this character
            let color_index = if is_holding {
                // During hold: shift trail by hold progress (trail fades away)
                directional_distance + hold_progress as isize
            } else if directional_distance > 0
                && (directional_distance as usize) < self.trail_length
            {
                // Normal movement: show gradient trail behind
                directional_distance
            } else if directional_distance == 0 {
                // At active position: brightest
                0
            } else {
                -1 // Inactive
            };

            // Get base color and apply appropriate dimming
            let color = if color_index >= 0 && (color_index as usize) < self.trail_length {
                // Active trail: use full trail colors
                self.color_for_trail_index(color_index as usize)
            } else {
                // Inactive: apply inactive_factor as base, then fade on top
                // opencode: defaultRgba.a = baseInactiveAlpha * fadeFactor
                // Effective range: 0.6 * 0.3 = 0.18 to 0.6 * 1.0 = 0.6
                let effective_brightness = self.inactive_factor * fade;
                self.dim_color(spinner_inactive(), effective_brightness)
            };

            // Choose character based on whether it's part of the active trail
            let ch = if color_index >= 0 && (color_index as usize) < self.trail_length {
                active_char
            } else {
                inactive_char
            };

            spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
        }

        spans
    }

    /// Reset spinner to initial state
    pub fn reset(&mut self) {
        self.frame = 0;
    }
}

impl Default for KnightRiderSpinner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_creation() {
        let spinner = KnightRiderSpinner::new();
        assert_eq!(spinner.width, 8);
        assert_eq!(spinner.frame, 0);
        // Total frames = 8 + 9 + 7 + 30 = 54
        assert_eq!(spinner.total_frames, 54);
    }

    #[test]
    fn test_spinner_phases() {
        let mut spinner = KnightRiderSpinner::with_width(4);
        // Total frames = 4 + 9 + 3 + 30 = 46

        // Frame 0: MovingForward, position 0
        let (phase, pos, _, _) = spinner.get_state();
        assert_eq!(phase, Phase::MovingForward);
        assert_eq!(pos, 0);

        // Frame 3: MovingForward, position 3 (last visible position)
        spinner.frame = 3;
        let (phase, pos, _, _) = spinner.get_state();
        assert_eq!(phase, Phase::MovingForward);
        assert_eq!(pos, 3);

        // Frame 4: HoldEnd starts
        spinner.frame = 4;
        let (phase, pos, progress, _) = spinner.get_state();
        assert_eq!(phase, Phase::HoldEnd);
        assert_eq!(pos, 3);
        assert_eq!(progress, 0);

        // Frame 13: MovingBackward starts (4 + 9 = 13)
        spinner.frame = 13;
        let (phase, pos, _, _) = spinner.get_state();
        assert_eq!(phase, Phase::MovingBackward);
        assert_eq!(pos, 2); // width-2 = 2

        // Frame 16: HoldStart (4 + 9 + 3 = 16)
        spinner.frame = 16;
        let (phase, pos, progress, _) = spinner.get_state();
        assert_eq!(phase, Phase::HoldStart);
        assert_eq!(pos, 0);
        assert_eq!(progress, 0);
    }

    #[test]
    fn test_spinner_tick_wraps() {
        let mut spinner = KnightRiderSpinner::with_width(4);
        // Total frames = 46
        spinner.frame = 45;
        spinner.tick();
        assert_eq!(spinner.frame, 0); // Should wrap around
    }

    #[test]
    fn test_render_output() {
        let spinner = KnightRiderSpinner::with_width(4);
        let spans = spinner.render();
        assert_eq!(spans.len(), 4);
    }
}
