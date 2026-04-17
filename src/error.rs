use std::io;
use std::path::PathBuf;

/// A structured error type for the tomegane library.
///
/// MCP handlers, the CLI, and library consumers can each branch on the
/// variant rather than parsing a string message. Use [`Error::code`] to get a
/// stable string identifier (`"ffmpeg_not_found"`, `"video_not_found"`, …)
/// suitable for surfacing to non-Rust callers.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ffmpeg not found. Please install ffmpeg: https://ffmpeg.org/download.html")]
    FfmpegMissing,

    #[error("ffmpeg {command} failed: {stderr}")]
    FfmpegFailed {
        command: &'static str,
        stderr: String,
    },

    #[error("Video file not found: {0}")]
    VideoNotFound(PathBuf),

    #[error("Unsupported image format '{0}'. Use: png, jpg")]
    InvalidFormat(String),

    #[error("Threshold must be between 0.0 and 1.0, got {0}")]
    InvalidThreshold(f64),

    #[error("{0}")]
    InvalidCrop(String),

    #[error("Timestamp {timestamp:.3}s is out of range; video duration is {duration:.3}s")]
    TimestampOutOfRange { timestamp: f64, duration: f64 },

    #[error("No frames were extracted. Is the video file valid?")]
    NoFramesExtracted,

    #[error("Streaming mode does not support max_frames yet")]
    StreamingUnsupportedWithMaxFrames,

    #[error("Streaming mode cannot be combined with --output")]
    StreamingUnsupportedWithOutput,

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Failed to parse {what} '{input}': {source}")]
    Parse {
        what: &'static str,
        input: String,
        #[source]
        source: std::num::ParseFloatError,
    },

    #[error("Image decode failed for {path}: {source}")]
    ImageDecode {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
}

impl Error {
    /// A stable short identifier for this error, suitable for machine-readable
    /// surfaces (MCP `error_code`, CLI log tags).
    pub fn code(&self) -> &'static str {
        match self {
            Error::FfmpegMissing => "ffmpeg_not_found",
            Error::FfmpegFailed { .. } => "ffmpeg_failed",
            Error::VideoNotFound(_) => "video_not_found",
            Error::InvalidFormat(_) => "invalid_format",
            Error::InvalidThreshold(_) => "invalid_threshold",
            Error::InvalidCrop(_) => "invalid_crop",
            Error::TimestampOutOfRange { .. } => "timestamp_out_of_range",
            Error::NoFramesExtracted => "no_frames_extracted",
            Error::StreamingUnsupportedWithMaxFrames => "streaming_unsupported_with_max_frames",
            Error::StreamingUnsupportedWithOutput => "streaming_unsupported_with_output",
            Error::InvalidArgument(_) => "invalid_argument",
            Error::Parse { .. } => "parse_error",
            Error::ImageDecode { .. } => "image_decode_error",
            Error::Io(_) => "io_error",
            Error::Serde(_) => "serialization_error",
        }
    }
}

/// The library-wide `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;
