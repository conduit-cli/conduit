# Workspaces: Worktrees vs Checkouts

Conduit can manage workspaces in two modes:

- **Worktrees (default):** Lightweight checkouts that share the base repository’s git metadata.
- **Checkouts:** Full clones of the repository for stronger isolation.

Each repository chooses one mode. The mode cannot be changed while active workspaces exist.
When you create the first workspace for a repository, Conduit will ask which mode to use.
