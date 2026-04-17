# tomegane

> The remote-seeing eye for AI agents — extract smart frames from screen recordings.

**Video in → smart frames + metadata out. Model-agnostic. Single Rust binary.**

tomegane lets AI agents "watch" screen recordings by extracting only the frames that matter. It uses perceptual hashing to detect meaningful UI state changes, so a 30-second recording becomes 5-10 key frames instead of 30+ near-identical ones.

```
screen-recording.mp4 → tomegane → [key frames + timestamps + metadata] → AI agent reasons about it
```

## Why?

AI agents can't watch videos. When a user says "here's a recording of the bug", the agent is blind. tomegane bridges this gap — it extracts the visually significant moments and hands them to the agent as images with context.

- **Model-agnostic** — tomegane doesn't call any LLM. It extracts frames; your agent does the reasoning.
- **MCP-native** — works as a tool in Claude Code, Cursor, or any MCP client.
- **Smart diffing** — perceptual hashing means you get the frames that matter, not every frame.
- **Single binary** — `cargo install` and you're done.

## Requirements

- **ffmpeg** — must be installed and on your PATH.
  - macOS: `brew install ffmpeg`
  - Ubuntu: `sudo apt install ffmpeg`
  - Windows: [ffmpeg.org/download](https://ffmpeg.org/download.html)

## Quick Start

### 1. Install tomegane

From crates.io:

```bash
cargo install tomegane
```

Or from source:

```bash
git clone https://github.com/yomete/tomegane.git
cd tomegane
cargo install --path .
```

### 2. Verify the install

```bash
tomegane --help
```

### 3. Set up MCP automatically

Ask `tomegane` to detect supported MCP clients and offer to add itself:

```bash
tomegane setup
```

This is the recommended setup path. It detects supported clients, checks whether `tomegane` is already configured, and offers to install the MCP entry for you.

By default this uses user-level config when supported. You can also target the current project or skip confirmation prompts:

```bash
tomegane setup --scope project
tomegane setup --yes
```

Supported setup targets right now:

- Claude Code
- Cursor
- Codex

Notes:

- Codex setup is currently user-scope only
- Cursor supports both user and project scope
- Claude Code support depends on the `claude` CLI being available on your `PATH`

To see what's already wired up without writing anything, run:

```bash
tomegane setup --list
```

### 4. Try it

```bash
tomegane analyze recording.mov --threshold 0.15
```

## Installation Details

If you just want the shortest path:

```bash
brew install ffmpeg
cargo install tomegane
tomegane setup
```

## CLI Usage

### Analyze a video

```bash
# Basic — extract frames at 1fps, output JSON to stdout
tomegane analyze recording.mov

# Smart frame selection — only keep frames with meaningful changes
tomegane analyze recording.mov --threshold 0.15

# Jank-oriented summary — highlight suspicious windows and repaint regions
tomegane analyze recording.mov --mode performance --interval 0.25

# Focus on a specific UI region
tomegane analyze recording.mov --crop 120,80,1440,900 --threshold 0.15

# Stream JSON events while frames are selected
tomegane analyze recording.mov --threshold 0.15 --stream

# Full control
tomegane analyze recording.mov \
  --interval 0.5 \
  --mode performance \
  --crop 120,80,1440,900 \
  --threshold 0.15 \
  --max-frames 20 \
  --output-dir ./frames \
  --output result.json \
  --base64
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `--interval` | `1.0` | Frame extraction interval in seconds |
| `--mode` | `overview` | `overview` for key frames, `performance` for jank-oriented insights |
| `--crop` | *(full frame)* | Region of interest in `x,y,w,h` format |
| `--threshold` | *(off)* | Change threshold for smart frame selection (0.0–1.0) |
| `--max-frames` | *(no limit)* | Maximum number of key frames to return |
| `--output-dir` | *(temp dir)* | Directory to save extracted frames |
| `--format` | `png` | Image format (`png` or `jpg`) |
| `--base64` | `false` | Include base64-encoded image data in JSON |
| `--output` | *(stdout)* | Write JSON to a file instead of stdout |
| `--stream` | `false` | Stream JSON events to stdout as frames are selected |

`--stream` emits newline-delimited JSON events to stdout. It is mutually
exclusive with `--output` and `--max-frames`; attempting to combine them is
rejected at argument-parse time.

### Exit codes

`tomegane analyze` uses distinct exit codes so scripts can branch on the
failure reason:

| Code | Meaning |
|------|---------|
| `0` | Success |
| `2` | Invalid argument (bad threshold, crop, format, incompatible flags) |
| `3` | ffmpeg not installed |
| `4` | I/O, missing video, ffmpeg failure, or no frames extracted |
| `1` | Anything else |

### Logging

Internal logs are emitted via `tracing`. Pass `-v` / `--verbose` to enable
debug output, or set `RUST_LOG=tomegane=debug` for fine-grained control:

```bash
RUST_LOG=tomegane=debug tomegane analyze recording.mov
```

### Output

```json
{
  "source": "recording.mov",
  "analysis_mode": "performance",
  "duration_seconds": 33.4,
  "total_frames_extracted": 67,
  "key_frames": [
    {
      "index": 0,
      "timestamp_seconds": 0.0,
      "image_path": "/tmp/tomegane/frame_0001.png",
      "change_score": 0.0,
      "description": "initial_state"
    },
    {
      "index": 12,
      "timestamp_seconds": 6.0,
      "image_path": "/tmp/tomegane/frame_0013.png",
      "change_score": 0.453,
      "description": "major_change"
    }
  ],
  "frame_count": 8,
  "output_format": "png",
  "performance_insights": {
    "summary": "Elevated visual churn from 12.5s to 15.0s stays concentrated around x=940, y=180, w=320, h=620; if that interaction felt laggy, this pattern often lines up with repeated rerender or layout work in one UI region.",
    "average_change_score": 0.11,
    "peak_change_score": 0.36,
    "elevated_change_threshold": 0.14,
    "frame_deltas": [
      {
        "from_index": 24,
        "to_index": 25,
        "start_timestamp_seconds": 12.0,
        "end_timestamp_seconds": 12.5,
        "change_score": 0.22,
        "changed_area_ratio": 0.08,
        "hotspot": {
          "x": 940,
          "y": 180,
          "width": 320,
          "height": 620,
          "coverage_ratio": 0.11
        }
      }
    ],
    "suspicious_windows": [
      {
        "start_timestamp_seconds": 12.5,
        "end_timestamp_seconds": 15.0,
        "sample_count": 5,
        "average_change_score": 0.24,
        "peak_change_score": 0.36,
        "average_changed_area_ratio": 0.09,
        "hotspot": {
          "x": 940,
          "y": 180,
          "width": 320,
          "height": 620,
          "coverage_ratio": 0.11
        },
        "assessment": "Sustained localized churn. If the UI felt sticky here, inspect rerenders or layout work in this region."
      }
    ]
  }
}
```

`performance` mode is still a visual heuristic. It helps narrow down where lag-like motion clusters, but it does not replace a real profiler.

## MCP Server

tomegane runs as an MCP server over stdin/stdout. Any MCP-compatible client can use it.

### Recommended setup

Use the built-in setup flow:

```bash
tomegane setup
```

This is the smoothest option for:

- Claude Code
- Cursor
- Codex

### Manual setup

If you want to configure a client manually, the MCP server command is:

```bash
tomegane mcp
```

The equivalent MCP server definition is:

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

If you built from source and have not installed `tomegane` on your `PATH`, use the absolute path to the binary instead:

```json
{
  "mcpServers": {
    "tomegane": {
      "command": "/path/to/tomegane/target/release/tomegane",
      "args": ["mcp"]
    }
  }
}
```

The only thing clients need is:

- `command`: the `tomegane` binary
- `args`: `["mcp"]`

### MCP Tools

#### `analyze_video`

Extract key frames from a screen recording.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `video_path` | string | yes | — | Absolute path to the video file |
| `threshold` | number | no | `0.15` | Change threshold (0.0–1.0) |
| `max_frames` | integer | no | `20` | Max frames to return |
| `interval` | number | no | `0.5` | Extraction interval in seconds |
| `mode` | string | no | `overview` | `overview` or `performance` |
| `crop` | string | no | — | Region of interest in `x,y,w,h` format |
| `format` | string | no | `png` | `png` or `jpg` |
| `include_image_data` | boolean | no | `true` | Set to `false` to skip inline base64 image blocks and keep responses small |
| `output_dir` | string | no | *(temp dir)* | Persist extracted frames to a specific directory; returned `image_path`s stay valid after the call |

Returns a summary text block followed by alternating text annotations and
image content blocks for each key frame. Text annotations now include the
on-disk `image_path`, which is useful when `include_image_data` is `false`.
In `performance` mode the summary also includes likely jank windows,
average/peak change scores, and localized repaint hints. Errors carry a
stable machine-readable tag like `[tomegane error:video_not_found]` at the
start of the text body.

#### `get_frame`

Extract a single frame at a specific timestamp.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `video_path` | string | yes | Absolute path to the video file |
| `timestamp_seconds` | number | yes | Timestamp to extract |
| `crop` | string | no | Region of interest in `x,y,w,h` format |

Returns the frame as an MCP image content block.

#### `compare_frames`

Compare two frames at different timestamps with a perceptual similarity score.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `video_path` | string | yes | Absolute path to the video file |
| `timestamp_a` | number | yes | First timestamp |
| `timestamp_b` | number | yes | Second timestamp |
| `crop` | string | no | Region of interest in `x,y,w,h` format |

Returns both frames with a change score (0.0 = identical, 1.0 = completely different).

## How it works

1. **Frame extraction** — shells out to `ffmpeg` to extract frames at the configured interval
2. **Perceptual hashing** — computes a DCT-based perceptual hash from low-frequency image coefficients
3. **Smart selection** — compares consecutive frame hashes via hamming distance; only keeps frames where the change exceeds the threshold
4. **Performance heuristics** — in `performance` mode, inspects consecutive frames for elevated change windows and localized repaint regions
5. **Output** — returns structured JSON (CLI) or MCP image content blocks (MCP server)

## Name

*Tomegane* (遠眼) comes from the **Tōmegane no Jutsu** (遠眼の術) — the Third Hokage's Crystal Ball Jutsu from Naruto. It lets you see what's happening remotely. That's exactly what this tool does for AI agents.

## License

MIT
