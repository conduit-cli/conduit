//! Terminal state guard for guaranteed cleanup.
//!
//! This module provides a RAII guard that ensures terminal state is restored
//! when the application exits, whether normally, via early return, or panic.

use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, PopKeyboardEnhancementFlags},
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use std::io::{self, Write};

/// Guard that restores terminal state when dropped.
///
/// This ensures terminal cleanup happens regardless of how the application exits:
/// - Normal return
/// - Early `?` error propagation
/// - Panic (when combined with panic hook)
pub struct TerminalGuard {
    keyboard_enhancement_enabled: bool,
    active: bool,
}

impl TerminalGuard {
    /// Create a new terminal guard.
    ///
    /// The guard should be created AFTER enabling raw mode and keyboard enhancements,
    /// so that Drop will clean them up if needed.
    pub fn new(keyboard_enhancement_enabled: bool) -> Self {
        Self {
            keyboard_enhancement_enabled,
            active: true,
        }
    }

    /// Perform manual cleanup and prevent Drop from running cleanup again.
    ///
    /// Call this for explicit cleanup with error handling.
    /// After calling this, Drop becomes a no-op.
    pub fn cleanup(&mut self) -> anyhow::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        self.do_cleanup()
    }

    /// Cleanup terminal state for suspend/editor flows while keeping guard active.
    pub fn cleanup_for_suspend(&self) -> anyhow::Result<()> {
        self.do_cleanup()
    }

    fn do_cleanup(&self) -> anyhow::Result<()> {
        let mut stdout = io::stdout();
        if self.keyboard_enhancement_enabled {
            // Pop keyboard enhancement flags and flush immediately to ensure
            // the terminal receives this before we change terminal modes
            if let Err(e) = execute!(stdout, PopKeyboardEnhancementFlags) {
                tracing::debug!(
                    error = %e,
                    "Failed to pop keyboard enhancement flags during cleanup"
                );
            }
            if let Err(e) = stdout.flush() {
                tracing::debug!(error = %e, "Failed to flush stdout during cleanup");
            }
        }
        disable_raw_mode()?;
        execute!(
            stdout,
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste
        )?;
        stdout.flush()?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.active {
            // Best effort cleanup - ignore errors since we can't propagate them from Drop
            if let Err(e) = self.do_cleanup() {
                tracing::debug!(error = %e, "Terminal cleanup failed in Drop");
            }
        }
    }
}

/// Install a panic hook that restores terminal state before printing the panic message.
///
/// This should be called early in main() before any terminal setup.
/// The hook will:
/// 1. Restore terminal to normal state
/// 2. Call the original panic hook to print the panic message
pub fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Best-effort terminal restoration before panic message is printed
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, PopKeyboardEnhancementFlags) {
            tracing::debug!(error = %e, "Failed to pop keyboard enhancement flags in panic hook");
        }
        if let Err(e) = stdout.flush() {
            tracing::debug!(error = %e, "Failed to flush stdout in panic hook");
        }
        if let Err(e) = disable_raw_mode() {
            tracing::debug!(error = %e, "Failed to disable raw mode in panic hook");
        }
        if let Err(e) = execute!(
            stdout,
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste
        ) {
            tracing::debug!(error = %e, "Failed to restore terminal screen in panic hook");
        }
        if let Err(e) = stdout.flush() {
            tracing::debug!(error = %e, "Failed to flush stdout after panic cleanup");
        }

        // Now call the original hook to print the panic
        original_hook(panic_info);
    }));
}
