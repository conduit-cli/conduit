//! Provider selector dialog built on the reusable multi-select dialog.

use crate::agent::AgentType;
use crate::config::Config;
use crate::util::{Tool, ToolAvailability};

use super::{MultiSelectDialog, MultiSelectDialogState, MultiSelectItem};

#[derive(Debug, Clone)]
pub struct ProviderSelectorState {
    pub dialog: MultiSelectDialogState,
}

impl ProviderSelectorState {
    pub fn new() -> Self {
        Self {
            dialog: MultiSelectDialogState::new(10),
        }
    }

    fn all_providers() -> Vec<AgentType> {
        AgentType::preferred_order().to_vec()
    }

    fn provider_tool(provider: AgentType) -> Tool {
        match provider {
            AgentType::Claude => Tool::Claude,
            AgentType::Codex => Tool::Codex,
            AgentType::Gemini => Tool::Gemini,
            AgentType::Opencode => Tool::Opencode,
            AgentType::Pi => Tool::Pi,
        }
    }

    pub fn configure_for(config: &Config, tools: &ToolAvailability) -> Self {
        let configured = config.enabled_providers.clone();
        let items = Self::all_providers()
            .into_iter()
            .map(|provider| {
                let installed = tools.is_available(Self::provider_tool(provider));
                let checked = if !installed {
                    false
                } else {
                    configured
                        .as_ref()
                        .is_none_or(|selected| selected.contains(&provider))
                };

                MultiSelectItem {
                    id: provider.as_str().to_string(),
                    title: provider.display_name().to_string(),
                    description: format!("{} provider", provider.display_name()),
                    checked,
                    disabled: !installed,
                }
            })
            .collect();

        let mut state = Self::new();
        state.dialog.configure(
            "Select Providers",
            Some("Choose which installed providers to include in model selection.".to_string()),
            items,
        );
        state
    }

    pub fn show(&mut self) {
        self.dialog.show();
    }

    pub fn hide(&mut self) {
        self.dialog.hide();
    }

    pub fn is_visible(&self) -> bool {
        self.dialog.is_visible()
    }

    pub fn insert_char(&mut self, c: char) {
        self.dialog.insert_char(c);
    }

    pub fn insert_str(&mut self, s: &str) {
        self.dialog.insert_str(s);
    }

    pub fn delete_char(&mut self) {
        self.dialog.delete_char();
    }

    pub fn delete_forward(&mut self) {
        self.dialog.delete_forward();
    }

    pub fn move_cursor_left(&mut self) {
        self.dialog.move_cursor_left();
    }

    pub fn move_cursor_right(&mut self) {
        self.dialog.move_cursor_right();
    }

    pub fn move_cursor_start(&mut self) {
        self.dialog.move_cursor_start();
    }

    pub fn move_cursor_end(&mut self) {
        self.dialog.move_cursor_end();
    }

    pub fn select_next(&mut self) {
        self.dialog.select_next();
    }

    pub fn select_previous(&mut self) {
        self.dialog.select_previous();
    }

    pub fn select_at_row(&mut self, row: usize) -> bool {
        self.dialog.select_at_row(row)
    }

    pub fn toggle_selected(&mut self) -> bool {
        self.dialog.toggle_selected()
    }

    pub fn selected_provider_ids(&self) -> Vec<String> {
        self.dialog.selected_ids()
    }

    pub fn selected_providers(&self) -> Vec<AgentType> {
        self.selected_provider_ids()
            .into_iter()
            .map(|id| AgentType::parse(&id))
            .collect()
    }

    pub fn validate_non_empty(&mut self) -> bool {
        if self.selected_providers().is_empty() {
            self.dialog.validation_error = Some("Select at least one provider.".to_string());
            return false;
        }
        self.dialog.validation_error = None;
        true
    }
}

impl Default for ProviderSelectorState {
    fn default() -> Self {
        Self::new()
    }
}

pub type ProviderSelector = MultiSelectDialog;
