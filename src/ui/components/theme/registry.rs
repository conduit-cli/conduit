//! Theme registry for discovery and loading.
//!
//! Manages built-in themes, discovered VS Code themes, and custom theme paths.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::builtin::{builtin_themes, get_builtin};
use super::types::{Theme, ThemeInfo, ThemeSource};
use super::vscode::VsCodeTheme;

/// Theme registry that manages all available themes.
#[derive(Debug, Default)]
pub struct ThemeRegistry {
    /// Discovered VS Code themes (name -> path to JSON)
    vscode_themes: HashMap<String, VsCodeThemeEntry>,
}

#[derive(Debug, Clone)]
struct VsCodeThemeEntry {
    /// Display name
    display_name: String,
    /// Path to theme JSON file
    path: PathBuf,
    /// Whether this is a light theme
    is_light: bool,
}

/// VS Code extension package.json structure (partial).
#[derive(Debug, Deserialize)]
struct PackageJson {
    #[serde(default)]
    contributes: Option<Contributes>,
}

#[derive(Debug, Deserialize)]
struct Contributes {
    #[serde(default)]
    themes: Option<Vec<ThemeContribution>>,
}

#[derive(Debug, Deserialize)]
struct ThemeContribution {
    label: Option<String>,
    #[serde(rename = "uiTheme")]
    ui_theme: Option<String>,
    path: Option<String>,
}

impl ThemeRegistry {
    /// Create a new theme registry and discover VS Code themes.
    pub fn new() -> Self {
        let mut registry = Self::default();
        registry.discover_vscode_themes();
        registry
    }

    /// Discover VS Code themes from ~/.vscode/extensions/.
    pub fn discover_vscode_themes(&mut self) {
        if let Some(extensions_dir) = Self::vscode_extensions_dir() {
            self.scan_extensions_dir(&extensions_dir);
        }
    }

    /// Get the VS Code extensions directory.
    fn vscode_extensions_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".vscode/extensions"))
    }

    /// Scan the extensions directory for theme extensions.
    fn scan_extensions_dir(&mut self, extensions_dir: &Path) {
        let Ok(entries) = fs::read_dir(extensions_dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.scan_extension(&path);
            }
        }
    }

    /// Scan a single extension directory for themes.
    fn scan_extension(&mut self, extension_dir: &Path) {
        let package_json_path = extension_dir.join("package.json");
        if !package_json_path.exists() {
            return;
        }

        let Ok(content) = fs::read_to_string(&package_json_path) else {
            return;
        };

        let Ok(package): Result<PackageJson, _> = serde_json::from_str(&content) else {
            return;
        };

        let Some(contributes) = package.contributes else {
            return;
        };

        let Some(themes) = contributes.themes else {
            return;
        };

        for theme in themes {
            let Some(label) = theme.label else {
                continue;
            };
            let Some(rel_path) = theme.path else {
                continue;
            };

            // Resolve the theme path relative to the extension directory
            let theme_path = extension_dir.join(&rel_path);
            if !theme_path.exists() {
                continue;
            }

            // Determine if this is a light theme
            let is_light = matches!(theme.ui_theme.as_deref(), Some("vs") | Some("hc-light"));

            // Use a unique key combining extension dir name and label
            let key = format!(
                "{}:{}",
                extension_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown"),
                &label
            );

            self.vscode_themes.insert(
                key,
                VsCodeThemeEntry {
                    display_name: label,
                    path: theme_path,
                    is_light,
                },
            );
        }
    }

    /// List all available themes.
    pub fn list_themes(&self) -> Vec<ThemeInfo> {
        let mut themes = Vec::new();

        // Add built-in themes
        for (name, theme) in builtin_themes() {
            themes.push(ThemeInfo {
                name: name.to_string(),
                display_name: theme.name.clone(),
                source: ThemeSource::Builtin,
                is_light: theme.is_light,
            });
        }

        // Add discovered VS Code themes
        for (key, entry) in &self.vscode_themes {
            themes.push(ThemeInfo {
                name: key.clone(),
                display_name: entry.display_name.clone(),
                source: ThemeSource::VsCodeExtension {
                    path: entry.path.clone(),
                },
                is_light: entry.is_light,
            });
        }

        // Sort by display name
        themes.sort_by(|a, b| a.display_name.cmp(&b.display_name));

        themes
    }

    /// Load a theme by name.
    ///
    /// Tries built-in themes first, then VS Code themes.
    pub fn load_theme(&self, name: &str) -> Option<Theme> {
        // Try built-in first
        if let Some(theme) = get_builtin(name) {
            return Some(theme);
        }

        // Try built-in themes by display name (case-insensitive)
        for (_, theme) in builtin_themes() {
            if theme.name.eq_ignore_ascii_case(name) {
                return Some(theme);
            }
        }

        // Try VS Code theme by key
        if let Some(entry) = self.vscode_themes.get(name) {
            let loaded = self.load_from_path(&entry.path);
            if loaded.is_none() {
                tracing::warn!(
                    theme = name,
                    path = %entry.path.display(),
                    "VS Code theme failed to load by key"
                );
            }
            return loaded;
        }

        // Try VS Code theme by display name (case-insensitive)
        for entry in self.vscode_themes.values() {
            if entry.display_name.eq_ignore_ascii_case(name) {
                let loaded = self.load_from_path(&entry.path);
                if loaded.is_none() {
                    tracing::warn!(
                        theme = name,
                        path = %entry.path.display(),
                        "VS Code theme failed to load by display name"
                    );
                }
                return loaded;
            }
        }

        None
    }

    /// Load a theme from a file path.
    pub fn load_from_path(&self, path: &Path) -> Option<Theme> {
        match VsCodeTheme::load_from_file(path) {
            Ok(vscode) => Some(vscode.to_theme()),
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "Failed to load VS Code theme file"
                );
                None
            }
        }
    }

    /// Get the number of available themes.
    pub fn theme_count(&self) -> usize {
        builtin_themes().len() + self.vscode_themes.len()
    }

    /// Check if a theme exists.
    pub fn has_theme(&self, name: &str) -> bool {
        get_builtin(name).is_some()
            || self.vscode_themes.contains_key(name)
            || self
                .vscode_themes
                .values()
                .any(|e| e.display_name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_themes_available() {
        let registry = ThemeRegistry::default();
        let themes = registry.list_themes();

        // Should have at least the built-in themes
        assert!(themes.iter().any(|t| t.name == "default-dark"));
        assert!(themes.iter().any(|t| t.name == "catppuccin-mocha"));
    }

    #[test]
    fn test_load_builtin_theme() {
        let registry = ThemeRegistry::default();

        let theme = registry.load_theme("catppuccin-mocha");
        assert!(theme.is_some());
        assert_eq!(theme.unwrap().name, "Catppuccin Mocha");
    }
}
