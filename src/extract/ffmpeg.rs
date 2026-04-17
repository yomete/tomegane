use std::path::Path;
use std::process::Command;

use tracing::debug;

use crate::ImageFormat;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    pub fn parse(spec: &str) -> Result<Self> {
        let parts: Vec<&str> = spec.split(',').collect();
        if parts.len() != 4 {
            return Err(Error::InvalidCrop(format!(
                "Invalid crop '{spec}'. Expected format: x,y,w,h"
            )));
        }

        let parse_u32 = |raw: &str, field: &str| -> Result<u32> {
            raw.trim()
                .parse::<u32>()
                .map_err(|_| Error::InvalidCrop(format!("Invalid crop {field} value in '{spec}'")))
        };

        let x = parse_u32(parts[0], "x")?;
        let y = parse_u32(parts[1], "y")?;
        let width = parse_u32(parts[2], "width")?;
        let height = parse_u32(parts[3], "height")?;

        if width == 0 || height == 0 {
            return Err(Error::InvalidCrop(format!(
                "Invalid crop '{spec}'. Width and height must be greater than zero"
            )));
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
pub fn check_ffmpeg() -> Result<()> {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|_| ())
        .map_err(|_| Error::FfmpegMissing)
}

/// Get the duration of a video file in seconds.
pub fn get_duration(video_path: &Path) -> Result<f64> {
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
        .map_err(|_| Error::FfmpegMissing)?;

    if !output.status.success() {
        return Err(Error::FfmpegFailed {
            command: "ffprobe",
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let trimmed = duration_str.trim();
    trimmed.parse::<f64>().map_err(|e| Error::Parse {
        what: "duration",
        input: trimmed.to_string(),
        source: e,
    })
}

/// Extract frames from a video at a given interval (in seconds).
/// Returns the number of frames extracted.
pub fn extract_frames(
    video_path: &Path,
    output_dir: &Path,
    interval: f64,
    format: ImageFormat,
    crop: Option<CropRect>,
) -> Result<usize> {
    let fps = 1.0 / interval;
    let extension = format.extension();
    let output_pattern = output_dir.join(format!("frame_%04d.{extension}"));
    let filter = build_video_filter(fps, crop);

    debug!(
        video = %video_path.display(),
        output_dir = %output_dir.display(),
        interval,
        format = %format,
        crop = ?crop,
        "extracting frames via ffmpeg",
    );

    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            video_path.to_str().ok_or_else(|| {
                Error::InvalidArgument("video path contains invalid UTF-8".to_string())
            })?,
            "-vf",
            &filter,
            "-q:v",
            "2",
            output_pattern.to_str().ok_or_else(|| {
                Error::InvalidArgument("output path contains invalid UTF-8".to_string())
            })?,
        ])
        .output()
        .map_err(|_| Error::FfmpegMissing)?;

    if !output.status.success() {
        return Err(Error::FfmpegFailed {
            command: "ffmpeg",
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let count = std::fs::read_dir(output_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == extension))
        .count();

    Ok(count)
}

pub fn extract_single_frame(
    video_path: &Path,
    timestamp: f64,
    output_path: &Path,
    crop: Option<CropRect>,
) -> Result<()> {
    let mut command = Command::new("ffmpeg");
    command.args([
        "-y",
        "-ss",
        &format!("{timestamp}"),
        "-i",
        video_path.to_str().ok_or_else(|| {
            Error::InvalidArgument("video path contains invalid UTF-8".to_string())
        })?,
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
        output_path.to_str().ok_or_else(|| {
            Error::InvalidArgument("output path contains invalid UTF-8".to_string())
        })?,
    ]);

    let output = command.output().map_err(|_| Error::FfmpegMissing)?;

    if !output.status.success() {
        return Err(Error::FfmpegFailed {
            command: "ffmpeg",
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
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

        let count = extract_frames(&fixture, tmp.path(), 1.0, ImageFormat::Png, None).unwrap();
        assert!(count > 0, "Should extract at least one frame");
        assert_eq!(count, 5, "5-second video at 1fps should produce 5 frames");

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

        let count = extract_frames(&fixture, tmp.path(), 2.0, ImageFormat::Png, None).unwrap();
        assert!(
            (2..=3).contains(&count),
            "Expected 2-3 frames at 2s interval, got {count}"
        );
    }

    #[test]
    fn extract_frames_supports_jpg() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();

        let count = extract_frames(&fixture, tmp.path(), 2.5, ImageFormat::Jpg, None).unwrap();
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
