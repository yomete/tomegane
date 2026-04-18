# tomegane

Extract smart frames from screen recordings for AI agents.

`tomegane` turns a video into a small set of useful frames plus metadata, so an agent can reason about what changed without stepping through the whole recording.

## Install

Requires `ffmpeg`.

```bash
brew install ffmpeg
cargo install tomegane
```

Or install from source:

```bash
git clone https://github.com/yomete/tomegane.git
cd tomegane
cargo install --path .
```

## Use

Extract key frames:

```bash
tomegane analyze recording.mov
```

Use smart frame selection:

```bash
tomegane analyze recording.mov --threshold 0.15
```

Look for likely jank windows:

```bash
tomegane analyze recording.mov --mode performance --interval 0.25
```

Common flags:

- `--threshold` keeps only meaningful visual changes
- `--interval` controls how often frames are sampled
- `--crop x,y,w,h` limits analysis to one region
- `--max-frames N` caps the result size
- `--output result.json` writes JSON to a file
- `--stream` emits newline-delimited JSON events as frames are selected

## MCP

Set it up automatically for supported clients:

```bash
tomegane setup
```

Current setup targets:

- Claude Code
- Cursor
- Codex

Useful setup commands:

```bash
tomegane setup --list
tomegane setup --scope project
tomegane setup --yes
```

Run the MCP server directly:

```bash
tomegane mcp
```

Manual config:

```json
{
  "mcpServers": {
    "tomegane": {
      "command": "tomegane",
      "args": ["mcp"]
    }
  }
}
```

## Output

CLI output is JSON with:

- source video metadata
- selected frame paths and timestamps
- change scores
- optional base64 image data
- optional performance insights in `--mode performance`

`performance` mode is a visual heuristic. It is useful for narrowing down suspicious windows, not a replacement for a real profiler.

## Why

AI agents cannot watch videos directly. `tomegane` gives them the few frames that matter.

## License

MIT
