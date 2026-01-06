//! Workspace name generation using gemstone names

use rand::seq::SliceRandom;

/// Pool of gemstone names for workspace generation
const GEMSTONE_NAMES: &[&str] = &[
    "amber", "jade", "onyx", "ruby", "opal", "pearl", "coral", "topaz", "jasper", "quartz",
    "garnet", "peridot", "citrine", "agate", "beryl", "zircon", "sapphire", "emerald", "lapis",
    "malachite", "turquoise", "obsidian", "moonstone", "sunstone", "jet", "pyrite", "hematite",
    "aquamarine", "tanzanite", "alexandrite",
];

/// Generate a unique workspace name not in the existing list
///
/// If all base names are used, appends a number suffix (e.g., "amber-2")
pub fn generate_workspace_name(existing: &[String]) -> String {
    let mut rng = rand::rng();

    // Try to find an unused name
    let mut available: Vec<&str> = GEMSTONE_NAMES
        .iter()
        .copied()
        .filter(|name| !existing.iter().any(|e| e == *name))
        .collect();

    if !available.is_empty() {
        available.shuffle(&mut rng);
        return available[0].to_string();
    }

    // All names used, find lowest available suffix
    let mut shuffled_names: Vec<&str> = GEMSTONE_NAMES.to_vec();
    shuffled_names.shuffle(&mut rng);

    for base_name in shuffled_names {
        for suffix in 2..=100 {
            let candidate = format!("{}-{}", base_name, suffix);
            if !existing.contains(&candidate) {
                return candidate;
            }
        }
    }

    // Fallback (extremely unlikely)
    format!("workspace-{}", uuid::Uuid::new_v4().as_simple())
}

/// Generate a branch name from username and workspace name
///
/// Format: `username/workspace-name`
pub fn generate_branch_name(username: &str, workspace_name: &str) -> String {
    let sanitized_username = sanitize_git_ref(username);
    format!("{}/{}", sanitized_username, workspace_name)
}

/// Get the username for branch naming
///
/// Priority:
/// 1. OS username (USER or USERNAME environment variable)
/// 2. git config user.name
/// 3. Fallback to "user"
pub fn get_git_username() -> String {
    // Try OS username first (USER on Unix, USERNAME on Windows)
    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            return sanitize_git_ref(&user);
        }
    }

    if let Ok(user) = std::env::var("USERNAME") {
        if !user.is_empty() {
            return sanitize_git_ref(&user);
        }
    }

    // Fall back to git config
    if let Ok(output) = std::process::Command::new("git")
        .args(["config", "user.name"])
        .output()
    {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return sanitize_git_ref(&name);
            }
        }
    }

    "user".to_string()
}

/// Sanitize a string for use in git refs
///
/// - Lowercase
/// - Replace spaces with hyphens
/// - Remove characters not allowed in git refs
fn sanitize_git_ref(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| match c {
            ' ' | '_' => '-',
            c if c.is_alphanumeric() || c == '-' || c == '.' => c,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_workspace_name_empty_existing() {
        let name = generate_workspace_name(&[]);
        assert!(GEMSTONE_NAMES.contains(&name.as_str()));
    }

    #[test]
    fn test_generate_workspace_name_avoids_existing() {
        let existing = vec!["amber".to_string(), "jade".to_string()];
        let name = generate_workspace_name(&existing);
        assert!(!existing.contains(&name));
    }

    #[test]
    fn test_generate_workspace_name_with_suffix() {
        let existing: Vec<String> = GEMSTONE_NAMES.iter().map(|s| s.to_string()).collect();
        let name = generate_workspace_name(&existing);
        assert!(name.contains('-'));
        // Should be something like "amber-2"
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(GEMSTONE_NAMES.contains(&parts[0]));
    }

    #[test]
    fn test_generate_branch_name() {
        let branch = generate_branch_name("fcoury", "amber");
        assert_eq!(branch, "fcoury/amber");
    }

    #[test]
    fn test_generate_branch_name_sanitizes() {
        let branch = generate_branch_name("Felipe Coury", "jade");
        assert_eq!(branch, "felipe-coury/jade");
    }

    #[test]
    fn test_sanitize_git_ref() {
        assert_eq!(sanitize_git_ref("Hello World"), "hello-world");
        assert_eq!(sanitize_git_ref("user_name"), "user-name");
        assert_eq!(sanitize_git_ref("John.Doe"), "john.doe");
        assert_eq!(sanitize_git_ref("--test--"), "test");
    }
}
