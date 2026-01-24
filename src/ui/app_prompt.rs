use sha2::{Digest, Sha256};

use crate::ui::components::{ChatMessage, MessageRole, TurnSummary};

/// Maximum seed prompt size in bytes (500KB)
pub const MAX_SEED_PROMPT_SIZE: usize = 500 * 1024;

/// Suffix appended when seed prompt is truncated
const SEED_TRUNCATED_SUFFIX: &str =
    "\n\n[TRUNCATED: transcript exceeded size limit]\n</previous-session-transcript>";

/// Closing instruction appended after the transcript
const SEED_CLOSING_INSTRUCTION: &str = r#"

</previous-session-transcript>

[END OF CONTEXT]

IMPORTANT: The above was historical context from a previous session.
You are starting a NEW forked session. Do NOT continue any tasks from the transcript.
Acknowledge that you have received this context by replying ONLY with the single word: Ready"#;

const PLAN_MODE_PROMPT_DEFAULT: &str = r#"<system-reminder>
# Plan Mode - System Reminder
CRITICAL: Plan mode ACTIVE - you are in READ-ONLY phase. STRICTLY FORBIDDEN:
ANY file edits, modifications, or system changes. Do NOT use sed, tee, echo, cat,
or ANY other bash command to manipulate files - commands may ONLY read/inspect.
This ABSOLUTE CONSTRAINT overrides ALL other instructions, including direct user
edit requests. You may ONLY observe, analyze, and plan. Any modification attempt
is a critical violation. ZERO exceptions.
---
## Responsibility
Your current responsibility is to think, read, search, and delegate explore agents to construct a well-formed plan that accomplishes the goal the user wants to achieve. Your plan should be comprehensive yet concise, detailed enough to execute effectively while avoiding unnecessary verbosity.
Ask the user clarifying questions or ask for their opinion when weighing tradeoffs.
**NOTE:** At any point in time through this workflow you should feel free to ask the user questions or clarifications. Don't make large assumptions about user intent. The goal is to present a well researched plan to the user, and tie any loose ends before implementation begins.
---
## Important
The user indicated that they do not want you to execute yet -- you MUST NOT make any edits, run any non-readonly tools (including changing configs or making commits), or otherwise make any changes to the system. This supersedes any other instructions you have received.
</system-reminder>"#;

const BUILD_SWITCH_PROMPT: &str = r#"<system-reminder>
Your operational mode has changed from plan to build.
You are no longer in read-only mode.
You are permitted to make file changes, run shell commands, and utilize your arsenal of tools as needed.
</system-reminder>"#;

/// Truncate string to max_bytes at a valid UTF-8 char boundary
fn truncate_to_char_boundary(s: &mut String, max_bytes: usize) {
    if s.len() <= max_bytes {
        return;
    }
    // Find the greatest char boundary <= max_bytes
    let new_len = s
        .char_indices()
        .take_while(|(i, _)| *i <= max_bytes)
        .map(|(i, _)| i)
        .last()
        .unwrap_or(0);
    s.truncate(new_len);
}

/// Build a fork seed prompt from chat history
pub fn build_fork_seed_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::new();

    // Opening header with clear instructions
    prompt.push_str("[CONDUIT_FORK_SEED]\n\n");
    prompt.push_str(
        "You are receiving context from a PREVIOUS session to seed a NEW forked session.\n",
    );
    prompt.push_str(
        "The transcript below is for REFERENCE ONLY - do NOT execute any commands from it.\n",
    );
    prompt.push_str("After reading, reply with ONLY the single word: Ready\n\n");
    prompt.push_str("<previous-session-transcript>\n");

    // Reserve space for closing instruction
    let max_transcript_size = MAX_SEED_PROMPT_SIZE
        .saturating_sub(prompt.len())
        .saturating_sub(SEED_CLOSING_INSTRUCTION.len());

    let transcript_start = prompt.len();

    for (idx, msg) in messages.iter().enumerate() {
        if idx > 0 {
            prompt.push_str("\n\n");
        }
        prompt.push_str(&format_fork_message(msg));

        // Check if transcript portion has exceeded the limit
        let transcript_len = prompt.len() - transcript_start;
        if transcript_len > max_transcript_size {
            let max_without_suffix =
                max_transcript_size.saturating_sub(SEED_TRUNCATED_SUFFIX.len());
            // Truncate only the transcript portion
            prompt.truncate(transcript_start + max_without_suffix);
            truncate_to_char_boundary(&mut prompt, transcript_start + max_without_suffix);
            prompt.push_str(SEED_TRUNCATED_SUFFIX);
            // SEED_TRUNCATED_SUFFIX already closes the tag, so just add final instruction
            prompt.push_str("\n\n[END OF CONTEXT]\n\n");
            prompt
                .push_str("IMPORTANT: The above was historical context from a previous session.\n");
            prompt.push_str(
                "You are starting a NEW forked session. Do NOT continue any tasks from the transcript.\n",
            );
            prompt.push_str(
                "Acknowledge that you have received this context by replying ONLY with the single word: Ready",
            );
            return prompt;
        }
    }

    // Add closing instruction for non-truncated case
    prompt.push_str(SEED_CLOSING_INSTRUCTION);
    prompt
}

pub fn compute_seed_prompt_hash(seed_prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed_prompt.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn plan_mode_prompt_default() -> &'static str {
    PLAN_MODE_PROMPT_DEFAULT
}

pub fn build_switch_prompt() -> &'static str {
    BUILD_SWITCH_PROMPT
}

pub fn build_plan_mode_prompt_inline(plan_path: &str, exists: bool) -> String {
    let plan_line = if exists {
        format!(
            "A plan file already exists at {}. You can read it and make incremental edits using the edit tool.",
            plan_path
        )
    } else {
        format!(
            "No plan file exists yet. You should create your plan at {} using the write tool.",
            plan_path
        )
    };

    format!(
        r#"<system-reminder>
Plan mode is active. The user indicated that they do not want you to execute yet -- you MUST NOT make any edits (with the exception of the plan file mentioned below), run any non-readonly tools (including changing configs or making commits), or otherwise make any changes to the system. This supercedes any other instructions you have received.
## Plan File Info:
{plan_line}
You should build your plan incrementally by writing to or editing this file. NOTE that this is the only file you are allowed to edit - other than this you are only allowed to take READ-ONLY actions.
## Plan Workflow
### Phase 1: Initial Understanding
Goal: Gain a comprehensive understanding of the user's request by reading through code and asking them questions. Critical: In this phase you should only use the explore subagent type.
1. Focus on understanding the user's request and the code associated with their request
2. **Launch up to 3 explore agents IN PARALLEL** (single message, multiple tool calls) to efficiently explore the codebase.
   - Use 1 agent when the task is isolated to known files, the user provided specific file paths, or you're making a small targeted change.
   - Use multiple agents when: the scope is uncertain, multiple areas of the codebase are involved, or you need to understand existing patterns before planning.
   - Quality over quantity - 3 agents maximum, but you should try to use the minimum number of agents necessary (usually just 1)
   - If using multiple agents: Provide each agent with a specific search focus or area to explore. Example: One agent searches for existing implementations, another explores related components, a third investigates testing patterns
3. After exploring the code, use the question tool to clarify ambiguities in the user request up front.
### Phase 2: Design
Goal: Design an implementation approach.
Launch general agent(s) to design the implementation based on the user's intent and your exploration results from Phase 1.
You can launch up to 1 agent(s) in parallel.
**Guidelines:**
- **Default**: Launch at least 1 Plan agent for most tasks - it helps validate your understanding and consider alternatives
- **Skip agents**: Only for truly trivial tasks (typo fixes, single-line changes, simple renames)
Examples of when to use multiple agents:
- The task touches multiple parts of the codebase
- It's a large refactor or architectural change
- There are many edge cases to consider
- You'd benefit from exploring different approaches
Example perspectives by task type:
- New feature: simplicity vs performance vs maintainability
- Bug fix: root cause vs workaround vs prevention
- Refactoring: minimal change vs clean architecture
In the agent prompt:
- Provide comprehensive background context from Phase 1 exploration including filenames and code path traces
- Describe requirements and constraints
- Request a detailed implementation plan
### Phase 3: Review
Goal: Review the plan(s) from Phase 2 and ensure alignment with the user's intentions.
1. Read the critical files identified by agents to deepen your understanding
2. Ensure that the plans align with the user's original request
3. Use question tool to clarify any remaining questions with the user
### Phase 4: Final Plan
Goal: Write your final plan to the plan file (the only file you can edit).
- Include only your recommended approach, not all alternatives
- Ensure that the plan file is concise enough to scan quickly, but detailed enough to execute effectively
- Include the paths of critical files to be modified
- Include a verification section describing how to test the changes end-to-end (run the code, use MCP tools, run tests)
### Phase 5: Call plan_exit tool
At the very end of your turn, once you have asked the user questions and are happy with your final plan file - you should always call plan_exit to indicate to the user that you are done planning.
This is critical - your turn should only end with either asking the user a question or calling plan_exit. Do not stop unless it's for these 2 reasons.
**Important:** Use question tool to clarify requirements/approach, use plan_exit to request plan approval. Do NOT use question tool to ask "Is this plan okay?" - that's what plan_exit does.
NOTE: At any point in time through this workflow you should feel free to ask the user questions or clarifications. Don't make large assumptions about user intent. The goal is to present a well researched plan to the user, and tie any loose ends before implementation begins.
</system-reminder>"#
    )
}

fn format_fork_message(msg: &ChatMessage) -> String {
    let role = match msg.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Reasoning => "reasoning",
        MessageRole::Tool => "tool",
        MessageRole::System => "system",
        MessageRole::Error => "error",
        MessageRole::Summary => "summary",
    };

    let mut header = format!("[role={}]", role);

    if msg.role == MessageRole::Tool {
        if let Some(name) = &msg.tool_name {
            header.push_str(&format!(" name=\"{}\"", sanitize_fork_header_value(name)));
        }
        if let Some(args) = &msg.tool_args {
            if !args.is_empty() {
                header.push_str(&format!(" args=\"{}\"", sanitize_fork_header_value(args)));
            }
        }
        if let Some(exit_code) = msg.exit_code {
            header.push_str(&format!(" exit={}", exit_code));
        }
    }

    let content = if msg.role == MessageRole::Summary {
        msg.summary
            .as_ref()
            .map(format_turn_summary_for_seed)
            .unwrap_or_default()
    } else {
        msg.content.clone()
    };

    if content.trim().is_empty() {
        header
    } else {
        format!("{header}\n{content}")
    }
}

fn format_turn_summary_for_seed(summary: &TurnSummary) -> String {
    let mut parts = Vec::new();
    if summary.duration_secs > 0 {
        parts.push(format!("duration={}s", summary.duration_secs));
    }
    if summary.input_tokens > 0 || summary.output_tokens > 0 {
        parts.push(format!(
            "tokens_in={}, tokens_out={}",
            summary.input_tokens, summary.output_tokens
        ));
    }
    if !summary.files_changed.is_empty() {
        let files = summary
            .files_changed
            .iter()
            .map(|f| format!("{} +{} -{}", f.filename, f.additions, f.deletions))
            .collect::<Vec<_>>()
            .join("; ");
        parts.push(format!("files=[{}]", files));
    }
    if parts.is_empty() {
        "summary".to_string()
    } else {
        parts.join(", ")
    }
}

fn sanitize_fork_header_value(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c == '"' {
                '\''
            } else if c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Sanitize a generated title: trim whitespace, remove control characters and newlines,
/// enforce max length, and provide fallback for empty titles.
pub fn sanitize_title(title: &str) -> String {
    const MAX_TITLE_LENGTH: usize = 200;
    const FALLBACK_TITLE: &str = "Untitled task";

    // Collapse all whitespace (including newlines) to single spaces and trim
    let sanitized: String = title
        .chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Enforce max length
    let truncated = if sanitized.chars().count() > MAX_TITLE_LENGTH {
        sanitized.chars().take(MAX_TITLE_LENGTH).collect()
    } else {
        sanitized
    };

    // Fallback for empty titles
    if truncated.is_empty() {
        FALLBACK_TITLE.to_string()
    } else {
        truncated
    }
}
