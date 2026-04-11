pub mod cli;
pub mod extract;
pub mod mcp;
pub mod output;

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use extract::{diff, ffmpeg};
use output::schema::{AnalysisResult, Frame};

/// Analyze a video file: extract frames and return structured results.
///
/// If `threshold` is provided, uses perceptual hashing to select only
/// frames with meaningful visual changes. Otherwise returns all frames.
pub fn analyze(
    video_path: &str,
    interval: f64,
    output_dir: Option<&str>,
    format: &str,
    include_base64: bool,
    threshold: Option<f64>,
    max_frames: Option<usize>,
) -> Result<AnalysisResult, String> {
    let video = Path::new(video_path);
    if !video.exists() {
        return Err(format!("Video file not found: {video_path}"));
    }

    let valid_formats = ["png", "jpg"];
    if !valid_formats.contains(&format) {
        return Err(format!("Unsupported format '{format}'. Use: png, jpg"));
    }

    if let Some(t) = threshold {
        if !(0.0..=1.0).contains(&t) {
            return Err(format!("Threshold must be between 0.0 and 1.0, got {t}"));
        }
    }

    ffmpeg::check_ffmpeg()?;

    let duration = ffmpeg::get_duration(video)?;

    let temp_dir;
    let frames_dir: PathBuf = if let Some(dir) = output_dir {
        let p = PathBuf::from(dir);
        std::fs::create_dir_all(&p)
            .map_err(|e| format!("Failed to create output dir: {e}"))?;
        p
    } else {
        temp_dir = tempfile::tempdir()
            .map_err(|e| format!("Failed to create temp dir: {e}"))?;
        temp_dir.path().to_path_buf()
    };

    let total_extracted = ffmpeg::extract_frames(video, &frames_dir, interval, format)?;

    if total_extracted == 0 {
        return Err("No frames were extracted. Is the video file valid?".to_string());
    }

    // Collect all frame paths, sorted
    let mut all_paths: Vec<PathBuf> = std::fs::read_dir(&frames_dir)
        .map_err(|e| format!("Failed to read frames dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == format))
        .collect();
    all_paths.sort();

    // Determine which frames to include
    let selected: Vec<(usize, f64)> = if let Some(t) = threshold {
        diff::select_key_frames(&all_paths, t, max_frames)?
    } else {
        // No threshold — include all frames (but still respect max_frames)
        let mut all: Vec<(usize, f64)> = (0..all_paths.len()).map(|i| (i, 0.0)).collect();
        if let Some(max) = max_frames {
            if all.len() > max {
                // Evenly sample frames
                let step = all.len() as f64 / max as f64;
                all = (0..max)
                    .map(|i| {
                        let idx = (i as f64 * step) as usize;
                        (idx, 0.0)
                    })
                    .collect();
            }
        }
        all
    };

    // Build output frames from selection
    let mut frames: Vec<Frame> = Vec::new();
    for &(idx, score) in &selected {
        let path = &all_paths[idx];
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

        frames.push(Frame {
            index: idx,
            timestamp_seconds: timestamp,
            image_path: path.to_string_lossy().to_string(),
            image_base64: b64,
            change_score: score,
            description,
        });
    }

    let frame_count = frames.len();

    Ok(AnalysisResult {
        source: video_path.to_string(),
        duration_seconds: duration,
        total_frames_extracted: total_extracted,
        key_frames: frames,
        frame_count,
        output_format: format.to_string(),
    })
}
