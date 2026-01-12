//! Inline prompt component for AskUserQuestion and ExitPlanMode tools
//!
//! Emulates Claude Code CLI's inline UI patterns for interactive tool responses.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use super::{
    accent_primary, accent_secondary, text_faint, text_muted, text_primary, text_secondary,
    InstructionBar, TextInputState,
};
use crate::agent::events::{QuestionOption, UserQuestion};

// ============================================================================
// Constants
// ============================================================================

/// Selection indicator character (heavy right-pointing angle)
const SELECTOR: &str = "❯";

/// Unchecked question indicator
const UNCHECKED: &str = "☐";

/// Checked question indicator
const CHECKED: &str = "✔";

/// Left arrow for tab navigation
const LEFT_ARROW: &str = "←";

/// Right arrow for tab navigation
const RIGHT_ARROW: &str = "→";

/// Horizontal line character for separators
const HORIZONTAL_LINE: char = '─';

/// Dashed line character for plan box
const DASHED_LINE: char = '╌';

// ============================================================================
// Types
// ============================================================================

/// The type of inline prompt being displayed
#[derive(Debug, Clone)]
pub enum InlinePromptType {
    /// AskUserQuestion with one or more questions
    AskUserQuestion { questions: Vec<UserQuestion> },
    /// ExitPlanMode with plan content
    ExitPlanMode {
        plan_content: String,
        plan_file_path: String,
    },
}

/// User's answer to a single question
#[derive(Debug, Clone)]
pub enum QuestionAnswer {
    /// Selected a predefined option (by index)
    Selected(usize),
    /// Selected multiple options (by index)
    MultiSelected(Vec<usize>),
    /// Provided custom text via "Type something"
    Custom(String),
}

/// Action resulting from key handling
#[derive(Debug, Clone)]
pub enum PromptAction {
    /// User submitted a response
    Submit(PromptResponse),
    /// User cancelled the prompt
    Cancel,
    /// Key was handled but no action yet
    Consumed,
    /// Key was not handled
    NotHandled,
}

/// The response to send back to the agent
#[derive(Debug, Clone)]
pub enum PromptResponse {
    /// Answers to AskUserQuestion
    AskUserAnswers {
        /// Map of question text to selected answers
        answers: std::collections::HashMap<String, PromptAnswer>,
    },
    /// Approval for ExitPlanMode
    ExitPlanApprove,
    /// Feedback for ExitPlanMode (stay in plan mode)
    ExitPlanFeedback(String),
}

/// Answer value for AskUserQuestion
#[derive(Debug, Clone)]
pub enum PromptAnswer {
    /// Single answer (option label or custom text)
    Single(String),
    /// Multiple selected answers
    Multiple(Vec<String>),
}

/// State for the inline prompt component
#[derive(Debug, Clone)]
pub struct InlinePromptState {
    /// Tool ID to send response to
    pub tool_id: String,
    /// Type of prompt (AskUser or ExitPlan)
    pub prompt_type: InlinePromptType,
    /// Currently selected option index (within current question or ExitPlan options)
    pub current_option: usize,
    /// For AskUserQuestion: index of current question being answered
    pub current_question_idx: usize,
    /// For AskUserQuestion: answers for each question (None if not yet answered)
    pub answers: Vec<Option<QuestionAnswer>>,
    /// Whether we're in text input mode (for "Type something" or feedback)
    pub input_mode: bool,
    /// Text input state for custom responses
    pub text_input: TextInputState,
}

impl InlinePromptState {
    /// Create a new AskUserQuestion prompt
    pub fn new_ask_user(tool_id: String, questions: Vec<UserQuestion>) -> Self {
        let questions: Vec<UserQuestion> = questions
            .into_iter()
            .enumerate()
            .map(|(idx, mut question)| {
                if question.header.trim().is_empty() {
                    let fallback: String = question.question.chars().take(12).collect();
                    let fallback = fallback.trim().to_string();
                    question.header = if fallback.is_empty() {
                        format!("Q{}", idx + 1)
                    } else {
                        fallback
                    };
                }
                question
            })
            .collect();
        let num_questions = questions.len();
        Self {
            tool_id,
            prompt_type: InlinePromptType::AskUserQuestion { questions },
            current_option: 0,
            current_question_idx: 0,
            answers: vec![None; num_questions],
            input_mode: false,
            text_input: TextInputState::new(),
        }
    }

    /// Create a new ExitPlanMode prompt
    pub fn new_exit_plan(tool_id: String, plan_content: String, plan_file_path: String) -> Self {
        Self {
            tool_id,
            prompt_type: InlinePromptType::ExitPlanMode {
                plan_content,
                plan_file_path,
            },
            current_option: 0,
            current_question_idx: 0,
            answers: vec![],
            input_mode: false,
            text_input: TextInputState::new(),
        }
    }

    /// Get the current question (for AskUserQuestion only)
    fn current_question(&self) -> Option<&UserQuestion> {
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                questions.get(self.current_question_idx)
            }
            _ => None,
        }
    }

    fn current_question_multi_select(&self) -> bool {
        self.current_question().is_some_and(|q| q.multi_select)
    }

    /// Check if we should show a Submit tab (for multi-select questions)
    fn has_submit_tab(&self) -> bool {
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                questions.iter().any(|q| q.multi_select)
            }
            _ => false,
        }
    }

    /// Check if the current tab is the Submit tab
    fn is_submit_tab(&self) -> bool {
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                self.has_submit_tab() && self.current_question_idx >= questions.len()
            }
            _ => false,
        }
    }

    /// Check if a specific question has been answered
    fn question_answered(&self, idx: usize) -> bool {
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                if questions.get(idx).is_none() {
                    return false;
                }
                match self.answers.get(idx) {
                    Some(Some(QuestionAnswer::Custom(text))) => !text.is_empty(),
                    Some(Some(QuestionAnswer::Selected(_))) => true,
                    Some(Some(QuestionAnswer::MultiSelected(items))) => !items.is_empty(),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Get the number of options for the current context
    fn option_count(&self) -> usize {
        if self.is_submit_tab() {
            return 0;
        }
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                questions
                    .get(self.current_question_idx)
                    .map(|q| q.options.len() + 1) // +1 for "Type something"
                    .unwrap_or(0)
            }
            InlinePromptType::ExitPlanMode { .. } => 2, // "Yes, start building" and "Type here..."
        }
    }

    /// Check if current selection is on "Type something" option
    fn is_type_something_selected(&self) -> bool {
        if self.is_submit_tab() {
            return false;
        }
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                if let Some(q) = questions.get(self.current_question_idx) {
                    self.current_option == q.options.len()
                } else {
                    false
                }
            }
            InlinePromptType::ExitPlanMode { .. } => self.current_option == 1,
        }
    }

    /// Handle a key event, returning the resulting action
    pub fn handle_key(&mut self, key: KeyEvent) -> PromptAction {
        // If in input mode, handle text input
        if self.input_mode {
            return self.handle_input_mode_key(key);
        }

        match key.code {
            // Navigation
            KeyCode::Up | KeyCode::Char('k') => {
                if self.current_option > 0 {
                    self.current_option -= 1;
                }
                PromptAction::Consumed
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.current_option < self.option_count().saturating_sub(1) {
                    self.current_option += 1;
                }
                PromptAction::Consumed
            }

            // Question navigation (for multi-question AskUser)
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => {
                if let InlinePromptType::AskUserQuestion { questions } = &self.prompt_type {
                    if questions.len() > 1 || self.has_submit_tab() {
                        if self.current_question_idx > 0 {
                            self.current_question_idx -= 1;
                        } else if self.has_submit_tab() {
                            self.current_question_idx = questions.len();
                        }
                        self.current_option = 0;
                    }
                }
                PromptAction::Consumed
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                if let InlinePromptType::AskUserQuestion { questions } = &self.prompt_type {
                    if questions.len() > 1 || self.has_submit_tab() {
                        let last_idx = if self.has_submit_tab() {
                            questions.len()
                        } else {
                            questions.len().saturating_sub(1)
                        };
                        if self.current_question_idx < last_idx {
                            self.current_question_idx += 1;
                        } else if self.has_submit_tab() {
                            self.current_question_idx = 0;
                        }
                        self.current_option = 0;
                    }
                }
                PromptAction::Consumed
            }

            // Quick select by number
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < self.option_count() {
                    self.current_option = idx;
                    // If it's "Type something", enter input mode
                    if self.is_type_something_selected() {
                        self.input_mode = true;
                        self.text_input.clear();
                    } else if self.current_question_multi_select() {
                        self.toggle_current_option();
                    } else {
                        // Select this option
                        return self.select_current_option();
                    }
                }
                PromptAction::Consumed
            }

            // Select current option
            KeyCode::Enter => {
                if self.is_submit_tab() {
                    if self.all_questions_answered() {
                        self.build_ask_user_response()
                    } else {
                        PromptAction::Consumed
                    }
                } else if self.is_type_something_selected() {
                    self.input_mode = true;
                    self.text_input.clear();
                    PromptAction::Consumed
                } else if self.current_question_multi_select() {
                    self.toggle_current_option();
                    PromptAction::Consumed
                } else {
                    self.select_current_option()
                }
            }

            // Cancel
            KeyCode::Esc => PromptAction::Cancel,

            // Toggle selection for multi-select
            KeyCode::Char(' ') => {
                if self.is_submit_tab() {
                    PromptAction::Consumed
                } else if self.is_type_something_selected() {
                    self.input_mode = true;
                    self.text_input.clear();
                    PromptAction::Consumed
                } else if self.current_question_multi_select() {
                    self.toggle_current_option();
                    PromptAction::Consumed
                } else {
                    PromptAction::NotHandled
                }
            }

            // Any other character - if on "Type something", enter input mode
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.is_type_something_selected() {
                    self.input_mode = true;
                    self.text_input.clear();
                    self.text_input.insert_char(c);
                    PromptAction::Consumed
                } else {
                    PromptAction::NotHandled
                }
            }

            _ => PromptAction::NotHandled,
        }
    }

    /// Handle key events in text input mode
    fn handle_input_mode_key(&mut self, key: KeyEvent) -> PromptAction {
        match key.code {
            KeyCode::Esc => {
                // Exit input mode, go back to option selection
                self.input_mode = false;
                self.text_input.clear();
                PromptAction::Consumed
            }
            KeyCode::Enter => {
                // Submit the text
                let text = self.text_input.value().to_string();
                if text.is_empty() {
                    // Don't submit empty text, just go back
                    self.input_mode = false;
                    PromptAction::Consumed
                } else {
                    self.submit_text_input(text)
                }
            }
            KeyCode::Backspace => {
                if self.text_input.is_empty() {
                    // If empty, exit input mode
                    self.input_mode = false;
                } else {
                    self.text_input.delete_char();
                }
                PromptAction::Consumed
            }
            KeyCode::Delete => {
                self.text_input.delete_forward();
                PromptAction::Consumed
            }
            KeyCode::Left => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.text_input.move_word_left();
                } else {
                    self.text_input.move_left();
                }
                PromptAction::Consumed
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.text_input.move_word_right();
                } else {
                    self.text_input.move_right();
                }
                PromptAction::Consumed
            }
            KeyCode::Home => {
                self.text_input.move_start();
                PromptAction::Consumed
            }
            KeyCode::End => {
                self.text_input.move_end();
                PromptAction::Consumed
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text_input.delete_to_start();
                PromptAction::Consumed
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text_input.delete_to_end();
                PromptAction::Consumed
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text_input.delete_word();
                PromptAction::Consumed
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text_input.move_start();
                PromptAction::Consumed
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text_input.move_end();
                PromptAction::Consumed
            }
            KeyCode::Char(c) => {
                self.text_input.insert_char(c);
                PromptAction::Consumed
            }
            _ => PromptAction::Consumed,
        }
    }

    fn toggle_current_option(&mut self) {
        if let InlinePromptType::AskUserQuestion { questions } = &self.prompt_type {
            let Some(question) = questions.get(self.current_question_idx) else {
                return;
            };
            if self.current_option >= question.options.len() {
                return;
            }

            let mut selections = match self.answers.get(self.current_question_idx).cloned() {
                Some(Some(QuestionAnswer::MultiSelected(items))) => items,
                Some(Some(QuestionAnswer::Selected(idx))) => vec![idx],
                _ => Vec::new(),
            };

            if let Some(pos) = selections
                .iter()
                .position(|idx| *idx == self.current_option)
            {
                selections.remove(pos);
            } else {
                selections.push(self.current_option);
            }
            selections.sort_unstable();

            if selections.is_empty() {
                self.answers[self.current_question_idx] = None;
            } else {
                self.answers[self.current_question_idx] =
                    Some(QuestionAnswer::MultiSelected(selections));
            }
        }
    }

    /// Select the current option and return appropriate action
    fn select_current_option(&mut self) -> PromptAction {
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                // Record the answer
                if let Some(question) = questions.get(self.current_question_idx) {
                    if question.multi_select {
                        self.toggle_current_option();
                        return PromptAction::Consumed;
                    }
                    if self.current_option < question.options.len() {
                        self.answers[self.current_question_idx] =
                            Some(QuestionAnswer::Selected(self.current_option));
                    }
                }

                // Check if all questions are answered
                if self.all_questions_answered() && !self.has_submit_tab() {
                    self.build_ask_user_response()
                } else {
                    // Move to next unanswered question
                    self.move_to_next_unanswered();
                    PromptAction::Consumed
                }
            }
            InlinePromptType::ExitPlanMode { .. } => {
                if self.current_option == 0 {
                    // "Yes, start building"
                    PromptAction::Submit(PromptResponse::ExitPlanApprove)
                } else {
                    // "Type here..." - enter input mode
                    self.input_mode = true;
                    self.text_input.clear();
                    PromptAction::Consumed
                }
            }
        }
    }

    /// Submit text input and return appropriate action
    fn submit_text_input(&mut self, text: String) -> PromptAction {
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { .. } => {
                // Record custom answer
                self.answers[self.current_question_idx] = Some(QuestionAnswer::Custom(text));

                // Check if all answered
                if self.all_questions_answered() && !self.has_submit_tab() {
                    self.build_ask_user_response()
                } else {
                    self.input_mode = false;
                    if !self.has_submit_tab() {
                        self.move_to_next_unanswered();
                    }
                    PromptAction::Consumed
                }
            }
            InlinePromptType::ExitPlanMode { .. } => {
                PromptAction::Submit(PromptResponse::ExitPlanFeedback(text))
            }
        }
    }

    /// Check if all questions have been answered
    fn all_questions_answered(&self) -> bool {
        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                (0..questions.len()).all(|idx| self.question_answered(idx))
            }
            _ => false,
        }
    }

    /// Move to the next unanswered question
    fn move_to_next_unanswered(&mut self) {
        if let InlinePromptType::AskUserQuestion { questions } = &self.prompt_type {
            for i in 0..questions.len() {
                let idx = (self.current_question_idx + 1 + i) % questions.len();
                if !self.question_answered(idx) {
                    self.current_question_idx = idx;
                    self.current_option = 0;
                    return;
                }
            }
        }
    }

    /// Build the final response for AskUserQuestion
    fn build_ask_user_response(&self) -> PromptAction {
        if let InlinePromptType::AskUserQuestion { questions } = &self.prompt_type {
            let mut answers = std::collections::HashMap::new();
            for (i, answer) in self.answers.iter().enumerate() {
                if let (Some(question), Some(ans)) = (questions.get(i), answer) {
                    let value = match ans {
                        QuestionAnswer::Selected(idx) => PromptAnswer::Single(
                            question
                                .options
                                .get(*idx)
                                .map(|o| o.label.clone())
                                .unwrap_or_default(),
                        ),
                        QuestionAnswer::MultiSelected(indices) => {
                            let mut labels = Vec::new();
                            for (opt_idx, opt) in question.options.iter().enumerate() {
                                if indices.contains(&opt_idx) {
                                    labels.push(opt.label.clone());
                                }
                            }
                            PromptAnswer::Multiple(labels)
                        }
                        QuestionAnswer::Custom(text) => PromptAnswer::Single(text.clone()),
                    };
                    answers.insert(question.question.clone(), value);
                }
            }
            PromptAction::Submit(PromptResponse::AskUserAnswers { answers })
        } else {
            PromptAction::NotHandled
        }
    }

    // ========================================================================
    // Line-based rendering (for scrollable chat integration)
    // ========================================================================

    /// Render the prompt as lines for inclusion in scrollable chat.
    /// This is an alternative to the Widget-based rendering that allows
    /// the prompt to be part of the conversation flow.
    pub fn render_as_lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        match &self.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                // Separator line
                lines.push(self.separator_line(width));

                // Tab bar for multi-question or submit mode
                let show_tab_bar = questions.len() > 1 || self.has_submit_tab();
                if show_tab_bar {
                    lines.push(self.tab_bar_line(questions));
                    lines.push(Line::from("")); // blank line
                }

                if self.is_submit_tab() {
                    // Submit summary view
                    self.append_submit_view_lines(&mut lines, questions);
                } else if let Some(question) = self.current_question() {
                    // Question content
                    self.append_question_lines(&mut lines, question, questions.len() > 1);
                }
            }
            InlinePromptType::ExitPlanMode {
                plan_content,
                plan_file_path,
            } => {
                // Header
                lines.push(Line::from(Span::styled(
                    " Here is Claude's plan:",
                    Style::default().fg(text_primary()),
                )));

                // Top dashed line
                lines.push(self.dashed_line(width));

                // Plan content (limited to 15 lines)
                let plan_style = Style::default().fg(text_secondary());
                for line in plan_content.lines().take(15) {
                    lines.push(Line::from(Span::styled(format!(" {}", line), plan_style)));
                }

                // Bottom dashed line
                lines.push(self.dashed_line(width));
                lines.push(Line::from("")); // blank line

                // Question
                lines.push(Line::from(Span::styled(
                    " Would you like to proceed?",
                    Style::default().fg(text_primary()),
                )));
                lines.push(Line::from("")); // blank line

                // Options
                lines.push(self.exit_plan_option_line(
                    0,
                    "Yes, start building",
                    self.current_option == 0,
                ));
                lines.push(self.exit_plan_option_line(
                    1,
                    "Give feedback...",
                    self.current_option == 1,
                ));

                if self.input_mode {
                    lines.push(Line::from("")); // blank line
                    lines.push(self.text_input_line());
                    lines.push(Line::from("")); // blank line
                    lines.push(
                        self.instruction_bar_line(&[("Enter", "submit"), ("Esc", "go back")]),
                    );
                } else {
                    lines.push(Line::from("")); // blank line
                    lines.push(Line::from(Span::styled(
                        format!(" Plan file: {}", plan_file_path),
                        Style::default().fg(text_muted()),
                    )));
                    lines.push(Line::from("")); // blank line
                    lines.push(self.instruction_bar_line(&[
                        ("Enter", "select"),
                        ("↑/↓", "navigate"),
                        ("Esc", "cancel"),
                    ]));
                }
            }
        }

        lines
    }

    /// Build a horizontal separator line
    fn separator_line(&self, width: usize) -> Line<'static> {
        Line::from(Span::styled(
            HORIZONTAL_LINE.to_string().repeat(width),
            Style::default().fg(text_faint()),
        ))
    }

    /// Build a dashed line (for plan box)
    fn dashed_line(&self, width: usize) -> Line<'static> {
        Line::from(Span::styled(
            DASHED_LINE.to_string().repeat(width),
            Style::default().fg(text_faint()),
        ))
    }

    /// Build the tab bar line for multi-question prompts
    fn tab_bar_line(&self, questions: &[UserQuestion]) -> Line<'static> {
        let mut spans = vec![Span::styled(
            format!("{} ", LEFT_ARROW),
            Style::default().fg(text_faint()),
        )];

        for (i, q) in questions.iter().enumerate() {
            let is_current = i == self.current_question_idx;
            let is_answered = self.question_answered(i);

            let indicator = if is_answered { CHECKED } else { UNCHECKED };
            let style = if is_current {
                Style::default()
                    .fg(accent_primary())
                    .add_modifier(Modifier::BOLD)
            } else if is_answered {
                Style::default().fg(accent_secondary())
            } else {
                Style::default().fg(text_muted())
            };

            spans.push(Span::styled(format!("{} {} ", indicator, q.header), style));

            if i < questions.len() - 1 {
                spans.push(Span::styled(" ", Style::default()));
            }
        }

        if self.has_submit_tab() {
            let all_answered = self.all_questions_answered();
            let is_current = self.is_submit_tab();
            let indicator = if all_answered { CHECKED } else { UNCHECKED };
            let submit_style = if is_current {
                Style::default()
                    .fg(accent_primary())
                    .add_modifier(Modifier::BOLD)
            } else if all_answered {
                Style::default().fg(accent_secondary())
            } else {
                Style::default().fg(text_faint())
            };
            spans.push(Span::styled(
                format!(" {} Submit ", indicator),
                submit_style,
            ));
        }

        spans.push(Span::styled(
            format!(" {}", RIGHT_ARROW),
            Style::default().fg(text_faint()),
        ));

        Line::from(spans)
    }

    /// Append submit view lines (summary of answers)
    fn append_submit_view_lines(&self, lines: &mut Vec<Line<'static>>, questions: &[UserQuestion]) {
        lines.push(Line::from(Span::styled(
            " Ready to submit your selections?",
            Style::default().fg(text_primary()),
        )));
        lines.push(Line::from("")); // blank line

        for (i, question) in questions.iter().enumerate() {
            let answer_text = match self.answers.get(i).and_then(|a| a.as_ref()) {
                Some(QuestionAnswer::Custom(text)) => text.clone(),
                Some(QuestionAnswer::Selected(idx)) => question
                    .options
                    .get(*idx)
                    .map(|o| o.label.clone())
                    .unwrap_or_default(),
                Some(QuestionAnswer::MultiSelected(indices)) => {
                    let mut labels = Vec::new();
                    for (opt_idx, opt) in question.options.iter().enumerate() {
                        if indices.contains(&opt_idx) {
                            labels.push(opt.label.clone());
                        }
                    }
                    labels.join(", ")
                }
                None => "(none)".to_string(),
            };

            lines.push(Line::from(Span::styled(
                format!(" {}: {}", question.header, answer_text),
                Style::default().fg(text_secondary()),
            )));
        }

        lines.push(Line::from("")); // blank line
        lines.push(self.instruction_bar_line(&[
            ("Enter", "submit"),
            ("Tab/←/→", "questions"),
            ("Esc", "cancel"),
        ]));
    }

    /// Append question content lines
    fn append_question_lines(
        &self,
        lines: &mut Vec<Line<'static>>,
        question: &UserQuestion,
        is_multi_question: bool,
    ) {
        // Question text
        lines.push(Line::from(Span::styled(
            question.question.clone(),
            Style::default().fg(text_primary()),
        )));
        lines.push(Line::from("")); // blank line

        if self.input_mode {
            // Text input mode
            lines.push(self.text_input_line());
            lines.push(Line::from("")); // blank line
            lines.push(self.instruction_bar_line(&[("Enter", "submit"), ("Esc", "go back")]));
        } else {
            let show_checkbox = question.multi_select;
            let answer = self
                .answers
                .get(self.current_question_idx)
                .and_then(|a| a.as_ref());

            // Options
            for (i, opt) in question.options.iter().enumerate() {
                let is_selected = self.current_option == i;
                let is_checked = match answer {
                    Some(QuestionAnswer::Selected(idx)) => *idx == i,
                    Some(QuestionAnswer::MultiSelected(indices)) => indices.contains(&i),
                    _ => false,
                };
                lines.push(self.option_line(i, &opt.label, is_selected, show_checkbox, is_checked));
                lines.push(Line::from(vec![
                    Span::raw("     "), // Indent to align with label
                    Span::styled(opt.description.clone(), Style::default().fg(text_muted())),
                ]));
            }

            // "Type something" option
            let type_idx = question.options.len();
            let is_selected = self.current_option == type_idx;
            let is_checked = matches!(answer, Some(QuestionAnswer::Custom(_)));
            lines.push(self.type_something_line(type_idx, is_selected, show_checkbox, is_checked));
            lines.push(Line::from("")); // blank line after "Type something"

            // Instructions
            let instructions = if question.multi_select {
                vec![
                    ("Enter/Space", "toggle"),
                    ("Tab/←/→", "questions"),
                    ("↑/↓", "navigate"),
                    ("Esc", "cancel"),
                ]
            } else if is_multi_question {
                vec![
                    ("Enter", "select"),
                    ("Tab/←/→", "questions"),
                    ("↑/↓", "navigate"),
                    ("Esc", "cancel"),
                ]
            } else {
                vec![("Enter", "select"), ("↑/↓", "navigate"), ("Esc", "cancel")]
            };
            lines.push(self.instruction_bar_line(&instructions));
        }
    }

    /// Build an option line
    fn option_line(
        &self,
        index: usize,
        label: &str,
        is_selected: bool,
        show_checkbox: bool,
        is_checked: bool,
    ) -> Line<'static> {
        let selector = if is_selected { SELECTOR } else { " " };
        let selector_style = Style::default().fg(accent_primary());
        let number_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_secondary())
        };
        let label_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_secondary())
        };

        let label_text = if show_checkbox {
            format!("[{}] {}", if is_checked { "x" } else { " " }, label)
        } else {
            label.to_string()
        };

        Line::from(vec![
            Span::styled(format!("{} ", selector), selector_style),
            Span::styled(format!("{}. ", index + 1), number_style),
            Span::styled(label_text, label_style),
        ])
    }

    /// Build the "Type something" option line
    fn type_something_line(
        &self,
        index: usize,
        is_selected: bool,
        show_checkbox: bool,
        is_checked: bool,
    ) -> Line<'static> {
        let selector = if is_selected { SELECTOR } else { " " };
        let selector_style = Style::default().fg(accent_primary());
        let number_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_secondary())
        };
        let label_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_muted())
        };

        let label = if show_checkbox {
            format!("[{}] Type something.", if is_checked { "x" } else { " " })
        } else {
            "Type something.".to_string()
        };

        Line::from(vec![
            Span::styled(format!("{} ", selector), selector_style),
            Span::styled(format!("{}. ", index + 1), number_style),
            Span::styled(label, label_style),
        ])
    }

    /// Build the text input line with cursor
    fn text_input_line(&self) -> Line<'static> {
        let prompt_style = Style::default().fg(accent_primary());
        let input_style = Style::default().fg(text_primary());
        let cursor_style = Style::default().add_modifier(Modifier::REVERSED);

        let input = &self.text_input.input;
        let cursor_pos = self.text_input.cursor;

        let mut spans = vec![Span::styled("> ", prompt_style)];

        if input.is_empty() {
            // Just show cursor at start
            spans.push(Span::styled(" ", cursor_style));
        } else if cursor_pos >= input.len() {
            // Cursor at end
            spans.push(Span::styled(input.clone(), input_style));
            spans.push(Span::styled(" ", cursor_style));
        } else {
            // Cursor in middle
            let (before, after) = input.split_at(cursor_pos);
            let (cursor_char, rest) = after.split_at(1);
            spans.push(Span::styled(before.to_string(), input_style));
            spans.push(Span::styled(cursor_char.to_string(), cursor_style));
            spans.push(Span::styled(rest.to_string(), input_style));
        }

        Line::from(spans)
    }

    /// Build the instruction bar line
    fn instruction_bar_line(&self, instructions: &[(&str, &str)]) -> Line<'static> {
        let mut spans = Vec::new();
        let key_style = Style::default().fg(accent_primary());
        let desc_style = Style::default().fg(text_muted());
        let sep_style = Style::default().fg(text_faint());

        for (i, (key, desc)) in instructions.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" │ ", sep_style));
            }
            spans.push(Span::styled(key.to_string(), key_style));
            spans.push(Span::styled(format!(" {}", desc), desc_style));
        }

        Line::from(spans)
    }

    /// Build an option line for ExitPlanMode
    fn exit_plan_option_line(&self, index: usize, label: &str, is_selected: bool) -> Line<'static> {
        let selector = if is_selected { SELECTOR } else { " " };
        let selector_style = Style::default().fg(accent_primary());
        let number_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_secondary())
        };
        let label_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_secondary())
        };

        Line::from(vec![
            Span::styled(format!("{} ", selector), selector_style),
            Span::styled(format!("{}. ", index + 1), number_style),
            Span::styled(label.to_string(), label_style),
        ])
    }
}

// ============================================================================
// Widget Implementation
// ============================================================================

/// Widget for rendering the inline prompt
pub struct InlinePrompt<'a> {
    state: &'a InlinePromptState,
}

impl<'a> InlinePrompt<'a> {
    pub fn new(state: &'a InlinePromptState) -> Self {
        Self { state }
    }

    /// Render the horizontal separator line
    fn render_separator(&self, area: Rect, buf: &mut Buffer) {
        let line_style = Style::default().fg(text_faint());
        for x in area.x..area.x + area.width {
            buf[(x, area.y)]
                .set_char(HORIZONTAL_LINE)
                .set_style(line_style);
        }
    }

    /// Render the dashed line (for plan box)
    fn render_dashed_line(&self, area: Rect, buf: &mut Buffer) {
        let line_style = Style::default().fg(text_faint());
        for x in area.x..area.x + area.width {
            buf[(x, area.y)].set_char(DASHED_LINE).set_style(line_style);
        }
    }

    /// Render the question tab bar for multi-question prompts
    fn render_tab_bar(
        &self,
        area: Rect,
        buf: &mut Buffer,
        questions: &[UserQuestion],
        show_submit: bool,
    ) {
        let mut spans = vec![Span::styled(
            format!("{} ", LEFT_ARROW),
            Style::default().fg(text_faint()),
        )];

        for (i, q) in questions.iter().enumerate() {
            let is_current = i == self.state.current_question_idx;
            let is_answered = self.state.question_answered(i);

            let indicator = if is_answered { CHECKED } else { UNCHECKED };
            let style = if is_current {
                Style::default()
                    .fg(accent_primary())
                    .add_modifier(Modifier::BOLD)
            } else if is_answered {
                Style::default().fg(accent_secondary())
            } else {
                Style::default().fg(text_muted())
            };

            spans.push(Span::styled(format!("{} {} ", indicator, q.header), style));

            if i < questions.len() - 1 {
                spans.push(Span::styled(" ", Style::default()));
            }
        }

        if show_submit {
            let all_answered = self.state.all_questions_answered();
            let is_current = self.state.is_submit_tab();
            let indicator = if all_answered { CHECKED } else { UNCHECKED };
            let submit_style = if is_current {
                Style::default()
                    .fg(accent_primary())
                    .add_modifier(Modifier::BOLD)
            } else if all_answered {
                Style::default().fg(accent_secondary())
            } else {
                Style::default().fg(text_faint())
            };
            spans.push(Span::styled(
                format!(" {} Submit ", indicator),
                submit_style,
            ));
        }

        spans.push(Span::styled(
            format!(" {}", RIGHT_ARROW),
            Style::default().fg(text_faint()),
        ));

        let line = Line::from(spans);
        Paragraph::new(line).render(area, buf);
    }

    /// Render a single option
    #[allow(clippy::too_many_arguments)]
    fn render_option(
        &self,
        area: Rect,
        buf: &mut Buffer,
        index: usize,
        option: &QuestionOption,
        is_selected: bool,
        show_checkbox: bool,
        is_checked: bool,
    ) {
        // First line: number and label
        let selector = if is_selected { SELECTOR } else { " " };
        let selector_style = Style::default().fg(accent_primary());
        let number_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_secondary())
        };
        let label_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_secondary())
        };

        let label = if show_checkbox {
            format!("[{}] {}", if is_checked { "x" } else { " " }, option.label)
        } else {
            option.label.clone()
        };
        let label_line = Line::from(vec![
            Span::styled(format!("{} ", selector), selector_style),
            Span::styled(format!("{}. ", index + 1), number_style),
            Span::styled(label, label_style),
        ]);
        Paragraph::new(label_line).render(area, buf);

        // Second line: description (indented)
        if area.height > 1 {
            let desc_area = Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: 1,
            };
            let desc_style = Style::default().fg(text_muted());
            let desc_line = Line::from(vec![
                Span::raw("     "), // Indent to align with label
                Span::styled(&option.description, desc_style),
            ]);
            Paragraph::new(desc_line).render(desc_area, buf);
        }
    }

    /// Render the "Type something" option
    fn render_type_something(
        &self,
        area: Rect,
        buf: &mut Buffer,
        index: usize,
        is_selected: bool,
        show_checkbox: bool,
        is_checked: bool,
    ) {
        let selector = if is_selected { SELECTOR } else { " " };
        let selector_style = Style::default().fg(accent_primary());
        let number_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_secondary())
        };
        let label_style = if is_selected {
            Style::default().fg(text_primary())
        } else {
            Style::default().fg(text_muted())
        };

        let label = if show_checkbox {
            format!("[{}] Type something.", if is_checked { "x" } else { " " })
        } else {
            "Type something.".to_string()
        };
        let line = Line::from(vec![
            Span::styled(format!("{} ", selector), selector_style),
            Span::styled(format!("{}. ", index + 1), number_style),
            Span::styled(label, label_style),
        ]);
        Paragraph::new(line).render(area, buf);
    }

    /// Render the text input mode
    fn render_input(&self, area: Rect, buf: &mut Buffer) {
        // Render prompt and input
        let prompt_style = Style::default().fg(accent_primary());
        let input_style = Style::default().fg(text_primary());

        // Draw prompt
        buf[(area.x, area.y)].set_char('>').set_style(prompt_style);
        buf[(area.x + 1, area.y)]
            .set_char(' ')
            .set_style(prompt_style);

        // Draw input text with cursor
        let input_area = Rect {
            x: area.x + 2,
            y: area.y,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        self.state.text_input.render(input_area, buf, input_style);
    }
}

impl Widget for InlinePrompt<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;

        match &self.state.prompt_type {
            InlinePromptType::AskUserQuestion { questions } => {
                // Separator line
                self.render_separator(
                    Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    },
                    buf,
                );
                y += 1;

                // Tab bar for multi-question or submit mode
                let show_tab_bar = questions.len() > 1 || self.state.has_submit_tab();
                if show_tab_bar {
                    self.render_tab_bar(
                        Rect {
                            x: area.x,
                            y,
                            width: area.width,
                            height: 1,
                        },
                        buf,
                        questions,
                        self.state.has_submit_tab(),
                    );
                    y += 2; // Tab bar + blank line
                } else {
                    y += 1; // Just blank line
                }

                if self.state.is_submit_tab() {
                    // Submit summary view
                    let header_style = Style::default().fg(text_primary());
                    Paragraph::new(" Ready to submit your selections?")
                        .style(header_style)
                        .render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                    y += 2;

                    for (i, question) in questions.iter().enumerate() {
                        let answer_text = match self.state.answers.get(i).and_then(|a| a.as_ref()) {
                            Some(QuestionAnswer::Custom(text)) => text.clone(),
                            Some(QuestionAnswer::Selected(idx)) => question
                                .options
                                .get(*idx)
                                .map(|o| o.label.clone())
                                .unwrap_or_default(),
                            Some(QuestionAnswer::MultiSelected(indices)) => {
                                let mut labels = Vec::new();
                                for (opt_idx, opt) in question.options.iter().enumerate() {
                                    if indices.contains(&opt_idx) {
                                        labels.push(opt.label.clone());
                                    }
                                }
                                labels.join(", ")
                            }
                            None => "(none)".to_string(),
                        };

                        let line = format!(" {}: {}", question.header, answer_text);
                        Paragraph::new(line)
                            .style(Style::default().fg(text_secondary()))
                            .render(
                                Rect {
                                    x: area.x,
                                    y,
                                    width: area.width,
                                    height: 1,
                                },
                                buf,
                            );
                        y += 1;
                    }

                    InstructionBar::new(vec![
                        ("Enter", "submit"),
                        ("Tab/←/→", "questions"),
                        ("Esc", "cancel"),
                    ])
                    .render(
                        Rect {
                            x: area.x,
                            y,
                            width: area.width,
                            height: 1,
                        },
                        buf,
                    );
                } else if let Some(question) = self.state.current_question() {
                    // Question text
                    let q_style = Style::default().fg(text_primary());
                    Paragraph::new(question.question.as_str())
                        .style(q_style)
                        .wrap(Wrap { trim: true })
                        .render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 2,
                            },
                            buf,
                        );
                    y += 2;

                    if self.state.input_mode {
                        // Render text input
                        self.render_input(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                        y += 2;

                        // Input mode instructions
                        InstructionBar::new(vec![("Enter", "submit"), ("Esc", "go back")]).render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                    } else {
                        let show_checkbox = question.multi_select;
                        let answer = self
                            .state
                            .answers
                            .get(self.state.current_question_idx)
                            .and_then(|a| a.as_ref());

                        // Options
                        for (i, opt) in question.options.iter().enumerate() {
                            let is_selected = self.state.current_option == i;
                            let is_checked = match answer {
                                Some(QuestionAnswer::Selected(idx)) => *idx == i,
                                Some(QuestionAnswer::MultiSelected(indices)) => {
                                    indices.contains(&i)
                                }
                                _ => false,
                            };
                            self.render_option(
                                Rect {
                                    x: area.x,
                                    y,
                                    width: area.width,
                                    height: 2,
                                },
                                buf,
                                i,
                                opt,
                                is_selected,
                                show_checkbox,
                                is_checked,
                            );
                            y += 2;
                        }

                        // "Type something" option
                        let type_idx = question.options.len();
                        let is_selected = self.state.current_option == type_idx;
                        let is_checked = matches!(answer, Some(QuestionAnswer::Custom(_)));
                        self.render_type_something(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                            type_idx,
                            is_selected,
                            show_checkbox,
                            is_checked,
                        );
                        y += 2;

                        // Instructions
                        let instructions = if question.multi_select {
                            vec![
                                ("Enter/Space", "toggle"),
                                ("Tab/←/→", "questions"),
                                ("↑/↓", "navigate"),
                                ("Esc", "cancel"),
                            ]
                        } else if questions.len() > 1 {
                            vec![
                                ("Enter", "select"),
                                ("Tab/←/→", "questions"),
                                ("↑/↓", "navigate"),
                                ("Esc", "cancel"),
                            ]
                        } else {
                            vec![("Enter", "select"), ("↑/↓", "navigate"), ("Esc", "cancel")]
                        };
                        InstructionBar::new(instructions).render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                    }
                }
            }

            InlinePromptType::ExitPlanMode {
                plan_content,
                plan_file_path,
            } => {
                // "Here is Claude's plan:" header
                let header_style = Style::default().fg(text_primary());
                Paragraph::new(" Here is Claude's plan:")
                    .style(header_style)
                    .render(
                        Rect {
                            x: area.x,
                            y,
                            width: area.width,
                            height: 1,
                        },
                        buf,
                    );
                y += 1;

                // Top dashed line
                self.render_dashed_line(
                    Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    },
                    buf,
                );
                y += 1;

                // Plan content (limited to available space)
                let max_plan_lines =
                    (area.height.saturating_sub(y - area.y).saturating_sub(8)) as usize;
                let plan_lines: Vec<&str> = plan_content.lines().take(max_plan_lines).collect();
                let plan_style = Style::default().fg(text_secondary());

                for line in &plan_lines {
                    Paragraph::new(format!(" {}", line))
                        .style(plan_style)
                        .render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                    y += 1;
                }

                // Bottom dashed line
                self.render_dashed_line(
                    Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: 1,
                    },
                    buf,
                );
                y += 2; // Line + blank

                if self.state.input_mode {
                    // "Would you like to proceed?" header
                    let prompt_style = Style::default().fg(text_primary());
                    Paragraph::new(" Would you like to proceed?")
                        .style(prompt_style)
                        .render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                    y += 2;

                    // Text input
                    self.render_input(
                        Rect {
                            x: area.x + 1,
                            y,
                            width: area.width.saturating_sub(1),
                            height: 1,
                        },
                        buf,
                    );
                    y += 2;

                    // File path
                    let path_style = Style::default().fg(text_faint());
                    Paragraph::new(format!(" {}", plan_file_path))
                        .style(path_style)
                        .render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                    y += 2;

                    // Instructions
                    InstructionBar::new(vec![("Enter", "submit"), ("Esc", "go back")]).render(
                        Rect {
                            x: area.x,
                            y,
                            width: area.width,
                            height: 1,
                        },
                        buf,
                    );
                } else {
                    // "Would you like to proceed?" header
                    let prompt_style = Style::default().fg(text_primary());
                    Paragraph::new(" Would you like to proceed?")
                        .style(prompt_style)
                        .render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                    y += 2;

                    // Option 1: Yes, start building
                    let opt1_selected = self.state.current_option == 0;
                    let selector1 = if opt1_selected { SELECTOR } else { " " };
                    let style1 = if opt1_selected {
                        Style::default().fg(text_primary())
                    } else {
                        Style::default().fg(text_secondary())
                    };
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" {} ", selector1),
                            Style::default().fg(accent_primary()),
                        ),
                        Span::styled("1. Yes, start building", style1),
                    ]))
                    .render(
                        Rect {
                            x: area.x,
                            y,
                            width: area.width,
                            height: 1,
                        },
                        buf,
                    );
                    y += 1;

                    // Option 2: Type here to tell Claude what to change
                    let opt2_selected = self.state.current_option == 1;
                    let selector2 = if opt2_selected { SELECTOR } else { " " };
                    let style2 = if opt2_selected {
                        Style::default().fg(text_primary())
                    } else {
                        Style::default().fg(text_muted())
                    };
                    Paragraph::new(Line::from(vec![
                        Span::styled(
                            format!(" {} ", selector2),
                            Style::default().fg(accent_primary()),
                        ),
                        Span::styled("2. Type here to tell Claude what to change", style2),
                    ]))
                    .render(
                        Rect {
                            x: area.x,
                            y,
                            width: area.width,
                            height: 1,
                        },
                        buf,
                    );
                    y += 2;

                    // File path
                    let path_style = Style::default().fg(text_faint());
                    Paragraph::new(format!(" {}", plan_file_path))
                        .style(path_style)
                        .render(
                            Rect {
                                x: area.x,
                                y,
                                width: area.width,
                                height: 1,
                            },
                            buf,
                        );
                    y += 2;

                    // Instructions
                    InstructionBar::new(vec![
                        ("Enter", "select"),
                        ("↑/↓", "navigate"),
                        ("Esc", "cancel"),
                    ])
                    .render(
                        Rect {
                            x: area.x,
                            y,
                            width: area.width,
                            height: 1,
                        },
                        buf,
                    );
                }
            }
        }
    }
}
