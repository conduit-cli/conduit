//! Utility modules

pub mod names;
pub mod paths;
pub mod title_generator;
pub mod tools;

pub use names::{generate_branch_name, generate_workspace_name, get_git_username};
pub use paths::{
    data_dir, database_path, init_data_dir, log_file_path, logs_dir,
    migrate_worktrees_to_workspaces, workspaces_dir,
};
pub use title_generator::{generate_title_and_branch, sanitize_branch_suffix, GeneratedMetadata};
pub use tools::{Tool, ToolAvailability, ToolPaths, ToolStatus};
