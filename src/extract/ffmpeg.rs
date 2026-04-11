use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let parts: Vec<&str> = spec.split(',').collect();
        if parts.len() != 4 {
            return Err(format!("Invalid crop '{spec}'. Expected format: x,y,w,h"));
        }

        let x = parts[0]
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("Invalid crop x value in '{spec}'"))?;
        let y = parts[1]
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("Invalid crop y value in '{spec}'"))?;
        let width = parts[2]
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("Invalid crop width value in '{spec}'"))?;
        let height = parts[3]
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("Invalid crop height value in '{spec}'"))?;

        if width == 0 || height == 0 {
            return Err(format!(
                "Invalid crop '{spec}'. Width and height must be greater than zero"
            ));
        }

        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    fn filter_expr(&self) -> String {
        format!("crop={}:{}:{}:{}", self.width, self.height, self.x, self.y)
    }
}

/// Check that ffmpeg is available on the system.
pub fn check_ffmpeg() -> Result<(), String> {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|_| ())
        .map_err(|_| {
            "ffmpeg not found. Please install ffmpeg: https://ffmpeg.org/download.html".to_string()
        })
}

/// Get the duration of a video file in seconds.
pub fn get_duration(video_path: &Path) -> Result<f64, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
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
    crop: Option<CropRect>,
) -> Result<usize, String> {
    let fps = 1.0 / interval;
    let output_pattern = output_dir.join(format!("frame_%04d.{format}"));
    let filter = build_video_filter(fps, crop);

    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            video_path.to_str().ok_or("Invalid video path")?,
            "-vf",
            &filter,
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
        .filter(|e| e.path().extension().is_some_and(|ext| ext == format))
        .count();

    Ok(count)
}

pub fn extract_single_frame(
    video_path: &Path,
    timestamp: f64,
    output_path: &Path,
    crop: Option<CropRect>,
) -> Result<(), String> {
    let mut command = Command::new("ffmpeg");
    command.args([
        "-y",
        "-ss",
        &format!("{timestamp}"),
        "-i",
        video_path.to_str().ok_or("Invalid video path")?,
        "-vframes",
        "1",
    ]);

    if let Some(crop) = crop {
        let filter = crop.filter_expr();
        command.args(["-vf", &filter]);
    }

    command.args([
        "-q:v",
        "2",
        output_path.to_str().ok_or("Invalid output path")?,
    ]);

    let output = command
        .output()
        .map_err(|e| format!("Failed to run ffmpeg: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg extraction failed: {stderr}"));
    }

    Ok(())
}

fn build_video_filter(fps: f64, crop: Option<CropRect>) -> String {
    let mut filters = Vec::new();
    if let Some(crop) = crop {
        filters.push(crop.filter_expr());
    }
    filters.push(format!("fps={fps}"));
    filters.join(",")
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
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let duration = get_duration(&fixture).unwrap();
        assert!(
            duration > 0.0,
            "Duration should be positive, got {duration}"
        );
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
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();

        let count = extract_frames(&fixture, tmp.path(), 1.0, "png", None).unwrap();
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
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();

        let count = extract_frames(&fixture, tmp.path(), 2.0, "png", None).unwrap();
        // 5-second video at 0.5fps → expect 2-3 frames
        assert!(
            (2..=3).contains(&count),
            "Expected 2-3 frames at 2s interval, got {count}"
        );
    }

    #[test]
    fn extract_frames_supports_jpg() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();

        let count = extract_frames(&fixture, tmp.path(), 2.5, "jpg", None).unwrap();
        assert!(count > 0);

        let jpg_files: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "jpg"))
            .collect();
        assert_eq!(jpg_files.len(), count);
    }

    #[test]
    fn parse_crop_rejects_invalid_specs() {
        assert!(CropRect::parse("1,2,3").is_err());
        assert!(CropRect::parse("1,2,0,4").is_err());
        assert!(CropRect::parse("a,2,3,4").is_err());
    }

    #[test]
    fn extract_single_frame_produces_file() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("frame.png");

        extract_single_frame(&fixture, 1.0, &output, None).unwrap();

        assert!(output.exists());
    }
}
