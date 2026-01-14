use std::time::{Duration, Instant};

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// Processing words used by Claude Code (subset of ~90 words)
const PROCESSING_WORDS: &[&str] = &[
    "Accomplishing",
    "Baking",
    "Booping",
    "Brewing",
    "Calculating",
    "Cerebrating",
    "Churning",
    "Clauding",
    "Cogitating",
    "Combobulating",
    "Computing",
    "Concocting",
    "Conjuring",
    "Contemplating",
    "Cooking",
    "Crafting",
    "Crunching",
    "Deciphering",
    "Deliberating",
    "Divining",
    "Enchanting",
    "Finagling",
    "Forging",
    "Frolicking",
    "Generating",
    "Hatching",
    "Hustling",
    "Ideating",
    "Imagining",
    "Incubating",
    "Inferring",
    "Manifesting",
    "Marinating",
    "Meandering",
    "Mulling",
    "Musing",
    "Noodling",
    "Percolating",
    "Pondering",
    "Pontificating",
    "Processing",
    "Puzzling",
    "Ruminating",
    "Scheming",
    "Shimmying",
    "Simmering",
    "Spelunking",
    "Spinning",
    "Stewing",
    "Sussing",
    "Synthesizing",
    "Thinking",
    "Tinkering",
    "Transmuting",
    "Unfurling",
    "Vibing",
    "Wandering",
    "Whirring",
    "Wizarding",
    "Working",
    "Wrangling",
];

/// Spinner animation frames (Claude Code style)
const SPINNER_FRAMES: &[&str] = &["·", "✢", "✳", "∗", "✻", "✽"];

/// Shimmer gradient colors (bright orange to very dark)
const SHIMMER_BRIGHT: (u8, u8, u8) = (255, 180, 80); // Bright orange
const SHIMMER_DIM: (u8, u8, u8) = (100, 50, 20); // Very dark for maximum contrast

/// Width of the shimmer "wave" in characters
const SHIMMER_WIDTH: f32 = 4.0;

/// Current processing state
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessingState {
    Thinking,
    ToolUse(String),
    Reading,
    Writing,
    Searching,
}

impl ProcessingState {
    pub fn as_str(&self) -> &str {
        match self {
            ProcessingState::Thinking => "thinking",
            ProcessingState::ToolUse(name) => name,
            ProcessingState::Reading => "reading",
            ProcessingState::Writing => "writing",
            ProcessingState::Searching => "searching",
        }
    }
}

/// Thinking indicator that shows while agent is processing
pub struct ThinkingIndicator {
    /// Current processing word
    word: &'static str,
    /// Spinner frame index
    spinner_frame: usize,
    /// Shimmer animation offset (moves the gradient)
    shimmer_offset: f32,
    /// When processing started
    start_time: Instant,
    /// Tokens received so far
    tokens: usize,
    /// Current processing state
    state: ProcessingState,
}

impl ThinkingIndicator {
    /// Create a new thinking indicator with a random word
    pub fn new() -> Self {
        Self {
            word: Self::random_word(),
            spinner_frame: 0,
            shimmer_offset: -SHIMMER_WIDTH, // Start from before the text
            start_time: Instant::now(),
            tokens: 0,
            state: ProcessingState::Thinking,
        }
    }

    /// Get a random processing word
    fn random_word() -> &'static str {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;

        // Simple pseudo-random selection based on current time
        let mut hasher = DefaultHasher::new();
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        let index = (hasher.finish() as usize) % PROCESSING_WORDS.len();
        PROCESSING_WORDS[index]
    }

    /// Advance the spinner and shimmer animations
    pub fn tick(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
        // Move shimmer by ~1.5 characters per tick (at ~10 ticks/sec = 15 chars/sec)
        self.shimmer_offset += 1.5;
        // Wrap around when the wave has fully passed the text
        // Text is roughly 20-25 chars, add padding for wave to exit before restart
        let wrap_point = 30.0 + SHIMMER_WIDTH;
        if self.shimmer_offset > wrap_point {
            self.shimmer_offset = -SHIMMER_WIDTH; // Start from before the text
        }
    }

    /// Add tokens to the count
    pub fn add_tokens(&mut self, count: usize) {
        self.tokens += count;
    }

    /// Set the current processing state
    pub fn set_state(&mut self, state: ProcessingState) {
        self.state = state;
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Reset with a new random word
    pub fn reset(&mut self) {
        self.word = Self::random_word();
        self.spinner_frame = 0;
        self.shimmer_offset = -SHIMMER_WIDTH; // Start from before the text
        self.start_time = Instant::now();
        self.tokens = 0;
        self.state = ProcessingState::Thinking;
    }

    /// Calculate shimmer color for a character at given position
    fn shimmer_color(&self, char_index: usize, _total_chars: usize) -> Color {
        // Calculate position in the shimmer wave
        // The wave moves from left to right as shimmer_offset increases
        let pos = char_index as f32 - self.shimmer_offset;

        // Use a smooth wave function (gaussian-like bump)
        // This creates a bright "highlight" that moves across the text
        let wave_pos = pos / SHIMMER_WIDTH;
        let highlight = (-wave_pos * wave_pos).exp(); // Gaussian curve, peaks at 1.0

        // Very minimal ambient - text is mostly dim
        let ambient = 0.15;

        // Highlight dominates - goes from ambient to full bright
        let final_brightness = (ambient + highlight * 0.85).clamp(0.0, 1.0);

        // Interpolate between dim and bright colors
        let r = lerp(SHIMMER_DIM.0, SHIMMER_BRIGHT.0, final_brightness);
        let g = lerp(SHIMMER_DIM.1, SHIMMER_BRIGHT.1, final_brightness);
        let b = lerp(SHIMMER_DIM.2, SHIMMER_BRIGHT.2, final_brightness);

        Color::Rgb(r, g, b)
    }

    /// Render text with shimmer effect
    fn render_shimmer_text(&self, text: &str) -> Vec<Span<'static>> {
        let chars: Vec<char> = text.chars().collect();
        let total = chars.len();

        chars
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let color = self.shimmer_color(i, total);
                Span::styled(c.to_string(), Style::default().fg(color))
            })
            .collect()
    }

    /// Render as a Line for display in chat view
    pub fn render(&self) -> Line<'static> {
        let elapsed = self.elapsed();
        let duration_str = format_duration(elapsed);

        let spinner = SPINNER_FRAMES[self.spinner_frame];
        let state = self.state.as_str();

        // Build the shimmering part: "✳ Tinkering… "
        let shimmer_text = format!("{} {}… ", spinner, self.word);
        let mut spans = self.render_shimmer_text(&shimmer_text);

        // Add the non-shimmering metadata part
        spans.extend(vec![
            Span::styled("(", Style::default().fg(Color::DarkGray)),
            Span::styled("esc", Style::default().fg(Color::Gray)),
            Span::styled(" to interrupt · ", Style::default().fg(Color::DarkGray)),
            Span::styled(duration_str, Style::default().fg(Color::Gray)),
            Span::styled(" · ↓ ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", self.tokens), Style::default().fg(Color::Gray)),
            Span::styled(" tokens · ", Style::default().fg(Color::DarkGray)),
            Span::styled(state.to_string(), Style::default().fg(Color::Gray)),
            Span::styled(")", Style::default().fg(Color::DarkGray)),
        ]);

        Line::from(spans)
    }
}

/// Linear interpolation between two u8 values
fn lerp(a: u8, b: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    (a as f32 + (b as f32 - a as f32) * t) as u8
}

/// Format duration in human-readable format (e.g., "1h 23m 45s")
fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();

    if total_secs < 60 {
        return format!("{}s", total_secs);
    }

    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    let mut parts = Vec::new();

    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }

    parts.join(" ")
}

impl Default for ThinkingIndicator {
    fn default() -> Self {
        Self::new()
    }
}
