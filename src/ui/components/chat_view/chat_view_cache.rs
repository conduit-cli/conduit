//! Caching helpers for the chat view.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ratatui::text::Line;

use super::{ChatMessage, ChatView, MessageRole};

/// Cached rendered lines for a single message
#[derive(Debug, Clone)]
pub(super) struct CachedMessageLines {
    /// Pre-rendered lines for this message
    pub(super) lines: Vec<Line<'static>>,
    /// Joiner strings for each line (soft-wrap continuations)
    pub(super) joiner_before: Vec<Option<String>>,
    /// Hash of message content for invalidation detection (reserved for future use)
    #[allow(dead_code)]
    pub(super) content_hash: u64,
}

/// Line cache for efficient rendering
#[derive(Debug, Clone, Default)]
pub(super) struct LineCache {
    /// Cached lines per message (indexed by message index)
    pub(super) entries: Vec<Option<CachedMessageLines>>,
    /// Total line count across all cached messages
    pub(super) total_line_count: usize,
}

impl ChatView {
    /// Compute a hash for a message's content (for cache invalidation)
    fn compute_message_hash(msg: &ChatMessage) -> u64 {
        let mut hasher = DefaultHasher::new();
        msg.content.hash(&mut hasher);
        msg.role.hash(&mut hasher);
        msg.is_collapsed.hash(&mut hasher);
        msg.is_streaming.hash(&mut hasher);
        if let Some(ref name) = msg.tool_name {
            name.hash(&mut hasher);
        }
        if let Some(ref args) = msg.tool_args {
            args.hash(&mut hasher);
        }
        msg.exit_code.hash(&mut hasher);
        // Hash summary fields if present (TurnSummary doesn't derive Hash)
        if let Some(ref summary) = msg.summary {
            summary.duration_secs.hash(&mut hasher);
            summary.input_tokens.hash(&mut hasher);
            summary.output_tokens.hash(&mut hasher);
            summary.files_changed.len().hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Render a single message to cached lines
    fn render_message_to_cache(
        &self,
        msg: &ChatMessage,
        width: usize,
        add_spacing: bool,
    ) -> CachedMessageLines {
        let mut lines = Vec::new();
        let mut joiner_before = Vec::new();
        self.format_message_with_joiners(msg, width, &mut lines, &mut joiner_before);
        if add_spacing {
            lines.push(Line::from(""));
            joiner_before.push(None);
        }
        CachedMessageLines {
            lines,
            joiner_before,
            content_hash: Self::compute_message_hash(msg),
        }
    }

    /// Ensure cache is valid for current width, rebuild if needed
    pub(super) fn ensure_cache(&mut self, width: u16) {
        // Check if we need to rebuild cache due to width change
        if self.cache_width != Some(width) {
            self.rebuild_cache(width);
            return;
        }

        // Ensure cache has correct number of entries
        if self.line_cache.entries.len() != self.messages.len() {
            self.rebuild_cache(width);
        }
    }

    /// Rebuild entire cache (called on width change or when cache is invalid)
    fn rebuild_cache(&mut self, width: u16) {
        self.line_cache.entries.clear();
        self.line_cache.total_line_count = 0;

        for i in 0..self.messages.len() {
            let add_spacing = self.should_add_spacing_after(i);
            let cached =
                self.render_message_to_cache(&self.messages[i], width as usize, add_spacing);
            self.line_cache.total_line_count += cached.lines.len();
            self.line_cache.entries.push(Some(cached));
        }

        self.cache_width = Some(width);
        self.flat_cache_dirty = true;
    }

    /// Check if spacing should be added after message at index
    fn should_add_spacing_after(&self, index: usize) -> bool {
        let msg = &self.messages[index];
        let is_summary = msg.role == MessageRole::Summary;
        let next_is_summary = self
            .messages
            .get(index + 1)
            .map(|m| m.role == MessageRole::Summary)
            .unwrap_or(false);
        !is_summary && !next_is_summary
    }

    /// Invalidate cache entry at specific index
    pub(super) fn invalidate_cache_entry(&mut self, index: usize) {
        if index < self.line_cache.entries.len() {
            // Subtract old line count
            if let Some(ref old) = self.line_cache.entries[index] {
                self.line_cache.total_line_count = self
                    .line_cache
                    .total_line_count
                    .saturating_sub(old.lines.len());
            }
            self.line_cache.entries[index] = None;
            self.flat_cache_dirty = true;
        }
    }

    /// Update cache entry at specific index
    pub(super) fn update_cache_entry(&mut self, index: usize, width: u16) {
        if index < self.messages.len() {
            // Subtract old line count if replacing an existing entry
            if let Some(Some(ref old)) = self.line_cache.entries.get(index) {
                self.line_cache.total_line_count = self
                    .line_cache
                    .total_line_count
                    .saturating_sub(old.lines.len());
            }

            let add_spacing = self.should_add_spacing_after(index);
            let cached =
                self.render_message_to_cache(&self.messages[index], width as usize, add_spacing);
            self.line_cache.total_line_count += cached.lines.len();

            if index < self.line_cache.entries.len() {
                self.line_cache.entries[index] = Some(cached);
            } else {
                // Extend if needed
                while self.line_cache.entries.len() < index {
                    self.line_cache.entries.push(None);
                }
                self.line_cache.entries.push(Some(cached));
            }
            self.flat_cache_dirty = true;
        }
    }

    pub(super) fn ensure_flat_cache(&mut self) {
        if !self.flat_cache_dirty && self.flat_cache_width == self.cache_width {
            return;
        }

        self.flat_cache.clear();
        self.flat_cache.reserve(self.line_cache.total_line_count);
        self.joiner_before.clear();
        self.joiner_before.reserve(self.line_cache.total_line_count);
        for cached in self.line_cache.entries.iter().flatten() {
            for (line, joiner) in cached.lines.iter().zip(cached.joiner_before.iter()) {
                // Skip consecutive blank lines to avoid excessive spacing
                let is_blank = is_blank_line(line);
                let last_is_blank = self.flat_cache.last().map(is_blank_line).unwrap_or(false);

                if is_blank && last_is_blank {
                    continue;
                }
                self.flat_cache.push(line.clone());
                self.joiner_before.push(joiner.clone());
            }
        }
        self.flat_cache_width = self.cache_width;
        self.flat_cache_dirty = false;
    }
}

/// Check if a line is blank (empty or only whitespace)
fn is_blank_line(line: &Line<'_>) -> bool {
    if line.spans.is_empty() {
        return true;
    }
    line.spans.iter().all(|span| span.content.trim().is_empty())
}
