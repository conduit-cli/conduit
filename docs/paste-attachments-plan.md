# Paste Large Files & Images in Chat — Initial Plan

## Summary
Enable users to paste large text blocks (or files) and images into the chat input. Large pastes should not explode the UI; images should be attached and forwarded to the agent where supported.

## Codex reference behavior (source: ~/code/codex)
- Large paste threshold (~1000 chars) inserts a placeholder in the input and stores the real content in a pending list. On submit, placeholders are expanded to full text.
- Paste-burst detection captures fast keypresses as paste when terminals don’t emit paste events.
- Image paste (Ctrl+V / Alt+V) reads clipboard images, writes a temp PNG, and inserts an image placeholder; images are sent as local image paths.
- Pasted file paths that are images are detected and attached as images.

## Implementation Plan (Conduit)

### 1) Input handling & paste events
- Handle `Event::Paste` in `src/ui/app.rs` and forward to the active session input.
- Normalize CRLF (`\r`) to LF (`\n`) before handing off.

### 2) Large paste placeholders
- Extend input state to track `pending_pastes: Vec<(placeholder, text)>`.
- On paste: if `text.len() > LARGE_PASTE_CHAR_THRESHOLD`, insert a placeholder (e.g., `[Pasted Content 1234 chars]`) into the input and store the real text.
- On submit: expand placeholders into the full text **before** sending to the agent.

**Relay to agents**: large pastes are expanded into the final prompt string; the agent receives only normal text (no placeholders).

### 3) Image attachments (clipboard + path)
- Add `attached_images: Vec<AttachedImage>` to input state (placeholder + local path + dimensions).
- Clipboard image paste (Ctrl+V / Alt+V): use `arboard` + `image` to read, encode PNG, write to a temp file, then attach.
- Pasted file path detection: if the pasted text resolves to an image file, attach it instead of inserting raw text.

### 4) Forward attachments to agents
- Introduce `AgentStartConfig.images: Vec<PathBuf>` (or similar) and plumb through `submit_prompt`.
- **Codex**: pass images via CLI flags (`--image`), similar to Codex exec in upstream.
- **Claude**: decide behavior (warn + skip images, or map to the appropriate Claude input if supported). This should be explicit in UI.

**Relay to agents**: images are forwarded as local paths for Codex; large text is expanded in the prompt string. This is the only relay needed for large pastes.

### 5) UI affordances
- Keep placeholders visible in the input so users can see attachments/paste summary.
- Optional: show “N images attached” in the status bar.

### 6) Tests
- Unit tests for:
  - placeholder insertion/expansion
  - deleting placeholders removes attachment
  - image path paste attaches image

## Open Questions
- Should large pastes ever be sent as separate file attachments (vs expanded into text)?
- Claude support for image attachments: do we want to block, degrade, or attempt API support?
