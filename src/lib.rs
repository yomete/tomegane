pub mod cli;
pub mod extract;
pub mod output;

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use extract::ffmpeg;
use output::schema::{AnalysisResult, Frame};

/// Analyze a video file: extract frames at the given interval and return structured results.
pub fn analyze(
    video_path: &str,
    interval: f64,
    output_dir: Option<&str>,
    format: &str,
    include_base64: bool,
) -> Result<AnalysisResult, String> {
    let video = Path::new(video_path);
    if !video.exists() {
        return Err(format!("Video file not found: {video_path}"));
    }

    let valid_formats = ["png", "jpg"];
    if !valid_formats.contains(&format) {
        return Err(format!("Unsupported format '{format}'. Use: png, jpg"));
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

    let mut frames: Vec<Frame> = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&frames_dir)
        .map_err(|e| format!("Failed to read frames dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == format))
        .collect();
    entries.sort();

    for (i, path) in entries.iter().enumerate() {
        let timestamp = i as f64 * interval;

        let b64 = if include_base64 {
            let data = std::fs::read(path)
                .map_err(|e| format!("Failed to read frame {}: {e}", path.display()))?;
            Some(BASE64.encode(&data))
        } else {
            None
        };

        frames.push(Frame {
            index: i,
            timestamp_seconds: timestamp,
            image_path: path.to_string_lossy().to_string(),
            image_base64: b64,
            change_score: 0.0,
            description: if i == 0 {
                "initial_state".to_string()
            } else {
                format!("frame_{i}")
            },
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
