use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;

use crate::agent::{AgentMode, AgentType, MessageDisplay};
use crate::config::{KeyCombo, KeyContext};
use crate::ui::action::Action;
use crate::ui::app::App;
use crate::ui::components::SIDEBAR_HEADER_ROWS;
use crate::ui::effect::Effect;
use crate::ui::events::{InputMode, ViewMode};
use crate::ui::terminal_guard::TerminalGuard;

impl App {
    pub(super) async fn handle_input_event(
        &mut self,
        input: Event,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<Vec<Effect>> {
        match input {
            Event::Key(key) => self.handle_key_event(key, terminal, guard).await,
            Event::Mouse(mouse) => self.handle_mouse_event(mouse, terminal, guard).await,
            Event::Paste(text) => {
                self.handle_paste_input(text);
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    pub(super) async fn handle_key_event(
        &mut self,
        key: KeyEvent,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<Vec<Effect>> {
        // Special handling for modes that bypass normal key processing
        if self.state.input_mode == InputMode::RemovingProject {
            // Ignore all input while removing project
            return Ok(Vec::new());
        }

        // Handle Ctrl+C with double-press detection (global)
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            tracing::debug!("Ctrl+C detected, calling handle_ctrl_c_press");
            let effects = self.handle_ctrl_c_press();
            return Ok(effects);
        }

        // Handle inline prompt input (AskUserQuestion, ExitPlanMode)
        if let Some(session) = self.state.tab_manager.active_session_mut() {
            if let Some(ref mut prompt) = session.inline_prompt {
                use crate::ui::components::{PromptAction, PromptResponse};

                match prompt.handle_key(key) {
                    PromptAction::Submit(response) => {
                        let tool_id = prompt.tool_id.clone();
                        let response_clone = response.clone();
                        let prompt_snapshot = prompt.clone();
                        let pending_request_id = session.pending_tool_permissions.remove(&tool_id);
                        let agent_type = session.agent_type;

                        // Clear the inline prompt
                        session.inline_prompt = None;

                        // Handle the response - format as natural language for the model
                        let effects = if let (AgentType::Claude, true, Some(request_id)) = (
                            agent_type,
                            session.agent_input_tx.is_some(),
                            pending_request_id.as_ref(),
                        ) {
                            match response_clone {
                                PromptResponse::AskUserAnswers { answers } => {
                                    let updated_input = Self::build_ask_user_updated_input(
                                        &prompt_snapshot,
                                        &answers,
                                    );
                                    let response_payload = Self::build_permission_allow_response(
                                        updated_input,
                                        Some(&tool_id),
                                    );
                                    self.send_control_response(request_id, response_payload)
                                }
                                PromptResponse::ExitPlanApprove => {
                                    // Switch to Build mode
                                    session.agent_mode = AgentMode::Build;
                                    session.update_status();
                                    let updated_input =
                                        Self::build_exit_plan_updated_input(&prompt_snapshot);
                                    let response_payload = Self::build_permission_allow_response(
                                        updated_input,
                                        Some(&tool_id),
                                    );
                                    self.send_control_response(request_id, response_payload)
                                }
                                PromptResponse::ExitPlanFeedback(feedback) => {
                                    let response_payload = Self::build_permission_deny_response(
                                        format!("User feedback on plan: {}", feedback),
                                        Some(&tool_id),
                                    );
                                    self.send_control_response(request_id, response_payload)
                                }
                            }
                        } else if agent_type == AgentType::Claude
                            && session.agent_input_tx.is_some()
                        {
                            let response_payload = match response_clone {
                                PromptResponse::AskUserAnswers { answers } => {
                                    let updated_input = Self::build_ask_user_updated_input(
                                        &prompt_snapshot,
                                        &answers,
                                    );
                                    Self::build_permission_allow_response(
                                        updated_input,
                                        Some(&tool_id),
                                    )
                                }
                                PromptResponse::ExitPlanApprove => {
                                    // Switch to Build mode
                                    session.agent_mode = AgentMode::Build;
                                    session.update_status();
                                    let updated_input =
                                        Self::build_exit_plan_updated_input(&prompt_snapshot);
                                    Self::build_permission_allow_response(
                                        updated_input,
                                        Some(&tool_id),
                                    )
                                }
                                PromptResponse::ExitPlanFeedback(feedback) => {
                                    Self::build_permission_deny_response(
                                        format!("User feedback on plan: {}", feedback),
                                        Some(&tool_id),
                                    )
                                }
                            };
                            session
                                .pending_tool_permission_responses
                                .insert(tool_id.clone(), response_payload);
                            Vec::new()
                        } else {
                            match response_clone {
                                PromptResponse::AskUserAnswers { answers } => {
                                    let (content, tool_use_result) =
                                        Self::build_ask_user_tool_result(
                                            &prompt_snapshot,
                                            &answers,
                                        );
                                    self.send_tool_result(&tool_id, content, tool_use_result)
                                }
                                PromptResponse::ExitPlanApprove => {
                                    // Switch to Build mode
                                    session.agent_mode = AgentMode::Build;
                                    session.update_status();
                                    let (content, tool_use_result) =
                                        Self::build_exit_plan_tool_result(
                                            &prompt_snapshot,
                                            true,
                                            None,
                                        );
                                    self.send_tool_result(&tool_id, content, tool_use_result)
                                }
                                PromptResponse::ExitPlanFeedback(feedback) => {
                                    let (content, tool_use_result) =
                                        Self::build_exit_plan_tool_result(
                                            &prompt_snapshot,
                                            false,
                                            Some(feedback),
                                        );
                                    self.send_tool_result(&tool_id, content, tool_use_result)
                                }
                            }
                        };
                        return Ok(effects);
                    }
                    PromptAction::Cancel => {
                        let tool_id = prompt.tool_id.clone();
                        let pending_request_id = session.pending_tool_permissions.remove(&tool_id);
                        let agent_type = session.agent_type;
                        session.inline_prompt = None;
                        // Send cancellation as clear message
                        let effects = if let (AgentType::Claude, true, Some(request_id)) = (
                            agent_type,
                            session.agent_input_tx.is_some(),
                            pending_request_id.as_ref(),
                        ) {
                            let response_payload = Self::build_permission_deny_response(
                                "User cancelled the prompt.".to_string(),
                                Some(&tool_id),
                            );
                            self.send_control_response(request_id, response_payload)
                        } else if agent_type == AgentType::Claude
                            && session.agent_input_tx.is_some()
                        {
                            let response_payload = Self::build_permission_deny_response(
                                "User cancelled the prompt.".to_string(),
                                Some(&tool_id),
                            );
                            session
                                .pending_tool_permission_responses
                                .insert(tool_id.clone(), response_payload);
                            Vec::new()
                        } else {
                            self.send_tool_result(
                                &tool_id,
                                "User cancelled the prompt.".to_string(),
                                None,
                            )
                        };
                        return Ok(effects);
                    }
                    PromptAction::Consumed => {
                        // Key was handled but no action yet
                        return Ok(Vec::new());
                    }
                    PromptAction::NotHandled => {
                        // Fall through to normal handling
                    }
                }
            }
        }

        // Esc exits shell mode back to normal input
        if key.code == KeyCode::Esc
            && !self.has_active_dialog()
            && matches!(
                self.state.input_mode,
                InputMode::Normal | InputMode::Scrolling
            )
        {
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                if session.input_box.is_shell_mode() {
                    session.input_box.set_shell_mode(false);
                    session.update_status();
                    self.state.last_esc_press = None;
                    return Ok(Vec::new());
                }
            }
        }

        // Handle Esc with double-press detection (only when no dialog active and in normal mode)
        if key.code == KeyCode::Esc
            && !self.has_active_dialog()
            && !self.state.show_first_time_splash
            && matches!(
                self.state.input_mode,
                InputMode::Normal | InputMode::Scrolling
            )
        {
            self.handle_esc_press();
            return Ok(Vec::new());
        }

        // First-time splash screen handling (only when no dialogs are visible)
        if self.state.show_first_time_splash
            && !self.state.command_palette_state.is_visible()
            && !self.state.base_dir_dialog_state.is_visible()
            && !self.state.project_picker_state.is_visible()
            && !self.state.add_repo_dialog_state.is_visible()
            && self.state.input_mode != InputMode::SelectingAgent
            && self.state.input_mode != InputMode::ShowingError
        {
            // Handle Ctrl+P to open command palette
            let is_ctrl_p = (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P')))
                || matches!(key.code, KeyCode::Char('\x10')); // ASCII 16 = Ctrl+P
            if is_ctrl_p {
                return self
                    .execute_action(Action::OpenCommandPalette, terminal, guard)
                    .await;
            }
            // Handle Ctrl+N to add new project
            let is_ctrl_n = (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N')))
                || matches!(key.code, KeyCode::Char('\x0e'));
            if is_ctrl_n || (key.modifiers.is_empty() && key.code == KeyCode::Enter) {
                return self
                    .execute_action(Action::NewProject, terminal, guard)
                    .await;
            }
            if key.modifiers.is_empty() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        return self.execute_action(Action::Quit, terminal, guard).await;
                    }
                    _ => {}
                }
            }
        }

        // Handle Ctrl+N and Ctrl+P when tabs are empty (works from any input mode)
        if self.state.tab_manager.is_empty() && !self.state.command_palette_state.is_visible() {
            let is_ctrl_n = (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N')))
                || matches!(key.code, KeyCode::Char('\x0e')); // ASCII 14 = Ctrl+N

            if is_ctrl_n {
                return self
                    .execute_action(Action::NewProject, terminal, guard)
                    .await;
            }

            // Handle Ctrl+P for command palette
            let is_ctrl_p = (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P')))
                || matches!(key.code, KeyCode::Char('\x10')); // ASCII 16 = Ctrl+P

            if is_ctrl_p {
                return self
                    .execute_action(Action::OpenCommandPalette, terminal, guard)
                    .await;
            }
        }

        // Image paste: Ctrl+V (Linux/Windows) or Alt+V (macOS terminals report Cmd as Alt)
        // Match either modifier independently (Cmd often maps to Alt in terminal emulators)
        if self.state.input_mode == InputMode::Normal
            && (key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT))
            && matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&'v'))
        {
            if let Some(session) = self.state.tab_manager.active_session_mut() {
                match crate::ui::clipboard_paste::paste_image_to_temp_png() {
                    Ok((path, info)) => {
                        session
                            .input_box
                            .attach_image(path, info.width, info.height);
                    }
                    Err(err) => {
                        let display = MessageDisplay::Error {
                            content: format!("Failed to paste image: {err}"),
                        };
                        session.chat_view.push(display.to_chat_message());
                    }
                }
            }
            return Ok(Vec::new());
        }

        // Global command mode trigger - ':' from most modes enters command mode
        // Only trigger when input box is empty (so pasting "hello:world" doesn't activate command mode)
        // Also skip when inline prompt is active (user should respond to prompt first)
        let active_session = self.state.tab_manager.active_session();
        let has_active_session = active_session.is_some();
        let has_inline_prompt = active_session.is_some_and(|s| s.inline_prompt.is_some());

        // Only enter command mode if the input box is empty and not in shell mode
        let (input_is_empty, shell_mode) = active_session
            .map(|s| (s.input_box.input().is_empty(), s.input_box.is_shell_mode()))
            .unwrap_or((true, false));

        if Self::should_trigger_command_mode(
            key.code,
            key.modifiers,
            self.state.input_mode,
            input_is_empty,
            shell_mode,
            has_inline_prompt,
        ) {
            self.state.command_buffer.clear();
            self.state.input_mode = InputMode::Command;
            return Ok(Vec::new());
        }

        if Self::should_trigger_slash_menu(
            key.code,
            key.modifiers,
            self.state.input_mode,
            input_is_empty,
            shell_mode,
            has_inline_prompt,
            has_active_session,
        ) {
            self.state.close_overlays();
            self.state.slash_menu_state.show();
            self.state.input_mode = InputMode::SlashMenu;
            return Ok(Vec::new());
        }

        // Get the current context from input mode and view mode
        let context = KeyContext::from_input_mode(self.state.input_mode, self.state.view_mode);

        // Text input (typing characters) handled specially
        if self.should_handle_as_text_input(&key, context) {
            self.handle_text_input(key);
            return Ok(Vec::new());
        }

        // Convert key event to KeyCombo for lookup
        let key_combo = KeyCombo::from_key_event(&key);

        // Look up action in config (context-specific first, then global)
        if let Some(action) = self.config().keybindings.get_action(&key_combo, context) {
            return self.execute_action(action.clone(), terminal, guard).await;
        }

        Ok(Vec::new())
    }

    /// Check if a key event should be handled as text input
    /// Returns true if the key is a printable character without Control/Alt modifiers
    /// and we're in a text-input context
    pub(super) fn should_handle_as_text_input(&self, key: &KeyEvent, context: KeyContext) -> bool {
        // Only handle plain characters (no Ctrl or Alt)
        let has_modifier = key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT);

        if has_modifier {
            return false;
        }

        // Check if this is a character key
        let is_char = matches!(key.code, KeyCode::Char(_));

        if !is_char {
            return false;
        }

        // Only treat as text input in appropriate contexts
        matches!(
            context,
            KeyContext::Chat
                | KeyContext::AddRepository
                | KeyContext::BaseDir
                | KeyContext::ProjectPicker
                | KeyContext::Command
                | KeyContext::HelpDialog
                | KeyContext::SessionImport
                | KeyContext::CommandPalette
                | KeyContext::ThemePicker
                | KeyContext::ModelSelector
        )
    }

    /// Handle text input for text-input contexts
    pub(super) fn handle_text_input(&mut self, key: KeyEvent) {
        let KeyCode::Char(c) = key.code else {
            return;
        };

        match self.state.input_mode {
            InputMode::Normal => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    // Note: ':' is handled globally in handle_key_event
                    // Trigger shell mode with leading '!'
                    if c == '!'
                        && session.input_box.input().is_empty()
                        && !session.input_box.is_shell_mode()
                    {
                        session.input_box.set_shell_mode(true);
                        session.update_status();
                        return;
                    }

                    // Check for help trigger (? on empty input)
                    if c == '?'
                        && session.input_box.input().is_empty()
                        && !session.input_box.is_shell_mode()
                    {
                        self.state.close_overlays();
                        let keybindings = self.config().keybindings.clone();
                        self.state.help_dialog_state.show(&keybindings);
                        self.state.input_mode = InputMode::ShowingHelp;
                        return;
                    }

                    session.input_box.insert_char(c);
                }
            }
            InputMode::Command => {
                self.state.command_buffer.push(c);
            }
            InputMode::ShowingHelp => {
                self.state.help_dialog_state.insert_char(c);
            }
            InputMode::AddingRepository => {
                self.state.add_repo_dialog_state.insert_char(c);
            }
            InputMode::SettingBaseDir => {
                self.state.base_dir_dialog_state.insert_char(c);
            }
            InputMode::PickingProject => {
                self.state.project_picker_state.insert_char(c);
            }
            InputMode::ImportingSession => {
                self.state.session_import_state.insert_char(c);
            }
            InputMode::CommandPalette => {
                self.state.command_palette_state.insert_char(c);
            }
            InputMode::SlashMenu => {
                self.state.slash_menu_state.insert_char(c);
            }
            InputMode::MissingTool => {
                self.state.missing_tool_dialog_state.insert_char(c);
            }
            InputMode::SelectingTheme => {
                self.state.theme_picker_state.insert_char(c);
            }
            InputMode::SelectingModel => {
                self.state.model_selector_state.insert_char(c);
            }
            _ => {}
        }
    }

    pub(super) fn handle_paste_input(&mut self, pasted: String) {
        // Normalize line endings: CRLF → LF, then lone CR → LF
        let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
        match self.state.input_mode {
            InputMode::Normal => {
                if let Some(session) = self.state.tab_manager.active_session_mut() {
                    let mut sanitized = pasted;
                    if session.input_box.input().is_empty()
                        && !session.input_box.is_shell_mode()
                        && sanitized.starts_with('!')
                    {
                        session.input_box.set_shell_mode(true);
                        session.update_status();
                        if let Some(stripped) = sanitized.strip_prefix('!') {
                            sanitized = stripped.to_string();
                        }
                        if sanitized.is_empty() {
                            return;
                        }
                    }
                    session.input_box.handle_paste(sanitized);
                }
            }
            InputMode::Command => {
                let sanitized = pasted.replace('\n', " ");
                self.state.command_buffer.push_str(&sanitized);
            }
            InputMode::ShowingHelp => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.help_dialog_state.insert_char(ch);
                }
            }
            InputMode::AddingRepository => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.add_repo_dialog_state.insert_char(ch);
                }
            }
            InputMode::SettingBaseDir => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.base_dir_dialog_state.insert_char(ch);
                }
            }
            InputMode::PickingProject => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.project_picker_state.insert_char(ch);
                }
            }
            InputMode::ImportingSession => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.session_import_state.insert_char(ch);
                }
            }
            InputMode::CommandPalette => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.command_palette_state.insert_char(ch);
                }
            }
            InputMode::SlashMenu => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.slash_menu_state.insert_char(ch);
                }
            }
            InputMode::MissingTool => {
                let sanitized = pasted.replace('\n', " ");
                for ch in sanitized.chars() {
                    self.state.missing_tool_dialog_state.insert_char(ch);
                }
            }
            InputMode::SelectingTheme => {
                let sanitized = pasted.replace('\n', " ");
                self.state.theme_picker_state.insert_str(&sanitized);
            }
            InputMode::SelectingModel => {
                let sanitized = pasted.replace('\n', " ");
                self.state.model_selector_state.insert_str(&sanitized);
            }
            _ => {}
        }
    }

    pub(super) async fn handle_mouse_event(
        &mut self,
        mouse: MouseEvent,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        guard: &mut TerminalGuard,
    ) -> anyhow::Result<Vec<Effect>> {
        let x = mouse.column;
        let y = mouse.row;

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                // Route scroll to appropriate component based on mode
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.scroll_up(3);
                } else if self.state.input_mode == InputMode::PickingProject
                    && self.state.project_picker_state.is_visible()
                {
                    self.state.project_picker_state.select_prev();
                } else if self.state.input_mode == InputMode::ImportingSession
                    && self.state.session_import_state.is_visible()
                {
                    self.state.session_import_state.select_prev();
                } else if self.state.input_mode == InputMode::SelectingTheme
                    && self.state.theme_picker_state.is_visible()
                {
                    self.state.theme_picker_state.select_prev();
                } else if self.handle_tab_bar_wheel(x, y, true) {
                    return Ok(Vec::new());
                } else if self.state.view_mode == ViewMode::RawEvents {
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if session.raw_events_view.is_detail_visible() {
                            session.raw_events_view.event_detail.scroll_up(3);
                        } else {
                            session.raw_events_view.scroll_up(3);
                        }
                    }
                    self.record_scroll(3);
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.scroll_up(1);
                    self.record_scroll(1);
                }
                Ok(Vec::new())
            }
            MouseEventKind::ScrollDown => {
                // Route scroll to appropriate component based on mode
                if self.state.input_mode == InputMode::ShowingHelp {
                    self.state.help_dialog_state.scroll_down(3);
                } else if self.state.input_mode == InputMode::PickingProject
                    && self.state.project_picker_state.is_visible()
                {
                    self.state.project_picker_state.select_next();
                } else if self.state.input_mode == InputMode::ImportingSession
                    && self.state.session_import_state.is_visible()
                {
                    self.state.session_import_state.select_next();
                } else if self.state.input_mode == InputMode::SelectingTheme
                    && self.state.theme_picker_state.is_visible()
                {
                    self.state.theme_picker_state.select_next();
                } else if self.handle_tab_bar_wheel(x, y, false) {
                    return Ok(Vec::new());
                } else if self.state.view_mode == ViewMode::RawEvents {
                    let list_height = self.raw_events_list_visible_height();
                    let detail_height = self.raw_events_detail_visible_height();
                    if let Some(session) = self.state.tab_manager.active_session_mut() {
                        if session.raw_events_view.is_detail_visible() {
                            let content_height = session.raw_events_view.detail_content_height();
                            session.raw_events_view.event_detail.scroll_down(
                                3,
                                content_height,
                                detail_height,
                            );
                        } else {
                            session.raw_events_view.scroll_down(3, list_height);
                        }
                    }
                    self.record_scroll(3);
                } else if let Some(session) = self.state.tab_manager.active_session_mut() {
                    session.chat_view.scroll_down(1);
                    self.record_scroll(1);
                }
                Ok(Vec::new())
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.handle_scrollbar_press(x, y) {
                    return Ok(Vec::new());
                }
                if self.handle_selection_start(x, y) {
                    return Ok(Vec::new());
                }
                // Handle left clicks based on position
                self.handle_mouse_click(x, y, terminal, guard).await
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.handle_scrollbar_drag(y) {
                    return Ok(Vec::new());
                }
                if self.handle_selection_drag(x, y) {
                    return Ok(Vec::new());
                }
                Ok(Vec::new())
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.state.scroll_drag = None;
                if let Some(mut effects) = self.handle_selection_end() {
                    // If no selection was made (simple click), check for clickable file paths
                    if effects.is_empty() {
                        if let Some(path_effects) = self.handle_file_path_click(x, y) {
                            effects.extend(path_effects);
                        }
                    }
                    return Ok(effects);
                }
                // No selection was active - still check for file path clicks
                // (e.g., clicking in prompt areas where selection isn't started)
                if let Some(path_effects) = self.handle_file_path_click(x, y) {
                    return Ok(path_effects);
                }
                Ok(Vec::new())
            }
            MouseEventKind::Moved => {
                // Update hover state for sidebar workspace name expansion
                if let Some(sidebar_area) = self.state.sidebar_area {
                    // Tree view starts after header (uses centralized constant for consistency)
                    let tree_start_y = sidebar_area.y.saturating_add(SIDEBAR_HEADER_ROWS);
                    // Sidebar has no borders - tree renders directly in content area
                    let inner_x = sidebar_area.x;
                    let inner_width = sidebar_area.width as usize;

                    if Self::point_in_rect(x, y, sidebar_area) && y >= tree_start_y {
                        // Calculate visual row within the tree view
                        let visual_row = (y - tree_start_y) as usize;
                        // Calculate x position within the tree inner area
                        let x_in_tree = x.saturating_sub(inner_x) as usize;
                        let scroll_offset = self.state.sidebar_state.tree_state.offset;

                        // Check if hovering over a workspace name (not git stats or PR)
                        if let Some(workspace_id) = self.state.sidebar_data.workspace_at_name_line(
                            visual_row,
                            x_in_tree,
                            scroll_offset,
                            inner_width,
                        ) {
                            self.state.sidebar_state.tree_state.set_hover(workspace_id);
                        } else {
                            self.state.sidebar_state.tree_state.clear_hover();
                        }
                    } else {
                        // Mouse left sidebar, clear hover
                        self.state.sidebar_state.tree_state.clear_hover();
                    }
                }

                // Update hover state for raw events session ID
                if self.state.view_mode == ViewMode::RawEvents {
                    if let Some(raw_events_area) = self.state.raw_events_area {
                        if let Some(session) = self.state.tab_manager.active_session_mut() {
                            let hover_changed = session.raw_events_view.update_session_id_hover(
                                x,
                                y,
                                raw_events_area,
                            );

                            if hover_changed {
                                // Check if now hovering (need to re-check since we mutated)
                                let is_hovered = session.raw_events_view.is_session_id_hovered();
                                if is_hovered {
                                    self.state.set_footer_message(Some(
                                        "Click session ID to copy".to_string(),
                                    ));
                                } else {
                                    // Clear the hint message when no longer hovering
                                    self.state.set_footer_message(None);
                                }
                            }
                        }
                    }
                }

                // Update hover state for clickable file paths in chat view
                if self.state.view_mode == ViewMode::Chat {
                    if let Some(chat_area) = self.state.chat_area {
                        let show_chat_scrollbar = self.config().ui.show_chat_scrollbar;
                        if let Some(session) = self.state.tab_manager.active_session_mut() {
                            let hover_changed = session.chat_view.update_file_path_hover(
                                x,
                                y,
                                chat_area,
                                show_chat_scrollbar,
                            );

                            if hover_changed {
                                let is_hovered = session.chat_view.is_file_path_hovered();
                                if is_hovered {
                                    self.state.set_footer_message(Some(
                                        "Click to open file in new tab".to_string(),
                                    ));
                                } else {
                                    // Clear the hint message when no longer hovering
                                    self.state.set_footer_message(None);
                                }
                            }
                        }
                    }
                }
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Handle click on a file path in the chat area
    /// Returns Some(effects) if a file path was clicked and opened
    fn handle_file_path_click(&mut self, x: u16, y: u16) -> Option<Vec<Effect>> {
        // Only handle in chat view mode
        if self.state.view_mode != ViewMode::Chat {
            return None;
        }

        // Get the chat area
        let chat_area = self.state.chat_area?;

        // Check if click is in chat area
        if !Self::point_in_rect(x, y, chat_area) {
            return None;
        }

        // Get the active session and check for file path at click position
        let show_chat_scrollbar = self.config().ui.show_chat_scrollbar;
        let session = self.state.tab_manager.active_session_mut()?;
        let path = session
            .chat_view
            .file_path_at_position(x, y, chat_area, show_chat_scrollbar)?;

        // Try to open the file in a new tab
        let path_buf = std::path::PathBuf::from(&path);
        match self.state.tab_manager.open_file(path_buf.clone()) {
            Ok(_) => {
                self.state.set_timed_footer_message(
                    format!("Opened: {}", path),
                    std::time::Duration::from_secs(2),
                );
                Some(vec![Effect::SaveSessionState])
            }
            Err(e) => {
                self.state.set_timed_footer_message(
                    format!("Failed to open {}: {}", path, e),
                    std::time::Duration::from_secs(3),
                );
                None
            }
        }
    }
}
