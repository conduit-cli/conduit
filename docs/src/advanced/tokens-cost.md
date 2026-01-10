# Token Usage & Cost Tracking

Monitor your AI usage in real-time.

## Status Bar Display

The status bar shows:
- **Input tokens** — Tokens sent to the model
- **Output tokens** — Tokens received
- **Estimated cost** — Based on model pricing

## Configuration

```toml
# Enable/disable display
show_token_usage = true
show_cost = true

# Pricing (per million tokens)
claude_input_cost_per_million = 3.0
claude_output_cost_per_million = 15.0
```

## Toggle Display

Press `Alt+P` to toggle the metrics display.

## Model Pricing

| Model | Input | Output |
|-------|-------|--------|
| Claude Sonnet 3.5 | $3.00/1M | $15.00/1M |
| Claude Opus | $15.00/1M | $75.00/1M |
| Claude Haiku | $0.25/1M | $1.25/1M |

## Context Window

The display also shows context usage relative to the model's limit (e.g., 200K for Claude).
