//! Agent selector dialog component

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::agent::AgentType;

use super::{DialogFrame, InstructionBar, SELECTED_BG};

/// State for the agent selector dialog
#[derive(Debug, Clone)]
pub struct AgentSelectorState {
    /// Whether the dialog is visible
    pub visible: bool,
    /// Currently selected index (0 = Claude, 1 = Codex)
    pub selected: usize,
    /// Available agents
    agents: Vec<AgentOption>,
}

#[derive(Debug, Clone)]
struct AgentOption {
    agent_type: AgentType,
    name: &'static str,
    description: &'static str,
}

impl Default for AgentSelectorState {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentSelectorState {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected: 0,
            agents: vec![
                AgentOption {
                    agent_type: AgentType::Claude,
                    name: "Claude Code",
                    description: "Anthropic's coding assistant",
                },
                AgentOption {
                    agent_type: AgentType::Codex,
                    name: "Codex CLI",
                    description: "OpenAI's code generation model",
                },
            ],
        }
    }

    /// Show the dialog
    pub fn show(&mut self) {
        self.visible = true;
        self.selected = 0;
    }

    /// Hide the dialog
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.agents.len() - 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % self.agents.len();
    }

    /// Get the currently selected agent type
    pub fn selected_agent(&self) -> AgentType {
        self.agents[self.selected].agent_type
    }

    /// Check if dialog is visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

/// Agent selector dialog widget
pub struct AgentSelector;

impl AgentSelector {
    pub fn new() -> Self {
        Self
    }

    /// Render the dialog
    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &AgentSelectorState) {
        if !state.visible {
            return;
        }

        // Render dialog frame
        let frame = DialogFrame::new("Select Agent", 44, 9);
        let inner = frame.render(area, buf);

        // Layout inside dialog
        let chunks = Layout::vertical([
            Constraint::Length(1), // Header
            Constraint::Length(1), // Spacing
            Constraint::Length(2), // Claude option
            Constraint::Length(2), // Codex option
            Constraint::Length(1), // Instructions
        ])
        .split(inner);

        // Render header
        let header = Paragraph::new("Choose an agent:")
            .style(Style::default().fg(Color::White));
        header.render(chunks[0], buf);

        // Render agent options
        for (i, agent) in state.agents.iter().enumerate() {
            let chunk_idx = i + 2; // Skip header and spacing
            if chunk_idx >= chunks.len() - 1 {
                break;
            }

            let is_selected = i == state.selected;

            // Agent name line
            let name_line = Line::from(vec![
                Span::styled(
                    if is_selected { " ▶ " } else { "   " },
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    agent.name,
                    if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ]);

            // Description line
            let desc_line = Line::from(vec![
                Span::raw("     "),
                Span::styled(agent.description, Style::default().fg(Color::DarkGray)),
            ]);

            let option = Paragraph::new(vec![name_line, desc_line]);
            option.render(chunks[chunk_idx], buf);

            // Highlight background for selected
            if is_selected {
                for dy in 0..2 {
                    let row_y = chunks[chunk_idx].y + dy;
                    if row_y < chunks[chunk_idx].y + chunks[chunk_idx].height {
                        for dx in 0..chunks[chunk_idx].width {
                            buf[(chunks[chunk_idx].x + dx, row_y)]
                                .set_bg(SELECTED_BG);
                        }
                    }
                }
            }
        }

        // Render instructions
        let instructions = InstructionBar::new(vec![
            ("↑↓", "select"),
            ("Enter", "confirm"),
            ("Esc", "cancel"),
        ]);
        instructions.render(chunks[4], buf);
    }
}

impl Default for AgentSelector {
    fn default() -> Self {
        Self::new()
    }
}
