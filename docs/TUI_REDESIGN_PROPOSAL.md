# Conduit TUI Redesign Proposal

A modern, polished redesign inspired by tools like Opencode and Neovim.

## Executive Summary

The current Conduit UI is functional but could benefit from:
- **Refined color palette** with better visual hierarchy
- **Rounded borders** for a modern feel
- **Improved spacing** to let content breathe
- **Clearer focus states** across all components
- **More restrained accent colors** (currently overuses Cyan)

---

## Current vs. Proposed Color Palette

### Current Palette (theme.rs)
```rust
// Very dark, flat grays - lacks depth
SELECTED_BG: Rgb(40, 60, 80)      // Bluish selection
SELECTED_BG_DIM: Rgb(30, 30, 30)  // Dark gray
TAB_BAR_BG: Rgb(20, 20, 20)       // Very dark
STATUS_BAR_BG: Rgb(30, 30, 30)    // Dark gray
FOOTER_BG: Rgb(25, 25, 25)        // Very dark
KEY_HINT_BG: Rgb(60, 60, 60)      // Medium gray
INPUT_BG: Rgb(24, 24, 24)         // Almost black
```

### Proposed Palette
```rust
// Background layers (dark to light for depth)
pub const BG_BASE: Color = Color::Rgb(22, 22, 30);       // Deepest - main background
pub const BG_SURFACE: Color = Color::Rgb(30, 30, 40);    // Panels, cards, sidebar
pub const BG_ELEVATED: Color = Color::Rgb(40, 40, 52);   // Modals, dropdowns, hover
pub const BG_HIGHLIGHT: Color = Color::Rgb(50, 55, 70);  // Selection background

// Text hierarchy
pub const TEXT_PRIMARY: Color = Color::Rgb(220, 220, 230);    // Main content - 87% white
pub const TEXT_SECONDARY: Color = Color::Rgb(160, 160, 180);  // Labels, metadata
pub const TEXT_MUTED: Color = Color::Rgb(100, 100, 120);      // Hints, disabled
pub const TEXT_FAINT: Color = Color::Rgb(70, 70, 85);         // Decorative, separators

// Accent colors (use sparingly - 5% of UI)
pub const ACCENT_PRIMARY: Color = Color::Rgb(130, 170, 255);   // Focus, selection - soft blue
pub const ACCENT_SECONDARY: Color = Color::Rgb(180, 140, 255); // Secondary highlights - purple
pub const ACCENT_SUCCESS: Color = Color::Rgb(130, 200, 140);   // Success states
pub const ACCENT_WARNING: Color = Color::Rgb(230, 180, 100);   // Warnings, processing
pub const ACCENT_ERROR: Color = Color::Rgb(230, 120, 120);     // Errors

// Agent-specific (maintain brand identity)
pub const AGENT_CLAUDE: Color = Color::Rgb(130, 180, 220);     // Softer cyan
pub const AGENT_CODEX: Color = Color::Rgb(180, 140, 200);      // Softer magenta

// Borders
pub const BORDER_DEFAULT: Color = Color::Rgb(50, 50, 65);      // Subtle border
pub const BORDER_FOCUSED: Color = Color::Rgb(130, 170, 255);   // Focused element
pub const BORDER_DIMMED: Color = Color::Rgb(35, 35, 45);       // Very subtle
```

---

## Border Treatment

### Current
- Default `Borders::ALL` with sharp corners
- Inconsistent border colors (Cyan for focus, DarkGray otherwise)

### Proposed
```rust
use ratatui::symbols::border;

// Modern rounded borders throughout
fn panel_block(title: &str, focused: bool) -> Block {
    Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(TEXT_SECONDARY).bold())
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)  // ╭╮╰╯ corners
        .border_style(Style::default().fg(
            if focused { BORDER_FOCUSED } else { BORDER_DEFAULT }
        ))
}

// Alternative for dense UIs: clean single lines
fn minimal_block() -> Block {
    Block::default()
        .borders(Borders::ALL)
        .border_set(border::PLAIN)
        .border_style(Style::default().fg(BORDER_DIMMED))
}
```

---

## Component Redesigns

### 1. Tab Bar

**Current Issues:**
- Hard `▶` indicator, basic colors
- Dense with no visual separation
- PR badges use harsh `Blue` background

**Proposed Design:**
```
╭─────────────────────────────────────────────────────────────────────────────╮
│  1  conduit          2  my-project  ●        3  feature-x  ⠋    [+] New    │
╰─────────────────────────────────────────────────────────────────────────────╯
   ▔▔▔▔▔▔▔▔▔▔▔                                    ↑ processing
   underline for                                    ↑ attention dot
   active tab
```

```rust
// Refined tab bar styling
impl TabBar {
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Background with subtle top border
        let bg_style = Style::default().bg(BG_SURFACE);

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active;

            // Tab number - muted
            let num_style = Style::default()
                .fg(if is_active { TEXT_PRIMARY } else { TEXT_MUTED });

            // Tab name
            let name_style = if is_active {
                Style::default().fg(TEXT_PRIMARY).bold()
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            // Active indicator: underline instead of arrow
            if is_active && self.focused {
                // Draw subtle underline below tab name
                // Using ▔ character or bottom border
            }

            // Processing spinner - softer yellow
            if is_processing {
                let spinner_style = Style::default().fg(ACCENT_WARNING);
            }

            // Attention dot - subtle green
            if needs_attention {
                let dot_style = Style::default().fg(ACCENT_SUCCESS);
            }

            // PR badge - toned down
            if let Some(pr) = pr_number {
                let badge_style = Style::default()
                    .fg(TEXT_PRIMARY)
                    .bg(BG_ELEVATED);  // Not harsh blue
            }
        }

        // [+] New button - subtle
        let add_style = Style::default().fg(TEXT_MUTED);
    }
}
```

### 2. Status Bar

**Current Issues:**
- Very dense information
- Uses pipe `│` separators
- Harsh color contrast

**Proposed Design:**
```
╭─────────────────────────────────────────────────────────────────────────────╮
│  ◆ Claude   Sonnet 4    abc12345   │  in: 12.4k  out: 3.2k  │  $0.0234    │
╰─────────────────────────────────────────────────────────────────────────────╯
   ↑ agent     ↑ model    ↑ session     ↑ tokens                 ↑ cost
   badge                   (dimmed)      secondary text           color-coded
```

```rust
impl StatusBar {
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let bg = Style::default().bg(BG_SURFACE);

        // Agent badge - softer colors with icon
        let agent_style = Style::default()
            .fg(BG_BASE)  // Dark text on colored bg
            .bg(match self.agent_type {
                AgentType::Claude => AGENT_CLAUDE,
                AgentType::Codex => AGENT_CODEX,
            })
            .bold();

        // Model name - primary text
        let model_style = Style::default().fg(TEXT_PRIMARY);

        // Session ID - very muted
        let session_style = Style::default().fg(TEXT_MUTED);

        // Separator - subtle vertical line with spacing
        // " │ " with TEXT_FAINT color

        // Token labels - muted
        let label_style = Style::default().fg(TEXT_MUTED);

        // Token values - secondary
        let value_style = Style::default().fg(TEXT_SECONDARY);

        // Cost - color based on threshold
        let cost_style = Style::default().fg(
            if self.estimated_cost > 0.50 { ACCENT_ERROR }
            else if self.estimated_cost > 0.10 { ACCENT_WARNING }
            else { ACCENT_SUCCESS }
        );

        // Processing indicator
        if self.is_processing {
            // Softer yellow, less intrusive
            let thinking_style = Style::default().fg(ACCENT_WARNING);
        }
    }
}
```

### 3. Sidebar

**Current Issues:**
- Sharp border corners
- Cyan focus color is harsh
- Selection background is too blue

**Proposed Design:**
```
╭─ Workspaces ──────────────────────╮
│                                   │
│  ▾ my-project                     │
│    ├─ main ●                      │
│    └─ feature-branch              │
│                                   │
│  ▸ another-repo                   │
│                                   │
╰───────────────────────────────────╯
```

```rust
impl Sidebar {
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Rounded border with subtle focus indication
        let border_color = if state.focused {
            ACCENT_PRIMARY  // Soft blue, not harsh cyan
        } else {
            BORDER_DEFAULT
        };

        let block = Block::default()
            .title(" Workspaces ")
            .title_style(Style::default().fg(TEXT_SECONDARY))
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(Style::default().fg(border_color))
            .bg(BG_SURFACE);  // Slightly lighter than main bg

        // Tree view styling
        let selected_style = Style::default()
            .bg(if state.focused { BG_HIGHLIGHT } else { BG_ELEVATED })
            .fg(TEXT_PRIMARY);

        // Tree connectors - very subtle
        let connector_style = Style::default().fg(TEXT_FAINT);
        // Use ├─ └─ ▾ ▸ characters
    }
}
```

### 4. Chat View

**Current Issues:**
- No visual distinction between message types
- Scrollbar could be more refined
- Code blocks blend in

**Proposed Design:**
```
╭─────────────────────────────────────────────────────────────────────────────╮
│                                                                             │
│  You                                                            10:42 AM   │
│  Can you help me understand this code?                                     │
│                                                                             │
│ ─────────────────────────────────────────────────────────────────────────  │
│                                                                             │
│  Claude                                                         10:42 AM   │
│  I'd be happy to help! Looking at the code...                              │
│                                                                             │
│  ╭─ src/main.rs ──────────────────────────────────────────────────────╮   │
│  │ fn main() {                                                         │   │
│  │     println!("Hello, world!");                                      │   │
│  │ }                                                                   │   │
│  ╰─────────────────────────────────────────────────────────────────────╯   │
│                                                                             │
╰─────────────────────────────────────────────────────────────────────────────╯
```

```rust
// Message role styling
fn role_style(role: &str) -> Style {
    match role {
        "user" | "You" => Style::default()
            .fg(ACCENT_PRIMARY)
            .bold(),
        "assistant" | "Claude" => Style::default()
            .fg(AGENT_CLAUDE)
            .bold(),
        _ => Style::default().fg(TEXT_SECONDARY),
    }
}

// Timestamp - very muted, right-aligned
let timestamp_style = Style::default().fg(TEXT_FAINT);

// Code block container
fn code_block(filename: Option<&str>) -> Block {
    Block::default()
        .title(filename.map(|f| format!(" {} ", f)).unwrap_or_default())
        .title_style(Style::default().fg(TEXT_MUTED))
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(BORDER_DIMMED))
        .bg(BG_BASE)  // Slightly darker for code
}

// Scrollbar - minimal
fn scrollbar_style() -> Style {
    Style::default().fg(TEXT_FAINT)
}
// Use thin characters: │ for track, ┃ or █ for thumb
```

### 5. Input Box

**Current Issues:**
- Background too dark (almost black)
- No clear boundaries

**Proposed Design:**
```
╭─────────────────────────────────────────────────────────────────────────────╮
│ Type a message...                                                        │ │
│                                                                          │ │
│                                                                          ▼ │
╰─────────────────────────────────────────────────────────────────────────────╯
```

```rust
fn input_box_style(focused: bool) -> Block {
    Block::default()
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(
            if focused { ACCENT_PRIMARY } else { BORDER_DEFAULT }
        ))
        .bg(BG_SURFACE)
}

// Placeholder text
let placeholder_style = Style::default().fg(TEXT_MUTED);

// Cursor - subtle blinking block or line
let cursor_style = Style::default().bg(ACCENT_PRIMARY);
```

### 6. Dialogs

**Current Issues:**
- Cyan border feels dated
- Instruction bar could be cleaner

**Proposed Design:**
```
                    ╭─ Select Project ─────────────────────╮
                    │                                      │
                    │   ▸ my-project                       │
                    │     another-repo                     │
                    │     third-project                    │
                    │                                      │
                    │   ↑/↓ Navigate   Enter Select   Esc  │
                    ╰──────────────────────────────────────╯
```

```rust
fn dialog_frame(title: &str, focused: bool) -> Block {
    Block::default()
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(TEXT_PRIMARY).bold())
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(
            if focused { ACCENT_PRIMARY } else { BORDER_DEFAULT }
        ))
        .bg(BG_ELEVATED)  // Elevated above main content
}

// Instruction bar - integrated into dialog bottom
fn instruction_bar_style() -> Style {
    Style::default()
        .fg(TEXT_MUTED)
        .bg(BG_SURFACE)
}

// Key hints
fn key_style() -> Style {
    Style::default()
        .fg(TEXT_SECONDARY)
        .bg(BG_ELEVATED)
        .bold()
}
```

### 7. Global Footer

**Current Issues:**
- Key hints have harsh background contrast
- Too visually heavy

**Proposed Design:**
```
  Tab Switch   C-t Sidebar   C-n Project   C-q Quit
  ▔▔▔          ▔▔▔           ▔▔▔           ▔▔▔
  keys are     actions are
  highlighted  muted
```

```rust
fn render_key_hints(hints: &[(&str, &str)], area: Rect, buf: &mut Buffer) {
    let bg = Style::default().bg(BG_BASE);

    for (key, action) in hints {
        // Key - subtle highlight
        let key_style = Style::default()
            .fg(TEXT_PRIMARY)
            .bg(BG_ELEVATED)
            .bold();

        // Action - muted
        let action_style = Style::default().fg(TEXT_MUTED);

        // Spacing between pairs
        // "  " (2 spaces)
    }
}
```

---

## Layout Improvements

### Increased Padding

```rust
// Inner content padding
fn content_area(outer: Rect) -> Rect {
    outer.inner(Margin { horizontal: 1, vertical: 1 })
}

// Gap between sidebar and main content
let layout = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([
        Constraint::Length(sidebar_width),
        Constraint::Length(1),  // 1-char gap
        Constraint::Min(50),
    ])
    .split(area);
```

### Visual Separators

Instead of hard borders between sections, use subtle horizontal lines:

```rust
// Subtle separator line
fn render_separator(area: Rect, buf: &mut Buffer) {
    let line = "─".repeat(area.width as usize);
    let style = Style::default().fg(BORDER_DIMMED);
    buf.set_string(area.x, area.y, &line, style);
}
```

---

## Animation Refinements

### Thinking Indicator

**Current:** Cycling words + shimmer effect (good!)

**Proposed refinement:**
```rust
// Softer color gradient
let shimmer_start = ACCENT_WARNING;  // Rgb(230, 180, 100)
let shimmer_end = Color::Rgb(80, 60, 40);  // Darker, warmer

// Spinner - use subtle dots
const SPINNER: &[&str] = &["·", "∙", "●", "∙"];  // Or keep current braille
```

### Tab Processing Spinner

Keep current braille spinner but use `ACCENT_WARNING` instead of raw `Yellow`.

---

## Implementation Priority

### Phase 1: Core Colors (Low Risk)
1. Update `theme.rs` with new color constants
2. Replace hardcoded colors with theme constants
3. Test across different terminals

### Phase 2: Border Treatment (Medium Risk)
1. Add `border::ROUNDED` to all Block widgets
2. Implement focus-aware border styling
3. Update dialog frames

### Phase 3: Component Polish (Higher Effort)
1. Refine tab bar with underline selection
2. Clean up status bar density
3. Improve chat message styling
4. Polish input box

### Phase 4: Spacing & Layout (Lower Priority)
1. Add consistent padding
2. Implement visual separators
3. Fine-tune responsive behavior

---

## Terminal Compatibility Notes

- **RGB colors**: Require truecolor terminal support
- **Rounded borders**: Require Unicode support (most modern terminals)
- **Fallback**: Consider 16-color fallback for basic terminals

```rust
fn supports_truecolor() -> bool {
    std::env::var("COLORTERM")
        .map(|v| v == "truecolor" || v == "24bit")
        .unwrap_or(false)
}
```

---

## Before/After Comparison (Conceptual)

### Before
```
 ▶ [1] conduit  [2] my-project  ● [+] New
┌─────────────────────────────────────────┐
│ Workspaces                              │
│  my-project                             │
│    main                                 │
└─────────────────────────────────────────┘
 Claude Sonnet │ in:12k out:3k │ $0.02
 Tab Switch │ C-t Sidebar │ C-q Quit
```

### After
```
   1  conduit          2  my-project  ●         [+] New
  ▔▔▔▔▔▔▔▔▔▔▔▔
╭─ Workspaces ──────────────────────────────────────────╮
│                                                       │
│  ▾ my-project                                         │
│    └─ main                                            │
│                                                       │
╰───────────────────────────────────────────────────────╯

  ◆ Claude   Sonnet 4   │  in: 12k  out: 3k  │  $0.02

  Tab Switch   C-t Sidebar   C-q Quit
```

---

## Summary

This redesign focuses on:

1. **Visual hierarchy** through layered backgrounds (base → surface → elevated)
2. **Restrained color** with 80% neutrals, 15% structure, 5% accent
3. **Modern borders** using rounded corners consistently
4. **Generous whitespace** with proper padding and gaps
5. **Clear focus states** using soft blue accent instead of harsh cyan
6. **Consistent styling** across all components

The changes are incremental and can be rolled out in phases without breaking functionality.
