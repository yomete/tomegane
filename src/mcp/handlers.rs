use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};
use std::path::Path;

use super::protocol::{ContentBlock, Tool, ToolResult};
use crate::extract::ffmpeg::{self, CropRect};

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
                    "crop": {
                        "type": "string",
                        "description": "Optional region-of-interest crop in x,y,w,h format"
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
    match name {
        "analyze_video" => handle_analyze_video(arguments),
        "get_frame" => handle_get_frame(arguments),
        "compare_frames" => handle_compare_frames(arguments),
        _ => ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("Unknown tool: {name}"),
            }],
            is_error: Some(true),
        },
    }
}

fn handle_analyze_video(args: &Value) -> ToolResult {
    let video_path = match args.get("video_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return error_result("Missing required parameter: video_path"),
    };

    let threshold = args
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.15);
    let max_frames = args
        .get("max_frames")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;
    let interval = args.get("interval").and_then(|v| v.as_f64()).unwrap_or(0.5);
    let crop = match parse_crop(args) {
        Ok(crop) => crop,
        Err(e) => return error_result(&e),
    };

    match crate::analyze(
        video_path,
        interval,
        None,
        "png",
        true, // always include base64 for MCP
        crop,
        Some(threshold),
        Some(max_frames),
    ) {
        Ok(result) => {
            let mut content: Vec<ContentBlock> = Vec::new();

            // Summary text
            content.push(ContentBlock::Text {
                text: format!(
                    "Analyzed: {}\nDuration: {:.1}s\nTotal frames extracted: {}\nKey frames selected: {} (threshold: {}, max: {})\n",
                    result.source,
                    result.duration_seconds,
                    result.total_frames_extracted,
                    result.frame_count,
                    threshold,
                    max_frames,
                ),
            });

            // Each key frame as an image + text annotation
            for frame in &result.key_frames {
                content.push(ContentBlock::Text {
                    text: format!(
                        "\n--- Frame {} at {:.1}s (change: {:.3}, {}) ---",
                        frame.index, frame.timestamp_seconds, frame.change_score, frame.description,
                    ),
                });

                if let Some(b64) = &frame.image_base64 {
                    content.push(ContentBlock::Image {
                        data: b64.clone(),
                        mime_type: "image/png".to_string(),
                    });
                }
            }

            ToolResult {
                content,
                is_error: None,
            }
        }
        Err(e) => error_result(&e),
    }
}

fn handle_get_frame(args: &Value) -> ToolResult {
    let video_path = match args.get("video_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return error_result("Missing required parameter: video_path"),
    };

    let timestamp = match args.get("timestamp_seconds").and_then(|v| v.as_f64()) {
        Some(t) => t,
        None => return error_result("Missing required parameter: timestamp_seconds"),
    };
    let crop = match parse_crop(args) {
        Ok(crop) => crop,
        Err(e) => return error_result(&e),
    };

    let video = Path::new(video_path);
    if !video.exists() {
        return error_result(&format!("Video file not found: {video_path}"));
    }

    let tmp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => return error_result(&format!("Failed to create temp dir: {e}")),
    };

    let output_path = tmp.path().join("frame.png");
    match ffmpeg::extract_single_frame(video, timestamp, &output_path, crop) {
        Ok(()) => match std::fs::read(&output_path) {
            Ok(data) => {
                let b64 = BASE64.encode(&data);
                ToolResult {
                    content: vec![
                        ContentBlock::Text {
                            text: format!("Frame at {timestamp:.1}s from {video_path}"),
                        },
                        ContentBlock::Image {
                            data: b64,
                            mime_type: "image/png".to_string(),
                        },
                    ],
                    is_error: None,
                }
            }
            Err(e) => error_result(&format!("Failed to read frame: {e}")),
        },
        Err(e) => error_result(&e),
    }
}

fn handle_compare_frames(args: &Value) -> ToolResult {
    let video_path = match args.get("video_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return error_result("Missing required parameter: video_path"),
    };

    let ts_a = match args.get("timestamp_a").and_then(|v| v.as_f64()) {
        Some(t) => t,
        None => return error_result("Missing required parameter: timestamp_a"),
    };

    let ts_b = match args.get("timestamp_b").and_then(|v| v.as_f64()) {
        Some(t) => t,
        None => return error_result("Missing required parameter: timestamp_b"),
    };
    let crop = match parse_crop(args) {
        Ok(crop) => crop,
        Err(e) => return error_result(&e),
    };

    let video = Path::new(video_path);
    if !video.exists() {
        return error_result(&format!("Video file not found: {video_path}"));
    }

    // Check duration
    let duration = match ffmpeg::get_duration(video) {
        Ok(d) => d,
        Err(e) => return error_result(&e),
    };

    if ts_a > duration || ts_b > duration {
        return error_result(&format!(
            "Timestamp out of range. Video duration: {duration:.1}s"
        ));
    }

    let tmp = match tempfile::tempdir() {
        Ok(t) => t,
        Err(e) => return error_result(&format!("Failed to create temp dir: {e}")),
    };

    // Extract both frames
    let frame_a = tmp.path().join("frame_a.png");
    let frame_b = tmp.path().join("frame_b.png");

    for (ts, path) in [(ts_a, &frame_a), (ts_b, &frame_b)] {
        if let Err(e) = ffmpeg::extract_single_frame(video, ts, path, crop) {
            return error_result(&format!("Failed to extract frame at t={ts}s: {e}"));
        }
    }

    // Compute similarity using perceptual hash
    let hash_a = match crate::extract::diff::phash(&frame_a) {
        Ok(h) => h,
        Err(e) => return error_result(&format!("Failed to hash frame A: {e}")),
    };
    let hash_b = match crate::extract::diff::phash(&frame_b) {
        Ok(h) => h,
        Err(e) => return error_result(&format!("Failed to hash frame B: {e}")),
    };

    let distance = crate::extract::diff::hamming_distance(hash_a, hash_b);
    let change_score = distance as f64 / 64.0;

    let data_a = std::fs::read(&frame_a).unwrap();
    let data_b = std::fs::read(&frame_b).unwrap();
    let b64_a = BASE64.encode(&data_a);
    let b64_b = BASE64.encode(&data_b);

    ToolResult {
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
                mime_type: "image/png".to_string(),
            },
            ContentBlock::Text {
                text: format!("Frame B ({ts_b:.1}s):"),
            },
            ContentBlock::Image {
                data: b64_b,
                mime_type: "image/png".to_string(),
            },
        ],
        is_error: None,
    }
}

fn parse_crop(args: &Value) -> Result<Option<CropRect>, String> {
    match args.get("crop").and_then(|v| v.as_str()) {
        Some(spec) => CropRect::parse(spec).map(Some),
        None => Ok(None),
    }
}

fn error_result(message: &str) -> ToolResult {
    ToolResult {
        content: vec![ContentBlock::Text {
            text: message.to_string(),
        }],
        is_error: Some(true),
    }
}
