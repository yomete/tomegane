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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_ffmpeg_succeeds_when_installed() {
        assert!(check_ffmpeg().is_ok());
    }

    #[test]
    fn get_duration_returns_positive_value() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/test_video.mp4");
        let duration = get_duration(&fixture).unwrap();
        assert!(duration > 0.0, "Duration should be positive, got {duration}");
        assert!(
            (duration - 5.0).abs() < 0.5,
            "Expected ~5s duration, got {duration}"
        );
    }

    #[test]
    fn get_duration_fails_for_missing_file() {
        let result = get_duration(Path::new("/nonexistent/video.mp4"));
        assert!(result.is_err());
    }

    #[test]
    fn extract_frames_produces_files() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();

        let count = extract_frames(&fixture, tmp.path(), 1.0, "png").unwrap();
        assert!(count > 0, "Should extract at least one frame");
        assert_eq!(count, 5, "5-second video at 1fps should produce 5 frames");

        // Verify actual files exist
        let png_files: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "png"))
            .collect();
        assert_eq!(png_files.len(), 5);
    }

    #[test]
    fn extract_frames_respects_interval() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();

        let count = extract_frames(&fixture, tmp.path(), 2.0, "png").unwrap();
        // 5-second video at 0.5fps → expect 2-3 frames
        assert!(count >= 2 && count <= 3, "Expected 2-3 frames at 2s interval, got {count}");
    }

    #[test]
    fn extract_frames_supports_jpg() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();

        let count = extract_frames(&fixture, tmp.path(), 2.5, "jpg").unwrap();
        assert!(count > 0);

        let jpg_files: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "jpg"))
            .collect();
        assert_eq!(jpg_files.len(), count);
    }
}
