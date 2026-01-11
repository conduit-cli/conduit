# Pending

- [ ] Sweep the code for "\_ = " error swallowing instances, add it to AGENTS.md and CLAUDE.md

- [ ] BUG: don't allow the user to send messages while streaming

- [ ] Handle claude ExitPlanMode and AskQuestion tools properly

- [ ] Ctrl+C once should clear the prompt, twice to quit.

- [ ] Validate number of tokens being shown by Codex when conversation ends:
      ─ ⏱ 22m 50s │ ↓33041.4k ↑141.1k ─────────────────

- [ ] When the PR is running checks, we display `PR #16 ⋯`. Instead of using just the ellipsis, we should display a spinner to indicate that checks are running. (See spinners-rs :-) )

- [ ] What do we do if we run the app on an environment where we don't have the required tools: git, codex and claude? We need to check it on startup and show a message dialog explaining the issue and how to fix it.

- [ ] Have a key (Ctrl+R) to refresh PR status, etc.?

- [ ] Have a way to move between user inputs on the history

- [ ] Redesign model picker: no longer a good place to display it, make it similar to the new project dialog with search.

- [ ] Allow Ctrl+Z

- [ ] Allow editing the prompt with external editor

- [ ] Disallow multiple parallel executions of the app, since we save and restore settings. This app is not really designed to be executed with multiple instances in parallel, it is an app that controls the multiple instances itself. Think of an elegant solution to this problem (maybe the last one assumes? But we need to pass the state around if that's the case). Maybe implement a simple message and allow the user to kill the running instance (which will persist settings) and reload the current session.
  - Just had an idea: show the application that's last used faded out and with a message saying another instance is in use. When user clicks we ask if he wants to move focus to this instance, load from database and re-render everything.

- [ ] Accept ! to execute commands, capture the output and stderr

- [ ] Project settings
  - [ ] Choose base branch

- [ ] Make project selector larger and wide for large number of projects with pontentially large names

- [ ] Better display for the user input text on the conversation area, inspired by OpenCode

- [ ] It seems like we are only seeing incoming messages when loading messages from history
- [ ] Tab area overflow scrolling (or how to handle tabs overflowing to the right)
- [ ] Auto name branch based on initial conversation
- [ ] Support slash commands
- [ ] Make imported sessions read-only by default
- [ ] Continue teaser video from session f7264a2d-a078-4c61-87a9-83754a7561b4

## In Progress

- [-] New command palette (Ctrl+P) to search for commands and actions

## Done

- [x] Map Tab key map to switching between Plan and Build modes and change current Tab and Shift+Tab behavior to Alt+Tab and Alt+Shift+Tab for switching between open workspaces

- [x] Implement selection: for both the conversation and prompt input areas.

- [x] Improve tool calling sections for Claude Code

- [x] Add a new action to copy the current workspace path to clipboard, map it to Alt+Shift+C

- [-] Improve PR tracking.
  - [x] Remove PR indicator from tab.
  - [x] Simplify what is displayed on the right-hand side of the prompt area, remove project and path, leaving just branch name. Prepend it with PR # if available plus our custom dot. If there are any git changes, display git status like +2 (changes/green), -66 (deletions/red). Order PR (if there is one) <dot> git status <dot> branch.
  - [x] Consider displaying similar git status indicators on workspace sidebar for insight just by looking at the side bar. Mock it up.
  - [x] When PR is open, show PR background color as green (same as GitHub), when merged Blue, research other states. Clicking the PR should open it in the browser.
  - [x] Add a background PR and git status tracker. Explore the best way to do this to NEVER block the main thread. Either by leveraging threads or async tasks or both.
    - [x] We need to poll for PRs, define a sane interval on a constant so we can tweak it.
    - [x] We also need to monitor git status, what is the best way? Use something like fs watcher or busy wait like PR. Design both solutions with performance in mind.

- [x] BUG: when reusing an old workspace branch name and directory, we have a few issues like reusing the PRs, the branch is not deleted and sometimes it ressurects old conversations.
- [x] BUG: when the conversation is in progress and you try to scroll down, it always push you back down. We may need to notice we are scrolling and stop auto moving the cursor to the end.
- [x] HIGH PRIORITY: how to handle compaction?
- [x] BUG: if you click around the last 4 characters of a tab, it selects the next one instead

- [x] BUG: Ctrl+C says Interrupt but the process continues
  - [x] actually interrupt the process
  - [x] When the system is generating the response ("thinking"):
    - [x] Make the first Ctrl+C show "Press Ctrl+C again to interrupt" and then interrupt the process, save app status and quit
    - [x] Similar with Esc key twice, but just interrupt, no quitting
  - [x] When the system is idle and the user is typing a prompt:
    - [x] Make Ctrl+C once clear the prompt input and shows "Ctrl-C again to quit", twice quits the app
    - [x] Esc once shows "press Esc again to clear", then twice clears the prompt input

- [x] Redefine spinner + message area, info display and statusbar
- [x] BUG: typing Ctrl+J while on the sidebar adds lines to the prompt input
- [x] When we have no workspaces under a project, it shows collapsed. It has to be shown expanded.
- [x] Dialogs are not showing when there's no workspace open. I tried Alt+I to import a session. Then when you open a tab the dialog is visible. Can you help me by compiling a list of all the keys that open dialogs so we can check which ones should be valid in this initial state?

- [ ] Investigate why sometimes the git status are not showing on the sidebar:
      ![Sidebar showing vast-snow with no git status](/Users/fcoury/Library/Application/%20Support/CleanShot/media/media_J7Q6Kciems/CleanShot/%202026-01-07/%20at/%2015.00.09@2x.png)
      ![Prompt shows the git status](/Users/fcoury/Library/Application/%20Support/CleanShot/media/media_U6MdBjM1Vi/CleanShot/%202026-01-07/%20at/%2015.00.53@2x.png)
