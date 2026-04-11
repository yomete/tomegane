---
tags:
  - Rust
  - TypeScript
  - video analysis
  - frame extraction
  - perceptual hashing
tools:
  - ffmpeg
  - ffprobe
  - clap
  - Serde
  - Rayon
frameworks:
  - MCP
projects:
  - tomegane
---
# tomegane Mini-Book

> A TypeScript developer's guide to how this Rust library works.

This document is meant to be read casually, not just used as API reference.
It explains how `tomegane` is put together, why the code looks the way it does,
and how to map Rust concepts to JavaScript/TypeScript mental models.

If you are comfortable in TypeScript but not in Rust, this is the right place to start.

## 1. What tomegane actually is

At a high level, `tomegane` is three things at once:

1. A Rust library that analyzes a video and returns selected frames plus metadata.
2. A CLI binary that exposes that library as a command-line tool.
3. An MCP server that exposes that library as tools for AI agents.

The core product idea is simple:

```plaintext
video file
  -> extract frames
  -> compare them perceptually
  -> keep the meaningful ones
  -> return structured output
```

If you were building this in TypeScript, you might imagine:

- a core `analyzeVideo()` function
- a CLI wrapper around it
- an API server wrapper around it

That is exactly how this repo is structured, just in Rust.

## 2. Repo map

The most important files are:

- [src/lib.rs](/Users/yomi/Documents/batcave/tomegane/src/lib.rs)
- [src/main.rs](/Users/yomi/Documents/batcave/tomegane/src/main.rs)
- [src/cli.rs](/Users/yomi/Documents/batcave/tomegane/src/cli.rs)
- [src/extract/ffmpeg.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/ffmpeg.rs)
- [src/extract/diff.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/diff.rs)
- [src/output/schema.rs](/Users/yomi/Documents/batcave/tomegane/src/output/schema.rs)
- [src/mcp/mod.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/mod.rs)
- [src/mcp/handlers.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/handlers.rs)
- [src/mcp/protocol.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/protocol.rs)
- [tests/integration\_test.rs](/Users/yomi/Documents/batcave/tomegane/tests/integration_test.rs)

The rough responsibilities are:

- `lib.rs`: the real application logic
- `main.rs`: wire CLI input to library calls
- `cli.rs`: define command-line flags with Clap
- `extract/ffmpeg.rs`: shell out to `ffmpeg` and `ffprobe`
- `extract/diff.rs`: perceptual hashing and key-frame selection
- `output/schema.rs`: serializable output structs/enums
- `mcp/*`: expose the same functionality over MCP
- `tests/*`: end-to-end confidence that behavior still works

## 3. The architectural shape

Think of the crate in layers.

### Layer 1: Extraction

This is the "get the raw data from the video" layer.

- Read video duration
- Extract many frames
- Extract a single frame
- Optionally crop before analysis

That lives in [src/extract/ffmpeg.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/ffmpeg.rs).

### Layer 2: Diffing / selection

This is the "which frames matter?" layer.

- Compute a perceptual hash for each image
- Compare hashes with Hamming distance
- Decide which frames are "different enough" to keep

That lives in [src/extract/diff.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/diff.rs).

### Layer 3: Orchestration

This is the "run the whole pipeline" layer.

- Validate inputs
- Create output directories
- Call ffmpeg extraction
- Gather frame paths
- Select key frames
- Build the final result object

That lives in [src/lib.rs](/Users/yomi/Documents/batcave/tomegane/src/lib.rs).

### Layer 4: Adapters

This is the "how users/agents talk to the library" layer.

- CLI adapter in [src/main.rs](/Users/yomi/Documents/batcave/tomegane/src/main.rs)
- MCP adapter in [src/mcp/handlers.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/handlers.rs)

This separation is important. It means:

- the CLI is thin
- the MCP layer is thin
- the reusable behavior stays in the library

That is good design in any language.

## 4. How to read Rust here if you think in TypeScript

This section is the translation table.

### `struct` \~= TypeScript object type + runtime container

Example:

```rust
pub struct AnalyzeOptions<'a> {
    pub interval: f64,
    pub output_dir: Option<&'a str>,
    pub format: &'a str,
    pub include_base64: bool,
    pub crop: Option<ffmpeg::CropRect>,
    pub threshold: Option<f64>,
    pub max_frames: Option<usize>,
}
```

TS mental model:

```typescript
type AnalyzeOptions = {
  interval: number
  output_dir?: string
  format: string
  include_base64: boolean
  crop?: CropRect
  threshold?: number
  max_frames?: number
}
```

The Rust version is an actual concrete data structure, not just a type annotation.

### `Option<T>` \~= `T | undefined`

Rust:

```rust
Option<f64>
```

TS:

```typescript
number | undefined
```

Rust makes this explicit and forces you to handle it.

### `Result<T, E>` \~= throwing, but explicit

Rust:

```rust
Result<AnalysisResult, String>
```

TS mental model:

```typescript
type Result = AnalysisResult | Error
```

But Rust makes this non-ambiguous and forces every caller to deal with success or failure.

If you see `?` in Rust, read it like:

"If this failed, return early with the error."

### `impl Default` \~= default config factory

Rust:

```rust
impl Default for AnalyzeOptions<'_> {
    fn default() -> Self { ... }
}
```

TS mental model:

```typescript
function defaultAnalyzeOptions(): AnalyzeOptions {
  return { ... }
}
```

### `enum` \~= discriminated union

In [src/output/schema.rs](/Users/yomi/Documents/batcave/tomegane/src/output/schema.rs), `StreamEvent` is very close to a TS discriminated union.

TS mental model:

```typescript
type StreamEvent =
  | { type: "started"; source: string; duration_seconds: number; ... }
  | { type: "frame"; frame: Frame }
  | { type: "completed"; result: AnalysisResult }
```

Rust enums are stronger than most JS devs expect. They are one of the biggest reasons Rust code can stay safe and readable.

### Borrowed strings like `&str`

`&str` means "a reference to a string slice" rather than an owned `String`.

In practice for this codebase, you can often think:

- `String` = owned string
- `&str` = borrowed string view

If you are a TS dev, just read `&str` as "read-only string input" most of the time.

### Lifetimes like `<'a>`

This looks scary:

```rust
pub struct AnalyzeOptions<'a> {
    pub output_dir: Option<&'a str>,
    pub format: &'a str,
}
```

For this repo, the practical interpretation is:

"This struct borrows some strings instead of owning them, and Rust wants to know how long those borrows live."

You do not need to become a lifetime expert to understand this library. Here it is mostly about avoiding unnecessary string allocation.

## 5. The main entry point: `analyze`

The heart of the library is in [src/lib.rs](/Users/yomi/Documents/batcave/tomegane/src/lib.rs).

The public entry points are:

- `analyze(video_path, options)`
- `analyze_stream(video_path, options, on_frame)`

If you were writing the library in TS, this would probably be:

```typescript
async function analyze(videoPath: string, options: AnalyzeOptions): Promise<AnalysisResult>
async function analyzeStream(
  videoPath: string,
  options: AnalyzeOptions,
  onFrame: (frame: Frame) => void
): Promise<AnalysisResult>
```

That is basically what the Rust API is doing.

### Why there are two functions

`analyze` is for:

- normal batch processing
- give me the final answer when you are done

`analyze_stream` is for:

- incremental processing in the CLI
- emit frames as they are selected

Both flow into a shared private function:

- `analyze_internal(...)`

This is a classic pattern:

- public wrappers for user-facing APIs
- one private implementation so logic is not duplicated

## 6. Step-by-step: what happens during analysis

Let’s walk the pipeline in order.

### Step 1: Validate the file and options

In `analyze_internal`, the code first checks:

- does the input video exist?
- is the image format supported?
- is threshold between `0.0` and `1.0`?

This is ordinary guard-clause code.

TS version might look like:

```typescript
if (!existsSync(videoPath)) throw new Error(...)
if (!["png", "jpg"].includes(options.format)) throw new Error(...)
if (options.threshold != null && (options.threshold < 0 || options.threshold > 1)) {
  throw new Error(...)
}
```

### Step 2: Verify `ffmpeg`

The library depends on external binaries:

- `ffmpeg`
- `ffprobe`

This code does not decode video itself. It delegates that job to battle-tested tools.

That is a pragmatic engineering choice:

- less custom video-processing logic
- better portability
- smaller Rust code surface

### Step 3: Determine video duration

`ffprobe` returns duration, which is used in the result metadata.

### Step 4: Choose where frames will be written

If `options.output_dir` exists:

- use it

Otherwise:

- create a temp dir

This is why the library can behave both as:

- an ephemeral analyzer
- a "save my extracted images" tool

### Step 5: Extract raw frames

This call happens in [src/extract/ffmpeg.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/ffmpeg.rs):

- apply `fps=1/interval`
- optionally prepend a crop filter
- write `frame_0001.png`, `frame_0002.png`, etc.

The crop support is implemented as `CropRect`, parsed from `x,y,w,h`.

This is a nice example of "small strong type over stringly input".

Instead of passing crop strings everywhere, the code parses once into:

```rust
CropRect { x, y, width, height }
```

That reduces duplication and centralizes validation.

### Step 6: Read back frame paths

After extraction, the library reads the directory, filters by extension, and sorts the results.

This is important because the selection logic assumes chronological order.

### Step 7: Decide which frames to keep

There are two modes.

#### Mode A: no threshold

If threshold is `None`, all frames are kept.

If `max_frames` is set, the code evenly samples frames.

TS mental model:

```typescript
if (!threshold) {
  let selected = allFrames
  if (maxFrames) selected = evenlySample(selected, maxFrames)
}
```

#### Mode B: thresholded key-frame selection

If threshold exists, `diff::select_key_frames(...)` computes perceptual hashes for all frames and then keeps frames whose change score is large enough relative to the last included frame.

That detail matters.

It does not compare every frame only to the immediately previous raw frame.
It compares to the last frame that was actually kept.

That avoids keeping many tiny incremental variations when the real UI state has not meaningfully changed.

### Step 8: Build `Frame` objects

For each selected frame, the library constructs:

- frame index
- timestamp
- image path
- optional base64
- change score
- description

The description is rule-based:

- first frame → `initial_state`
- score >= `0.5` → `major_change`
- score >= `0.2` → `moderate_change`
- score > `0.0` → `minor_change`
- else → fallback label

This is lightweight labeling, not AI labeling.

### Step 9: Return `AnalysisResult`

Final output includes:

- `source`
- `duration_seconds`
- `total_frames_extracted`
- `key_frames`
- `frame_count`
- `output_format`

That is the library’s canonical output object.

## 7. How frame extraction works

Open [src/extract/ffmpeg.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/ffmpeg.rs).

This file is intentionally small and focused.

Main responsibilities:

- check if `ffmpeg` exists
- get duration via `ffprobe`
- extract many frames
- extract a single frame
- parse crop values

### `CropRect`

`CropRect` is a plain struct representing:

- `x`
- `y`
- `width`
- `height`

Why not just keep this as a string?

Because if you keep it stringly typed, every caller has to remember:

- the order
- parsing rules
- validation rules

By parsing once, the rest of the code can stay strongly typed.

### `extract_frames`

This function shells out to `ffmpeg` with a generated filter string.

If crop exists, the filter chain is:

```plaintext
crop=...,fps=...
```

If crop does not exist:

```plaintext
fps=...
```

This is the kind of code that often becomes a mess in scripting languages because command construction is spread across many call sites. Here it is centralized.

### `extract_single_frame`

This is used by the MCP tools:

- `get_frame`
- `compare_frames`

Instead of duplicating a custom ffmpeg shell command in multiple places, the code now has one reusable function.

That is good refactoring and a common maintainability win.

## 8. How perceptual hashing works

The important logic is in [src/extract/diff.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/diff.rs).

This file answers:

"How visually different are these images?"

### Why perceptual hashing exists here

If you compared raw image bytes, tiny encoding changes would look huge.
If you compared pixels naively, tiny rendering or compression noise would cause instability.

Perceptual hashing tries to answer:

"Do these images look meaningfully similar to a human?"

### The current algorithm

This repo now uses a DCT-based pHash, not a simple mean hash.

High-level flow:

1. Load image
2. Resize to `32x32`
3. Convert to grayscale
4. Shift brightness values around zero
5. Compute low-frequency DCT coefficients
6. Keep the top-left `8x8`
7. Compare each coefficient to the mean
8. Pack the result into a 64-bit hash

### TypeScript mental model

If you were sketching this in JS:

```typescript
const img = decodeImage(file)
const gray = resizeAndGray(img, 32, 32)
const dct = compute2dDct(gray)
const lowFreq = topLeft8x8(dct)
const mean = average(lowFreq.slice(1))
const bits = lowFreq.map(v => v > mean ? 1 : 0)
const hash = packBits(bits)
```

That is nearly the algorithm.

### Why DCT-based pHash is better than mean hash

The older mean-hash approach is:

- simpler
- cheaper
- less accurate

The DCT version is better at focusing on structural visual information instead of raw local brightness.

That matters for UI recordings because:

- subtle layout changes matter
- tiny compression noise should not matter
- major screen transitions should register strongly

### `PHash(pub u64)`

This is a tiny newtype wrapper over `u64`.

Why wrap a `u64` instead of just using `u64` directly?

Because it adds meaning.

`u64` alone could mean anything.
`PHash(u64)` means:

"This number is a perceptual hash."

That makes APIs safer and clearer.

### Hamming distance

The code compares two hashes using:

```rust
(a.0 ^ b.0).count_ones()
```

That means:

- XOR the two 64-bit values
- count how many bits are different

Then normalize:

```rust
distance / 64.0
```

So final change score is from:

- `0.0` identical
- `1.0` completely different

## 9. How key-frame selection works

This is the behavioral core of the product.

Look at `select_key_frames(...)` in [src/extract/diff.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/diff.rs).

### The algorithm

1. Hash every extracted frame
2. Always keep frame `0`
3. For each next frame:

- compute change score against the last kept frame
- if score >= threshold, keep it

4. If `max_frames` is set and too many are selected:

- keep the first frame
- sort remaining selected frames by significance
- take the top changes
- re-sort chronologically

This is a good balance between:

- preserving chronology
- emphasizing meaningful changes
- keeping result size bounded

### Why compare to the last kept frame

Suppose a loading spinner moves 1 pixel per frame.

If you compare each frame to the immediately previous frame:

- every frame may look "different enough"

If you compare to the last kept frame:

- the code only keeps the next frame once the overall drift becomes meaningful

That is usually the desired behavior for UI inspection.

### Why hashing is parallelized

This line matters:

```rust
frame_paths.par_iter()
```

That uses Rayon for parallel iteration.

TS mental model:

This is closest to:

```typescript
await Promise.all(framePaths.map(phash))
```

except implemented with Rust’s CPU-thread parallelism rather than async I/O promises.

That makes sense because image hashing is CPU work, not network work.

## 10. Streaming mode

The library has:

- `analyze(...)`
- `analyze_stream(...)`

Streaming mode is used by the CLI.

Instead of waiting for the full result, it emits frames as they are selected via a callback.

### Why this is useful

For long videos, users do not want to wait for the final full payload before seeing progress.

### Why `max_frames` is blocked in streaming mode

This is a design tradeoff in the current implementation.

When `max_frames` is set, the batch selector may need to know all selected frames first, sort by importance, and then trim.

That conflicts with "emit each frame immediately as soon as you decide it matters."

So the current code explicitly rejects streaming + `max_frames`.

That is a good example of honest API design:

- do not pretend a feature combination works if semantics are unclear

## 11. CLI layer

The CLI is defined in [src/cli.rs](/Users/yomi/Documents/batcave/tomegane/src/cli.rs).

This uses the `clap` crate.

If you are a TS dev, `clap` is in the same ecosystem category as:

- Commander
- yargs
- oclif flag parsing

The CLI defines:

- subcommands
- flags
- defaults
- help output

### `Cli` and `Commands`

These are Rust data structures derived from command-line parsing.

TS mental model:

```typescript
type Commands =
  | {
      type: "analyze"
      video_path: string
      interval: number
      ...
    }
  | { type: "mcp" }
```

That is very close to what the Rust enum is doing.

### `main.rs`

The binary entry point in [src/main.rs](/Users/yomi/Documents/batcave/tomegane/src/main.rs):

1. parses CLI args
2. converts CLI strings into typed config like `CropRect`
3. builds `AnalyzeOptions`
4. calls library functions
5. prints JSON or emits stream events

The important design point is this:

`main.rs` does very little real business logic.

That is good. It keeps the core library reusable and testable.

## 12. Output schema

Open [src/output/schema.rs](/Users/yomi/Documents/batcave/tomegane/src/output/schema.rs).

This file defines the shapes that get serialized into JSON.

The important types are:

- `Frame`
- `AnalysisResult`
- `StreamEvent`

These are the runtime equivalents of TS API response types.

### `#[derive(Serialize)]`

This is roughly:

"Make this type JSON-serializable."

With Serde, Rust structs can serialize similarly to plain JS objects.

### `#[serde(skip_serializing_if = "Option::is_none")]`

This means:

"Do not include this field in JSON when it is `None`."

TS analogy:

```typescript
const output = {
  ...(image_base64 ? { image_base64 } : {})
}
```

It keeps the JSON cleaner.

## 13. MCP layer

The MCP server is split into:

- [src/mcp/protocol.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/protocol.rs)
- [src/mcp/mod.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/mod.rs)
- [src/mcp/handlers.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/handlers.rs)

### `mcp/mod.rs`

This is the transport loop.

Responsibilities:

- read newline-delimited JSON-RPC messages from stdin
- deserialize requests
- dispatch by method name
- serialize responses to stdout

TS mental model:

```typescript
for await (const line of stdinLines()) {
  const request = JSON.parse(line)
  const response = handleRequest(request)
  if (request.id != null) stdout.write(JSON.stringify(response) + "\n")
}
```

That is very close to what this module does.

### `mcp/handlers.rs`

This is the adapter from MCP tool calls to library functions.

It defines three tools:

- `analyze_video`
- `get_frame`
- `compare_frames`

This file is effectively the controller layer.

It:

- validates incoming JSON args
- parses crop if present
- calls the library or ffmpeg helper
- converts results into MCP `ContentBlock`s

### Why MCP is separate from `lib.rs`

Because MCP is only one delivery channel.

The core library should not know:

- JSON-RPC
- content blocks
- protocol versions
- tool definitions

That boundary is clean in this codebase.

## 14. `get_frame` and `compare_frames`

These two MCP tools are worth understanding separately.

### `get_frame`

This is:

"Extract a single frame at timestamp T and return it."

It uses `extract_single_frame(...)`, reads the generated image bytes, base64-encodes them, and returns an MCP image block.

### `compare_frames`

This is:

"Extract frame A and frame B from the same video, compute perceptual difference, and return both images plus the score."

This is useful for:

- debugging screen transitions
- validating if two moments are actually different
- checking how much a UI changed

TS analogy:

```typescript
const frameA = await getFrame(video, tsA)
const frameB = await getFrame(video, tsB)
const score = changeScore(phash(frameA), phash(frameB))
return { frameA, frameB, score }
```

## 15. Error handling style

This repo uses `Result<_, String>` widely.

That is not the fanciest Rust error architecture, but it is pragmatic and coherent for a small tool.

Benefits:

- easy to read
- easy to bubble up
- simple to print in CLI/MCP contexts

Tradeoff:

- string errors are less structured than custom error enums

If the project grew more complex, you might eventually switch to:

- `thiserror`
- custom error types
- richer context propagation

But for the current size, the existing choice is reasonable.

## 16. Testing strategy

The tests are split conceptually into three kinds.

### Unit tests

Examples:

- pHash behavior
- Hamming distance
- crop parsing
- frame extraction helpers
- output serialization

These sit close to the modules they test.

### Integration tests

See [tests/integration\_test.rs](/Users/yomi/Documents/batcave/tomegane/tests/integration_test.rs).

These test the library and binary more end-to-end.

Examples:

- analyze returns reasonable frame counts
- base64 inclusion behavior
- threshold behavior
- CLI outputs valid JSON
- stream mode emits JSON lines

### MCP tests

There are also MCP-focused tests in [src/mcp/mod.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/mod.rs).

These verify:

- protocol handling
- tool registration
- fixture-based tool calls

This is a good spread. It tests:

- algorithmic pieces
- public API behavior
- protocol integration

## 17. Why the code uses external processes instead of native Rust video libraries

As a TS developer, you might ask:

"Why not just use a Rust video decoding crate directly?"

Short answer:

- `ffmpeg` is the industry standard
- it is already extremely capable
- wrapping it is cheaper and less risky

This is similar to how many Node tools shell out to:

- `git`
- `ffmpeg`
- `imagemagick`

instead of reimplementing those systems in JS.

That is often the correct product decision.

## 18. Performance characteristics

The main costs in this library are:

- invoking ffmpeg
- writing extracted frames to disk
- decoding images for hashing
- computing DCT-based pHash
- base64-encoding images if requested

### Likely bottlenecks

For long videos:

- I/O and ffmpeg extraction will matter a lot
- hashing all frames also matters

For MCP responses:

- base64 image size can dominate payload cost

### Current performance wins

- hashing is parallelized via Rayon
- ffmpeg handles extraction efficiently in native code
- crop can reduce noise and analysis scope

### Current performance limitations

- extracted frames are written to disk first
- streaming mode still depends on extracted files, not a pure in-memory pipeline
- `max_frames` + streaming is not supported

All of that is reasonable for v1 of a tool like this.

## 19. Design choices that are especially good

These are the parts of the implementation I think are strongest.

### Thin adapters over a reusable core

`main.rs` and `mcp/handlers.rs` do not own the actual product logic.
That keeps the library reusable.

### Small, focused modules

The repo does not mix:

- ffmpeg command construction
- hashing math
- JSON-RPC transport
- CLI parsing

into one giant file.

That makes the codebase much easier to maintain.

### Type-safe crop handling

`CropRect` is exactly the kind of small type that pays for itself.

### Explicit output schema

`Frame`, `AnalysisResult`, and `StreamEvent` make the external behavior clear and stable.

### Real perceptual hashing

Upgrading to DCT-based pHash was a meaningful quality improvement.

## 20. Places you could extend the library

If you wanted to evolve this project, here are realistic directions.

### A. Better streaming

Current CLI streaming is useful, but the MCP side is still request/response.

Possible future work:

- protocol extension for progressive MCP output
- partial responses or event notifications

### B. Smarter frame selection

Right now selection is purely perceptual difference-based.

Future options:

- scene-cut detection
- OCR-weighted differences
- UI-region heuristics
- motion-aware grouping

### C. Richer metadata

Each frame could eventually include:

- image dimensions
- crop metadata
- hash values
- labels derived from OCR or CV

### D. In-memory pipelines

Instead of writing all frames to disk first, you could explore:

- pipe-based frame extraction
- chunked processing
- streaming decode + hash

That would be more complex, but could improve performance.

### E. Better error types

If the codebase gets much larger, move away from raw `String` errors.

## 21. How I would explain this codebase in one sentence

`tomegane` is a Rust core pipeline that uses ffmpeg for extraction, perceptual hashing for frame selection, and thin CLI/MCP adapters to expose the result to humans and AI agents.

## 22. If you want to navigate the code in a sensible order

Read it in this sequence:

1. [README.md](/Users/yomi/Documents/batcave/tomegane/README.md)
2. [src/cli.rs](/Users/yomi/Documents/batcave/tomegane/src/cli.rs)
3. [src/main.rs](/Users/yomi/Documents/batcave/tomegane/src/main.rs)
4. [src/lib.rs](/Users/yomi/Documents/batcave/tomegane/src/lib.rs)
5. [src/extract/ffmpeg.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/ffmpeg.rs)
6. [src/extract/diff.rs](/Users/yomi/Documents/batcave/tomegane/src/extract/diff.rs)
7. [src/output/schema.rs](/Users/yomi/Documents/batcave/tomegane/src/output/schema.rs)
8. [src/mcp/handlers.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/handlers.rs)
9. [src/mcp/mod.rs](/Users/yomi/Documents/batcave/tomegane/src/mcp/mod.rs)
10. [tests/integration\_test.rs](/Users/yomi/Documents/batcave/tomegane/tests/integration_test.rs)

That order moves from:

- what users do
- to what the binary does
- to how the core pipeline works
- to how protocols wrap it
- to how it is tested

## 23. A final TS-to-Rust cheat sheet for this repo

- `struct` → object shape + actual runtime container
- `enum` → discriminated union
- `Option<T>` → `T | undefined`
- `Result<T, E>` → explicit success/error return
- `impl Default` → default config factory
- `&str` → borrowed string input
- `String` → owned string
- `Vec<T>` → `T[]`
- iterator chains → `map/filter/reduce`, but strongly typed and often zero-cost
- `par_iter()` → parallel map over CPU work
- `derive(Serialize)` → JSON-serializable type
- `match` → exhaustive branching with compiler help

## 24. What to keep in your head as the simplest mental model

If you only remember one simplified model, use this:

```plaintext
CLI/MCP input
  -> parse options
  -> call library
  -> ffmpeg extracts frames
  -> diff module scores visual change
  -> lib.rs chooses which frames matter
  -> output schema serializes result
  -> CLI prints JSON or MCP returns content blocks
```

That mental model is accurate enough to orient yourself anywhere in the codebase.

## 25. Suggested next reads

If you want to learn Rust through this repo specifically, the best next step is:

1. read `AnalyzeOptions`
2. trace `analyze(...)`
3. trace `extract_frames(...)`
4. trace `select_key_frames(...)`
5. read one integration test and map it back to the implementation

That will give you the fastest "I can actually follow this code" payoff.