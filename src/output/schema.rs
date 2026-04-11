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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_serializes_without_base64_when_none() {
        let frame = Frame {
            index: 0,
            timestamp_seconds: 0.0,
            image_path: "/tmp/frame_0001.png".to_string(),
            image_base64: None,
            change_score: 0.0,
            description: "initial_state".to_string(),
        };

        let json = serde_json::to_value(&frame).unwrap();
        assert!(!json.as_object().unwrap().contains_key("image_base64"));
        assert_eq!(json["index"], 0);
        assert_eq!(json["description"], "initial_state");
    }

    #[test]
    fn frame_serializes_with_base64_when_present() {
        let frame = Frame {
            index: 1,
            timestamp_seconds: 1.0,
            image_path: "/tmp/frame_0002.png".to_string(),
            image_base64: Some("aGVsbG8=".to_string()),
            change_score: 0.5,
            description: "frame_1".to_string(),
        };

        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["image_base64"], "aGVsbG8=");
        assert_eq!(json["change_score"], 0.5);
    }

    #[test]
    fn analysis_result_serializes_correctly() {
        let result = AnalysisResult {
            source: "test.mp4".to_string(),
            duration_seconds: 5.0,
            total_frames_extracted: 5,
            key_frames: vec![
                Frame {
                    index: 0,
                    timestamp_seconds: 0.0,
                    image_path: "/tmp/f1.png".to_string(),
                    image_base64: None,
                    change_score: 0.0,
                    description: "initial_state".to_string(),
                },
            ],
            frame_count: 1,
            output_format: "png".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["source"], "test.mp4");
        assert_eq!(parsed["duration_seconds"], 5.0);
        assert_eq!(parsed["total_frames_extracted"], 5);
        assert_eq!(parsed["frame_count"], 1);
        assert_eq!(parsed["output_format"], "png");
        assert_eq!(parsed["key_frames"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn analysis_result_with_empty_frames() {
        let result = AnalysisResult {
            source: "empty.mp4".to_string(),
            duration_seconds: 0.0,
            total_frames_extracted: 0,
            key_frames: vec![],
            frame_count: 0,
            output_format: "png".to_string(),
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["key_frames"].as_array().unwrap().len(), 0);
        assert_eq!(json["frame_count"], 0);
    }
}
