use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use tomegane::error::Error;
use tomegane::{AnalyzeOptions, ImageFormat};

fn fixture_video() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/test_video.mp4")
        .to_string_lossy()
        .to_string()
}

fn options() -> AnalyzeOptions {
    AnalyzeOptions::default()
}

// ─── analyze() function tests ───

#[test]
fn analyze_returns_correct_frame_count() {
    let result = tomegane::analyze(fixture_video(), &options()).unwrap();
    assert_eq!(result.frame_count, 5);
    assert_eq!(result.key_frames.len(), 5);
    assert_eq!(result.output_format, "png");
}

#[test]
fn analyze_first_frame_is_initial_state() {
    let result = tomegane::analyze(fixture_video(), &options()).unwrap();
    let first = &result.key_frames[0];
    assert_eq!(first.index, 0);
    assert_eq!(first.timestamp_seconds, 0.0);
    assert_eq!(first.description, "initial_state");
    assert_eq!(first.change_score, 0.0);
}

#[test]
fn analyze_timestamps_are_sequential() {
    let result = tomegane::analyze(fixture_video(), &options()).unwrap();
    for (i, frame) in result.key_frames.iter().enumerate() {
        let expected = i as f64 * 1.0;
        assert!(
            (frame.timestamp_seconds - expected).abs() < f64::EPSILON,
            "Frame {i} timestamp: expected {expected}, got {}",
            frame.timestamp_seconds
        );
    }
}

#[test]
fn analyze_with_base64_includes_data() {
    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            include_base64: true,
            ..options()
        },
    )
    .unwrap();
    for frame in &result.key_frames {
        assert!(
            frame.image_base64.is_some(),
            "Frame {} should have base64 data",
            frame.index
        );
        let b64 = frame.image_base64.as_ref().unwrap();
        assert!(!b64.is_empty(), "base64 data should not be empty");
        assert!(
            b64.starts_with("iVBOR"),
            "PNG base64 should start with iVBOR, got {}",
            &b64[..20.min(b64.len())]
        );
    }
}

#[test]
fn analyze_without_base64_omits_data() {
    let result = tomegane::analyze(fixture_video(), &options()).unwrap();
    for frame in &result.key_frames {
        assert!(frame.image_base64.is_none());
    }
}

#[test]
fn analyze_with_output_dir_persists_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let dir: PathBuf = tmp.path().to_path_buf();
    let dir_str = dir.to_string_lossy().to_string();

    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            output_dir: Some(dir.clone()),
            ..options()
        },
    )
    .unwrap();

    for frame in &result.key_frames {
        assert!(
            frame.image_path.starts_with(&dir_str),
            "Frame path should be in output dir: {}",
            frame.image_path
        );
        assert!(
            Path::new(&frame.image_path).exists(),
            "Frame file should exist: {}",
            frame.image_path
        );
    }
}

#[test]
fn analyze_duration_is_reasonable() {
    let result = tomegane::analyze(fixture_video(), &options()).unwrap();
    assert!(
        (result.duration_seconds - 5.0).abs() < 0.5,
        "Expected ~5s duration, got {}",
        result.duration_seconds
    );
}

#[test]
fn analyze_source_matches_input() {
    let video = fixture_video();
    let result = tomegane::analyze(&video, &options()).unwrap();
    assert_eq!(result.source, video);
}

#[test]
fn analyze_rejects_missing_video() {
    let err = tomegane::analyze("/nonexistent/video.mp4", &options()).unwrap_err();
    assert!(matches!(err, Error::VideoNotFound(_)));
    assert_eq!(err.code(), "video_not_found");
}

#[test]
fn invalid_format_string_errors_out() {
    let result: Result<ImageFormat, _> = "bmp".parse();
    assert!(matches!(result, Err(Error::InvalidFormat(_))));
}

#[test]
fn analyze_with_smaller_interval_extracts_more_frames() {
    let result_1s = tomegane::analyze(fixture_video(), &options()).unwrap();
    let result_half = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            interval: 0.5,
            ..options()
        },
    )
    .unwrap();

    assert!(
        result_half.frame_count > result_1s.frame_count,
        "0.5s interval ({}) should produce more frames than 1s ({})",
        result_half.frame_count,
        result_1s.frame_count
    );
}

#[test]
fn analyze_with_jpg_format_writes_jpg_files() {
    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            format: ImageFormat::Jpg,
            ..options()
        },
    )
    .unwrap();

    assert_eq!(result.output_format, "jpg");
    for frame in &result.key_frames {
        assert!(
            frame.image_path.ends_with(".jpg"),
            "Expected .jpg extension, got {}",
            frame.image_path
        );
    }
}

#[test]
fn analyze_options_builder_sets_fields() {
    let built = AnalyzeOptions::builder()
        .interval(0.5)
        .format(ImageFormat::Jpg)
        .threshold(0.2)
        .max_frames(4)
        .include_base64(true)
        .build();
    assert_eq!(built.interval, 0.5);
    assert_eq!(built.format, ImageFormat::Jpg);
    assert_eq!(built.threshold, Some(0.2));
    assert_eq!(built.max_frames, Some(4));
    assert!(built.include_base64);
}

// ─── Smart frame selection tests ───

#[test]
fn analyze_with_threshold_reduces_frames() {
    let all = tomegane::analyze(fixture_video(), &options()).unwrap();
    let smart = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            threshold: Some(0.1),
            ..options()
        },
    )
    .unwrap();

    assert!(
        smart.frame_count <= all.frame_count,
        "Threshold should reduce frames: {} vs {}",
        smart.frame_count,
        all.frame_count
    );
    assert!(smart.frame_count >= 1);
    assert_eq!(smart.key_frames[0].description, "initial_state");
}

#[test]
fn analyze_with_high_threshold_keeps_only_first() {
    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            threshold: Some(1.0),
            ..options()
        },
    )
    .unwrap();
    assert_eq!(
        result.frame_count, 1,
        "Threshold 1.0 should keep only the first frame"
    );
    assert_eq!(result.key_frames[0].index, 0);
}

#[test]
fn analyze_with_zero_threshold_keeps_all_changed() {
    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            threshold: Some(0.0),
            ..options()
        },
    )
    .unwrap();
    assert_eq!(result.frame_count, 5);
}

#[test]
fn analyze_max_frames_caps_output() {
    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            max_frames: Some(3),
            ..options()
        },
    )
    .unwrap();
    assert!(
        result.frame_count <= 3,
        "max_frames=3 should cap output, got {}",
        result.frame_count
    );
}

#[test]
fn analyze_max_frames_with_threshold() {
    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            threshold: Some(0.0),
            max_frames: Some(2),
            ..options()
        },
    )
    .unwrap();
    assert!(
        result.frame_count <= 2,
        "max_frames=2 with threshold should cap output, got {}",
        result.frame_count
    );
    assert_eq!(result.key_frames[0].index, 0);
}

#[test]
fn analyze_threshold_produces_change_scores() {
    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            threshold: Some(0.0),
            ..options()
        },
    )
    .unwrap();

    assert_eq!(result.key_frames[0].change_score, 0.0);

    for frame in &result.key_frames[1..] {
        assert!(
            frame.change_score >= 0.0 && frame.change_score <= 1.0,
            "Change score should be 0.0-1.0, got {}",
            frame.change_score
        );
    }
}

#[test]
fn analyze_total_extracted_stays_same_with_threshold() {
    let all = tomegane::analyze(fixture_video(), &options()).unwrap();
    let smart = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            threshold: Some(0.5),
            ..options()
        },
    )
    .unwrap();

    assert_eq!(
        all.total_frames_extracted, smart.total_frames_extracted,
        "total_frames_extracted should be the same regardless of threshold"
    );
}

#[test]
fn analyze_performance_mode_populates_insights() {
    let result = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            interval: 0.5,
            analysis_mode: tomegane::analysis::AnalysisMode::Performance,
            ..options()
        },
    )
    .unwrap();

    assert_eq!(
        serde_json::to_value(&result).unwrap()["analysis_mode"],
        "performance"
    );

    let insights = result
        .performance_insights
        .as_ref()
        .expect("performance mode should populate insights");
    assert_eq!(
        insights.frame_deltas.len(),
        result.total_frames_extracted - 1
    );
    assert!(!insights.summary.is_empty());
}

#[test]
fn analyze_rejects_invalid_threshold() {
    let err = tomegane::analyze(
        fixture_video(),
        &AnalyzeOptions {
            threshold: Some(1.5),
            ..options()
        },
    )
    .unwrap_err();
    assert!(matches!(err, Error::InvalidThreshold(_)));
}

// ─── analyze_stream() coverage ───

#[test]
fn analyze_stream_invokes_callback_per_frame() {
    let frames = Arc::new(Mutex::new(Vec::new()));
    let sink = frames.clone();

    let result = tomegane::analyze_stream(fixture_video(), &options(), move |frame| {
        sink.lock().unwrap().push(frame.index);
        Ok(())
    })
    .unwrap();

    let collected = frames.lock().unwrap().clone();
    assert!(!collected.is_empty());
    assert_eq!(collected.len(), result.frame_count);
    let mut sorted = collected.clone();
    sorted.sort();
    assert_eq!(collected, sorted, "callback should fire in index order");
}

#[test]
fn analyze_stream_propagates_callback_error() {
    let err = tomegane::analyze_stream(fixture_video(), &options(), |_| {
        Err(Error::InvalidArgument("stop".into()))
    })
    .unwrap_err();
    assert!(matches!(err, Error::InvalidArgument(_)));
}

#[test]
fn analyze_stream_rejects_max_frames() {
    let err = tomegane::analyze_stream(
        fixture_video(),
        &AnalyzeOptions {
            max_frames: Some(1),
            ..options()
        },
        |_| Ok(()),
    )
    .unwrap_err();
    assert!(matches!(err, Error::StreamingUnsupportedWithMaxFrames));
}

// ─── CLI binary tests ───

fn run_cli(args: &[&str]) -> std::process::Output {
    let binary = env!("CARGO_BIN_EXE_tomegane");
    Command::new(binary)
        .args(args)
        .output()
        .expect("Failed to run tomegane binary")
}

#[test]
fn cli_help_exits_successfully() {
    let output = run_cli(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("remote-seeing eye"));
}

#[test]
fn cli_analyze_outputs_valid_json() {
    let output = run_cli(&["analyze", &fixture_video()]);
    assert!(output.status.success(), "CLI should exit 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");

    assert!(parsed["source"].is_string());
    assert!(parsed["key_frames"].is_array());
    assert!(parsed["frame_count"].is_number());
}

#[test]
fn cli_analyze_with_output_writes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let output_path = tmp.path().join("result.json");

    let output = run_cli(&[
        "analyze",
        &fixture_video(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    assert!(output_path.exists(), "Output file should be created");

    let contents = std::fs::read_to_string(&output_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(parsed["key_frames"].is_array());
}

#[test]
fn cli_missing_video_exits_with_io_code() {
    let output = run_cli(&["analyze", "/nonexistent/video.mp4"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn cli_invalid_threshold_exits_with_arg_code() {
    let output = run_cli(&["analyze", &fixture_video(), "--threshold", "2.0"]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn cli_stream_and_output_conflict_at_parse_time() {
    let output = run_cli(&[
        "analyze",
        &fixture_video(),
        "--stream",
        "--output",
        "/tmp/out.json",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "Expected clap conflict error, got stderr: {stderr}"
    );
}

#[test]
fn cli_stream_and_max_frames_conflict_at_parse_time() {
    let output = run_cli(&["analyze", &fixture_video(), "--stream", "--max-frames", "5"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "Expected clap conflict error, got stderr: {stderr}"
    );
}

#[test]
fn cli_no_args_shows_help() {
    let output = run_cli(&[]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage"));
}

#[test]
fn cli_analyze_with_threshold() {
    let output = run_cli(&["analyze", &fixture_video(), "--threshold", "0.1"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed["key_frames"].is_array());
    assert!(parsed["frame_count"].as_u64().unwrap() >= 1);
}

#[test]
fn cli_analyze_with_max_frames() {
    let output = run_cli(&["analyze", &fixture_video(), "--max-frames", "2"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed["frame_count"].as_u64().unwrap() <= 2);
}

#[test]
fn cli_analyze_with_crop() {
    let output = run_cli(&["analyze", &fixture_video(), "--crop", "0,0,16,16"]);
    assert!(output.status.success());
}

#[test]
fn cli_analyze_with_performance_mode() {
    let output = run_cli(&["analyze", &fixture_video(), "--mode", "performance"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["analysis_mode"], "performance");
    assert!(parsed["performance_insights"]["summary"].is_string());
}

#[test]
fn cli_analyze_streams_json_lines() {
    let output = run_cli(&[
        "analyze",
        &fixture_video(),
        "--stream",
        "--threshold",
        "0.1",
    ]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let first: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(first["type"], "started");

    let last: serde_json::Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert_eq!(last["type"], "completed");
}

#[test]
fn cli_setup_list_reports_status() {
    let output = run_cli(&["setup", "--list"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Claude Code"));
    assert!(stdout.contains("Cursor"));
}
