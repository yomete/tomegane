//! # tomegane
//!
//! Extract meaningful frames from screen recordings for AI agents.
//!
//! tomegane shells out to `ffmpeg` to pull frames at a configured interval,
//! then uses a DCT-based perceptual hash to keep only the frames where the
//! screen actually changed. It exposes:
//!
//! - a Rust library (this crate) for programmatic use,
//! - a CLI binary (`tomegane`) for scripting, and
//! - an MCP server (`tomegane mcp`) so AI clients can use it as a tool.
//!
//! ## Quick example
//!
//! ```no_run
//! use tomegane::{analyze, AnalyzeOptions};
//!
//! let options = AnalyzeOptions::builder()
//!     .interval(0.5)
//!     .threshold(0.15)
//!     .max_frames(10)
//!     .build();
//!
//! let result = analyze("recording.mov", &options).unwrap();
//! for frame in &result.key_frames {
//!     println!("{:>5.1}s  {}", frame.timestamp_seconds, frame.image_path);
//! }
//! ```

pub mod analysis;
pub mod cli;
pub mod error;
pub mod extract;
pub mod mcp;
pub mod output;
pub mod setup;

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use analysis::{AnalysisMode, inspect_performance};
pub use error::{Error, Result};
use extract::{diff, ffmpeg};
use output::schema::{AnalysisResult, Frame};

/// Image format produced when extracting frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageFormat {
    #[default]
    Png,
    Jpg,
}

impl ImageFormat {
    /// File extension used by ffmpeg and on-disk frame files.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpg => "jpg",
        }
    }

    /// MIME type for HTTP / MCP image content blocks.
    pub fn mime_type(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpg => "image/jpeg",
        }
    }
}

impl std::str::FromStr for ImageFormat {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "png" => Ok(ImageFormat::Png),
            "jpg" | "jpeg" => Ok(ImageFormat::Jpg),
            other => Err(Error::InvalidFormat(other.to_string())),
        }
    }
}

impl std::fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.extension())
    }
}

/// Options controlling an [`analyze`] or [`analyze_stream`] call.
///
/// Construct with [`AnalyzeOptions::default`] or [`AnalyzeOptions::builder`].
#[derive(Debug, Clone)]
pub struct AnalyzeOptions {
    /// Frame extraction interval in seconds.
    pub interval: f64,
    /// Where to write the extracted frames. `None` uses a temporary directory
    /// that is cleaned up when the call returns.
    pub output_dir: Option<PathBuf>,
    /// Output image format.
    pub format: ImageFormat,
    /// When `true`, each returned [`Frame`] carries a base64-encoded copy of
    /// its image bytes.
    pub include_base64: bool,
    /// Optional crop applied before extraction.
    pub crop: Option<ffmpeg::CropRect>,
    /// When set, only keep frames whose perceptual change exceeds this
    /// threshold (0.0 — 1.0).
    pub threshold: Option<f64>,
    /// Optional cap on the number of frames returned.
    pub max_frames: Option<usize>,
    /// Analysis flavour: [`AnalysisMode::Overview`] or
    /// [`AnalysisMode::Performance`].
    pub analysis_mode: AnalysisMode,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            interval: 1.0,
            output_dir: None,
            format: ImageFormat::Png,
            include_base64: false,
            crop: None,
            threshold: None,
            max_frames: None,
            analysis_mode: AnalysisMode::Overview,
        }
    }
}

impl AnalyzeOptions {
    /// Start building an [`AnalyzeOptions`] with fluent setters.
    pub fn builder() -> AnalyzeOptionsBuilder {
        AnalyzeOptionsBuilder::default()
    }
}

/// Fluent builder for [`AnalyzeOptions`].
#[derive(Debug, Default, Clone)]
pub struct AnalyzeOptionsBuilder {
    inner: AnalyzeOptions,
}

impl AnalyzeOptionsBuilder {
    pub fn interval(mut self, interval: f64) -> Self {
        self.inner.interval = interval;
        self
    }

    pub fn output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.inner.output_dir = Some(dir.into());
        self
    }

    pub fn format(mut self, format: ImageFormat) -> Self {
        self.inner.format = format;
        self
    }

    pub fn include_base64(mut self, include: bool) -> Self {
        self.inner.include_base64 = include;
        self
    }

    pub fn crop(mut self, crop: ffmpeg::CropRect) -> Self {
        self.inner.crop = Some(crop);
        self
    }

    pub fn threshold(mut self, threshold: f64) -> Self {
        self.inner.threshold = Some(threshold);
        self
    }

    pub fn max_frames(mut self, max: usize) -> Self {
        self.inner.max_frames = Some(max);
        self
    }

    pub fn analysis_mode(mut self, mode: AnalysisMode) -> Self {
        self.inner.analysis_mode = mode;
        self
    }

    pub fn build(self) -> AnalyzeOptions {
        self.inner
    }
}

/// Analyze a video file: extract frames and return structured results.
///
/// If `options.threshold` is `Some`, only frames whose perceptual change
/// exceeds it are returned; otherwise every extracted frame is included.
pub fn analyze(video_path: impl AsRef<Path>, options: &AnalyzeOptions) -> Result<AnalysisResult> {
    analyze_internal(video_path.as_ref(), options, None)
}

/// Analyze a video file and invoke `on_frame` for each selected frame as it
/// becomes available. Returns the complete [`AnalysisResult`] once every
/// frame has been processed.
///
/// Streaming mode does not yet support `options.max_frames`; set it to `None`
/// or use [`analyze`] instead.
pub fn analyze_stream<F>(
    video_path: impl AsRef<Path>,
    options: &AnalyzeOptions,
    mut on_frame: F,
) -> Result<AnalysisResult>
where
    F: FnMut(&Frame) -> Result<()>,
{
    if options.max_frames.is_some() {
        return Err(Error::StreamingUnsupportedWithMaxFrames);
    }

    analyze_internal(video_path.as_ref(), options, Some(&mut on_frame))
}

type FrameCallback<'a> = dyn FnMut(&Frame) -> Result<()> + 'a;

fn analyze_internal(
    video: &Path,
    options: &AnalyzeOptions,
    mut on_frame: Option<&mut FrameCallback<'_>>,
) -> Result<AnalysisResult> {
    if !video.exists() {
        return Err(Error::VideoNotFound(video.to_path_buf()));
    }

    if let Some(t) = options.threshold
        && !(0.0..=1.0).contains(&t)
    {
        return Err(Error::InvalidThreshold(t));
    }

    ffmpeg::check_ffmpeg()?;

    let duration = ffmpeg::get_duration(video)?;

    let temp_dir;
    let frames_dir: PathBuf = if let Some(dir) = options.output_dir.as_deref() {
        std::fs::create_dir_all(dir)?;
        dir.to_path_buf()
    } else {
        temp_dir = tempfile::tempdir()?;
        temp_dir.path().to_path_buf()
    };

    let total_extracted = ffmpeg::extract_frames(
        video,
        &frames_dir,
        options.interval,
        options.format,
        options.crop,
    )?;

    if total_extracted == 0 {
        return Err(Error::NoFramesExtracted);
    }

    let extension = options.format.extension();
    let mut all_paths: Vec<PathBuf> = std::fs::read_dir(&frames_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == extension))
        .collect();
    all_paths.sort();

    let mut frames: Vec<Frame> = Vec::new();

    if on_frame.is_some() && options.max_frames.is_none() {
        stream_selected_frames(
            &all_paths,
            options.interval,
            options.include_base64,
            options.threshold,
            &mut frames,
            &mut on_frame,
        )?;
    } else {
        let selected: Vec<(usize, f64)> = if let Some(t) = options.threshold {
            diff::select_key_frames(&all_paths, t, options.max_frames)?
        } else {
            let mut all: Vec<(usize, f64)> = (0..all_paths.len()).map(|i| (i, 0.0)).collect();
            if let Some(max) = options.max_frames
                && all.len() > max
            {
                let step = all.len() as f64 / max as f64;
                all = (0..max)
                    .map(|i| {
                        let idx = (i as f64 * step) as usize;
                        (idx, 0.0)
                    })
                    .collect();
            }
            all
        };

        for &(idx, score) in &selected {
            let frame = build_frame(
                &all_paths[idx],
                idx,
                options.interval,
                options.include_base64,
                score,
            )?;
            if let Some(callback) = on_frame.as_deref_mut() {
                callback(&frame)?;
            }
            frames.push(frame);
        }
    }

    let frame_count = frames.len();
    let performance_insights = match options.analysis_mode {
        AnalysisMode::Overview => None,
        AnalysisMode::Performance => Some(inspect_performance(&all_paths, options.interval)?),
    };

    Ok(AnalysisResult {
        source: video.to_string_lossy().to_string(),
        analysis_mode: options.analysis_mode,
        duration_seconds: duration,
        total_frames_extracted: total_extracted,
        key_frames: frames,
        frame_count,
        output_format: options.format.extension().to_string(),
        performance_insights,
    })
}

fn stream_selected_frames(
    all_paths: &[PathBuf],
    interval: f64,
    include_base64: bool,
    threshold: Option<f64>,
    frames: &mut Vec<Frame>,
    on_frame: &mut Option<&mut FrameCallback<'_>>,
) -> Result<()> {
    if all_paths.is_empty() {
        return Ok(());
    }

    if let Some(threshold) = threshold {
        let first_hash = diff::phash(&all_paths[0])?;
        let first_frame = build_frame(&all_paths[0], 0, interval, include_base64, 0.0)?;
        emit_frame(frames, on_frame, first_frame)?;

        let mut last_hash = first_hash;
        for (idx, path) in all_paths.iter().enumerate().skip(1) {
            let current_hash = diff::phash(path)?;
            let score = diff::change_score(last_hash, current_hash);
            if score >= threshold {
                let frame = build_frame(path, idx, interval, include_base64, score)?;
                emit_frame(frames, on_frame, frame)?;
                last_hash = current_hash;
            }
        }
    } else {
        for (idx, path) in all_paths.iter().enumerate() {
            let frame = build_frame(path, idx, interval, include_base64, 0.0)?;
            emit_frame(frames, on_frame, frame)?;
        }
    }

    Ok(())
}

fn emit_frame(
    frames: &mut Vec<Frame>,
    on_frame: &mut Option<&mut FrameCallback<'_>>,
    frame: Frame,
) -> Result<()> {
    if let Some(callback) = on_frame.as_deref_mut() {
        callback(&frame)?;
    }
    frames.push(frame);
    Ok(())
}

fn build_frame(
    path: &Path,
    idx: usize,
    interval: f64,
    include_base64: bool,
    score: f64,
) -> Result<Frame> {
    let timestamp = idx as f64 * interval;

    let b64 = if include_base64 {
        let data = std::fs::read(path)?;
        Some(BASE64.encode(&data))
    } else {
        None
    };

    let description = if idx == 0 {
        "initial_state".to_string()
    } else if score >= 0.5 {
        "major_change".to_string()
    } else if score >= 0.2 {
        "moderate_change".to_string()
    } else if score > 0.0 {
        "minor_change".to_string()
    } else {
        format!("frame_{idx}")
    };

    Ok(Frame {
        index: idx,
        timestamp_seconds: timestamp,
        image_path: path.to_string_lossy().to_string(),
        image_base64: b64,
        change_score: score,
        description,
    })
}
