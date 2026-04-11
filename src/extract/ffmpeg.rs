use std::path::Path;
use std::process::Command;

/// Check that ffmpeg is available on the system.
pub fn check_ffmpeg() -> Result<(), String> {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|_| ())
        .map_err(|_| "ffmpeg not found. Please install ffmpeg: https://ffmpeg.org/download.html".to_string())
}

/// Get the duration of a video file in seconds.
pub fn get_duration(video_path: &Path) -> Result<f64, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(video_path)
        .output()
        .map_err(|e| format!("Failed to run ffprobe: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffprobe failed: {stderr}"));
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    duration_str
        .trim()
        .parse::<f64>()
        .map_err(|e| format!("Failed to parse duration '{duration_str}': {e}"))
}

/// Extract frames from a video at a given interval (in seconds).
/// Returns the number of frames extracted.
pub fn extract_frames(
    video_path: &Path,
    output_dir: &Path,
    interval: f64,
    format: &str,
) -> Result<usize, String> {
    let fps = 1.0 / interval;
    let output_pattern = output_dir.join(format!("frame_%04d.{format}"));

    let output = Command::new("ffmpeg")
        .args([
            "-i",
            video_path.to_str().ok_or("Invalid video path")?,
            "-vf",
            &format!("fps={fps}"),
            "-q:v",
            "2",
            output_pattern.to_str().ok_or("Invalid output path")?,
        ])
        .output()
        .map_err(|e| format!("Failed to run ffmpeg: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg extraction failed: {stderr}"));
    }

    // Count extracted frames
    let count = std::fs::read_dir(output_dir)
        .map_err(|e| format!("Failed to read output dir: {e}"))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == format)
        })
        .count();

    Ok(count)
}
