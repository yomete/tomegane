pub mod cli;
pub mod extract;
pub mod mcp;
pub mod output;
pub mod setup;

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use extract::{diff, ffmpeg};
use output::schema::{AnalysisResult, Frame};

#[derive(Debug, Clone)]
pub struct AnalyzeOptions<'a> {
    pub interval: f64,
    pub output_dir: Option<&'a str>,
    pub format: &'a str,
    pub include_base64: bool,
    pub crop: Option<ffmpeg::CropRect>,
    pub threshold: Option<f64>,
    pub max_frames: Option<usize>,
}

impl Default for AnalyzeOptions<'_> {
    fn default() -> Self {
        Self {
            interval: 1.0,
            output_dir: None,
            format: "png",
            include_base64: false,
            crop: None,
            threshold: None,
            max_frames: None,
        }
    }
}

/// Analyze a video file: extract frames and return structured results.
///
/// If `threshold` is provided, uses perceptual hashing to select only
/// frames with meaningful visual changes. Otherwise returns all frames.
pub fn analyze(video_path: &str, options: &AnalyzeOptions<'_>) -> Result<AnalysisResult, String> {
    analyze_internal(video_path, options, None)
}

pub fn analyze_stream<F>(
    video_path: &str,
    options: &AnalyzeOptions<'_>,
    mut on_frame: F,
) -> Result<AnalysisResult, String>
where
    F: FnMut(&Frame) -> Result<(), String>,
{
    if options.max_frames.is_some() {
        return Err("Streaming mode does not support max_frames yet".to_string());
    }

    analyze_internal(video_path, options, Some(&mut on_frame))
}

type FrameCallback<'a> = dyn FnMut(&Frame) -> Result<(), String> + 'a;

fn analyze_internal(
    video_path: &str,
    options: &AnalyzeOptions<'_>,
    mut on_frame: Option<&mut FrameCallback<'_>>,
) -> Result<AnalysisResult, String> {
    let video = Path::new(video_path);
    if !video.exists() {
        return Err(format!("Video file not found: {video_path}"));
    }

    let valid_formats = ["png", "jpg"];
    if !valid_formats.contains(&options.format) {
        return Err(format!(
            "Unsupported format '{}'. Use: png, jpg",
            options.format
        ));
    }

    if let Some(t) = options.threshold
        && !(0.0..=1.0).contains(&t)
    {
        return Err(format!("Threshold must be between 0.0 and 1.0, got {t}"));
    }

    ffmpeg::check_ffmpeg()?;

    let duration = ffmpeg::get_duration(video)?;

    let temp_dir;
    let frames_dir: PathBuf = if let Some(dir) = options.output_dir {
        let p = PathBuf::from(dir);
        std::fs::create_dir_all(&p).map_err(|e| format!("Failed to create output dir: {e}"))?;
        p
    } else {
        temp_dir = tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {e}"))?;
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
        return Err("No frames were extracted. Is the video file valid?".to_string());
    }

    // Collect all frame paths, sorted
    let mut all_paths: Vec<PathBuf> = std::fs::read_dir(&frames_dir)
        .map_err(|e| format!("Failed to read frames dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == options.format))
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

    Ok(AnalysisResult {
        source: video_path.to_string(),
        duration_seconds: duration,
        total_frames_extracted: total_extracted,
        key_frames: frames,
        frame_count,
        output_format: options.format.to_string(),
    })
}

fn stream_selected_frames(
    all_paths: &[PathBuf],
    interval: f64,
    include_base64: bool,
    threshold: Option<f64>,
    frames: &mut Vec<Frame>,
    on_frame: &mut Option<&mut FrameCallback<'_>>,
) -> Result<(), String> {
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
) -> Result<(), String> {
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
) -> Result<Frame, String> {
    let timestamp = idx as f64 * interval;

    let b64 = if include_base64 {
        let data = std::fs::read(path)
            .map_err(|e| format!("Failed to read frame {}: {e}", path.display()))?;
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
