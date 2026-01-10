# conduit migrate-theme

Convert VS Code themes to Conduit's native TOML format.

## Usage

```bash
conduit migrate-theme INPUT [OPTIONS]
```

## Arguments

| Argument | Description |
|----------|-------------|
| `INPUT` | Path to VS Code theme JSON file |

## Options

| Option | Description |
|--------|-------------|
| `--output PATH` | Output path (default: `~/.conduit/themes/<name>.toml`) |
| `--palette` | Extract common colors into a palette section |

## Examples

```bash
# Convert a VS Code theme
conduit migrate-theme ~/.vscode/extensions/theme-dracula/theme.json

# Specify output location
conduit migrate-theme theme.json --output ~/my-theme.toml

# Extract color palette
conduit migrate-theme theme.json --palette
```

## Output Format

The generated TOML file can be used directly:

```toml
[theme]
path = "~/.conduit/themes/my-theme.toml"
```

## Supported Themes

Most VS Code color themes are supported. The migration extracts:
- Editor colors
- Syntax highlighting
- UI element colors
