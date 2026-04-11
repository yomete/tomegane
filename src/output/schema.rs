use serde::Serialize;

/// A single extracted frame from the video.
#[derive(Debug, Serialize)]
pub struct Frame {
    pub index: usize,
    pub timestamp_seconds: f64,
    pub image_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_base64: Option<String>,
    pub change_score: f64,
    pub description: String,
}

/// The top-level result of analyzing a video.
#[derive(Debug, Serialize)]
pub struct AnalysisResult {
    pub source: String,
    pub duration_seconds: f64,
    pub total_frames_extracted: usize,
    pub key_frames: Vec<Frame>,
    pub frame_count: usize,
    pub output_format: String,
}
