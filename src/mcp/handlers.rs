use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};

use super::protocol::{ContentBlock, Tool, ToolResult};
use crate::analysis::AnalysisMode;
use crate::error::{Error, Result};
use crate::extract::ffmpeg::{self, CropRect};
use crate::{AnalyzeOptions, ImageFormat};

pub fn tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "analyze_video".to_string(),
            description: "Extract key frames from a screen recording or video file. Returns frames as images with timestamps and change scores. Use this to understand what happened in a video.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "video_path": {
                        "type": "string",
                        "description": "Absolute path to the video file (mp4, mov, webm, mkv)"
                    },
                    "threshold": {
                        "type": "number",
                        "description": "Change threshold for smart frame selection (0.0-1.0). Lower values keep more frames. Default: 0.15",
                        "default": 0.15
                    },
                    "max_frames": {
                        "type": "integer",
                        "description": "Maximum number of frames to return. Default: 20",
                        "default": 20
                    },
                    "interval": {
                        "type": "number",
                        "description": "Frame extraction interval in seconds. Lower values capture more detail but take longer. Default: 0.5",
                        "default": 0.5
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["overview", "performance"],
                        "description": "Analysis mode. Use performance to detect likely jank windows and localized repaint activity. Default: overview",
                        "default": "overview"
                    },
                    "crop": {
                        "type": "string",
                        "description": "Optional region-of-interest crop in x,y,w,h format"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["png", "jpg"],
                        "description": "Image format for extracted frames. Default: png",
                        "default": "png"
                    },
                    "include_image_data": {
                        "type": "boolean",
                        "description": "Include inline image content blocks (base64). Set to false to keep the response small when you only need paths and metadata. Default: true",
                        "default": true
                    },
                    "output_dir": {
                        "type": "string",
                        "description": "Optional directory to persist extracted frames. When omitted, frames are written to a temp dir and paths remain valid only for the duration of this call."
                    }
                },
                "required": ["video_path"]
            }),
        },
        Tool {
            name: "get_frame".to_string(),
            description: "Extract a single frame from a video at a specific timestamp. Returns the frame as an image.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "video_path": {
                        "type": "string",
                        "description": "Absolute path to the video file"
                    },
                    "timestamp_seconds": {
                        "type": "number",
                        "description": "Timestamp in seconds to extract the frame from"
                    },
                    "crop": {
                        "type": "string",
                        "description": "Optional region-of-interest crop in x,y,w,h format"
                    }
                },
                "required": ["video_path", "timestamp_seconds"]
            }),
        },
        Tool {
            name: "compare_frames".to_string(),
            description: "Compare two frames from a video at different timestamps. Returns both frames side by side with a similarity score.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "video_path": {
                        "type": "string",
                        "description": "Absolute path to the video file"
                    },
                    "timestamp_a": {
                        "type": "number",
                        "description": "First timestamp in seconds"
                    },
                    "timestamp_b": {
                        "type": "number",
                        "description": "Second timestamp in seconds"
                    },
                    "crop": {
                        "type": "string",
                        "description": "Optional region-of-interest crop in x,y,w,h format"
                    }
                },
                "required": ["video_path", "timestamp_a", "timestamp_b"]
            }),
        },
    ]
}

pub fn handle_tool_call(name: &str, arguments: &Value) -> ToolResult {
    let result = match name {
        "analyze_video" => handle_analyze_video(arguments),
        "get_frame" => handle_get_frame(arguments),
        "compare_frames" => handle_compare_frames(arguments),
        _ => {
            return ToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("[tomegane error:unknown_tool] Unknown tool: {name}"),
                }],
                is_error: Some(true),
            };
        }
    };

    match result {
        Ok(tr) => tr,
        Err(e) => error_result(&e),
    }
}

fn handle_analyze_video(args: &Value) -> Result<ToolResult> {
    let video_path = require_str(args, "video_path")?;

    let threshold = args
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.15);
    let max_frames = args
        .get("max_frames")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;
    let interval = args.get("interval").and_then(|v| v.as_f64()).unwrap_or(0.5);
    let analysis_mode = parse_mode(args)?;
    let crop = parse_crop(args)?;
    let format = parse_format(args)?;
    let include_image_data = args
        .get("include_image_data")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let output_dir = args
        .get("output_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);

    let options = AnalyzeOptions {
        interval,
        output_dir,
        format,
        include_base64: include_image_data,
        crop,
        threshold: Some(threshold),
        max_frames: Some(max_frames),
        analysis_mode,
    };

    let result = crate::analyze(video_path, &options)?;

    let mut content: Vec<ContentBlock> = Vec::new();

    content.push(ContentBlock::Text {
        text: format!(
            "Analyzed: {}\nMode: {}\nDuration: {:.1}s\nTotal frames extracted: {}\nKey frames selected: {} (threshold: {}, max: {})\n",
            result.source,
            match result.analysis_mode {
                AnalysisMode::Overview => "overview",
                AnalysisMode::Performance => "performance",
            },
            result.duration_seconds,
            result.total_frames_extracted,
            result.frame_count,
            threshold,
            max_frames,
        ),
    });

    if let Some(insights) = &result.performance_insights {
        content.push(ContentBlock::Text {
            text: format!(
                "Performance summary: {}\nSuspicious windows: {} | Avg change: {:.3} | Peak change: {:.3}\n",
                insights.summary,
                insights.suspicious_windows.len(),
                insights.average_change_score,
                insights.peak_change_score,
            ),
        });
    }

    for frame in &result.key_frames {
        content.push(ContentBlock::Text {
            text: format!(
                "\n--- Frame {} at {:.1}s (change: {:.3}, {}) [{}] ---",
                frame.index,
                frame.timestamp_seconds,
                frame.change_score,
                frame.description,
                frame.image_path,
            ),
        });

        if let Some(b64) = &frame.image_base64 {
            content.push(ContentBlock::Image {
                data: b64.clone(),
                mime_type: format.mime_type().to_string(),
            });
        }
    }

    Ok(ToolResult {
        content,
        is_error: None,
    })
}

fn handle_get_frame(args: &Value) -> Result<ToolResult> {
    let video_path = require_str(args, "video_path")?;
    let timestamp = args
        .get("timestamp_seconds")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| {
            Error::InvalidArgument("Missing required parameter: timestamp_seconds".into())
        })?;
    let crop = parse_crop(args)?;

    let video = Path::new(video_path);
    if !video.exists() {
        return Err(Error::VideoNotFound(video.to_path_buf()));
    }

    let duration = ffmpeg::get_duration(video)?;
    if timestamp < 0.0 || timestamp > duration {
        return Err(Error::TimestampOutOfRange {
            timestamp,
            duration,
        });
    }

    let tmp = tempfile::tempdir()?;
    let output_path = tmp.path().join("frame.png");

    ffmpeg::extract_single_frame(video, timestamp, &output_path, crop)?;

    let data = std::fs::read(&output_path)?;
    let b64 = BASE64.encode(&data);

    Ok(ToolResult {
        content: vec![
            ContentBlock::Text {
                text: format!("Frame at {timestamp:.1}s from {video_path}"),
            },
            ContentBlock::Image {
                data: b64,
                mime_type: ImageFormat::Png.mime_type().to_string(),
            },
        ],
        is_error: None,
    })
}

fn handle_compare_frames(args: &Value) -> Result<ToolResult> {
    let video_path = require_str(args, "video_path")?;
    let ts_a = args
        .get("timestamp_a")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| Error::InvalidArgument("Missing required parameter: timestamp_a".into()))?;
    let ts_b = args
        .get("timestamp_b")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| Error::InvalidArgument("Missing required parameter: timestamp_b".into()))?;
    let crop = parse_crop(args)?;

    let video = Path::new(video_path);
    if !video.exists() {
        return Err(Error::VideoNotFound(video.to_path_buf()));
    }

    let duration = ffmpeg::get_duration(video)?;
    for ts in [ts_a, ts_b] {
        if ts < 0.0 || ts > duration {
            return Err(Error::TimestampOutOfRange {
                timestamp: ts,
                duration,
            });
        }
    }

    let tmp = tempfile::tempdir()?;
    let frame_a = tmp.path().join("frame_a.png");
    let frame_b = tmp.path().join("frame_b.png");

    for (ts, path) in [(ts_a, &frame_a), (ts_b, &frame_b)] {
        ffmpeg::extract_single_frame(video, ts, path, crop)?;
    }

    let hash_a = crate::extract::diff::phash(&frame_a)?;
    let hash_b = crate::extract::diff::phash(&frame_b)?;

    let distance = crate::extract::diff::hamming_distance(hash_a, hash_b);
    let change_score = distance as f64 / 64.0;

    let data_a = std::fs::read(&frame_a)?;
    let data_b = std::fs::read(&frame_b)?;
    let b64_a = BASE64.encode(&data_a);
    let b64_b = BASE64.encode(&data_b);

    Ok(ToolResult {
        content: vec![
            ContentBlock::Text {
                text: format!(
                    "Comparing frames at {ts_a:.1}s and {ts_b:.1}s from {video_path}\nChange score: {change_score:.3} (0.0 = identical, 1.0 = completely different)\nHamming distance: {distance}/64",
                ),
            },
            ContentBlock::Text {
                text: format!("Frame A ({ts_a:.1}s):"),
            },
            ContentBlock::Image {
                data: b64_a,
                mime_type: ImageFormat::Png.mime_type().to_string(),
            },
            ContentBlock::Text {
                text: format!("Frame B ({ts_b:.1}s):"),
            },
            ContentBlock::Image {
                data: b64_b,
                mime_type: ImageFormat::Png.mime_type().to_string(),
            },
        ],
        is_error: None,
    })
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidArgument(format!("Missing required parameter: {key}")))
}

fn parse_crop(args: &Value) -> Result<Option<CropRect>> {
    match args.get("crop").and_then(|v| v.as_str()) {
        Some(spec) => CropRect::parse(spec).map(Some),
        None => Ok(None),
    }
}

fn parse_mode(args: &Value) -> Result<AnalysisMode> {
    match args.get("mode").and_then(|v| v.as_str()) {
        None | Some("overview") => Ok(AnalysisMode::Overview),
        Some("performance") => Ok(AnalysisMode::Performance),
        Some(other) => Err(Error::InvalidArgument(format!(
            "Invalid mode '{other}'. Expected one of: overview, performance"
        ))),
    }
}

fn parse_format(args: &Value) -> Result<ImageFormat> {
    match args.get("format").and_then(|v| v.as_str()) {
        None => Ok(ImageFormat::Png),
        Some(s) => s.parse(),
    }
}

fn error_result(error: &Error) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: format!("[tomegane error:{}] {}", error.code(), error),
        }],
        is_error: Some(true),
    }
}
