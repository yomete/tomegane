use std::time::Instant;

use clap::Parser;

use tomegane::AnalyzeOptions;
use tomegane::analysis::AnalysisMode;
use tomegane::cli::{Cli, Commands};
use tomegane::extract::ffmpeg::CropRect;
use tomegane::output::schema::StreamEvent;
use tomegane::setup;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Analyze {
            video_path,
            interval,
            output_dir,
            format,
            base64,
            crop,
            threshold,
            max_frames,
            mode,
            output,
            stream,
            verbose,
        } => {
            if verbose {
                eprintln!("tomegane v{}", env!("CARGO_PKG_VERSION"));
                eprintln!("Video: {video_path}");
                eprintln!("Interval: {interval}s | Format: {format}");
                if let Some(ref crop) = crop {
                    eprintln!("Crop: {crop}");
                }
                if let Some(t) = threshold {
                    eprintln!("Threshold: {t}");
                }
                if let Some(m) = max_frames {
                    eprintln!("Max frames: {m}");
                }
                eprintln!(
                    "Mode: {}",
                    match mode {
                        AnalysisMode::Overview => "overview",
                        AnalysisMode::Performance => "performance",
                    }
                );
            }

            let crop = match crop {
                Some(spec) => match CropRect::parse(&spec) {
                    Ok(crop) => Some(crop),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                },
                None => None,
            };

            let start = Instant::now();
            let options = AnalyzeOptions {
                interval,
                output_dir: output_dir.as_deref(),
                format: &format,
                include_base64: base64,
                crop,
                threshold,
                max_frames,
                analysis_mode: mode,
            };

            let result = if stream {
                if output.is_some() {
                    eprintln!("Error: --stream cannot be combined with --output");
                    std::process::exit(1);
                }

                (|| -> Result<_, String> {
                    let emit_event = |event: &StreamEvent| -> Result<(), String> {
                        let json = serde_json::to_string(event)
                            .map_err(|e| format!("Failed to serialize stream event: {e}"))?;
                        println!("{json}");
                        Ok(())
                    };

                    let summary = tomegane::analyze(
                        &video_path,
                        &AnalyzeOptions {
                            include_base64: false,
                            threshold: None,
                            max_frames: Some(1),
                            analysis_mode: AnalysisMode::Overview,
                            ..options.clone()
                        },
                    )?;

                    emit_event(&StreamEvent::Started {
                        source: summary.source.clone(),
                        duration_seconds: summary.duration_seconds,
                        total_frames_extracted: summary.total_frames_extracted,
                        output_format: summary.output_format.clone(),
                    })?;

                    let analysis = tomegane::analyze_stream(&video_path, &options, |frame| {
                        emit_event(&StreamEvent::Frame {
                            frame: frame.clone(),
                        })
                    })?;

                    emit_event(&StreamEvent::Completed {
                        result: analysis.clone(),
                    })?;

                    Ok(analysis)
                })()
            } else {
                tomegane::analyze(&video_path, &options)
            };

            match result {
                Ok(analysis) => {
                    if stream {
                        return;
                    }

                    if verbose {
                        let elapsed = start.elapsed();
                        eprintln!(
                            "Done in {:.2}s — {}/{} frames selected ({:.1}s video)",
                            elapsed.as_secs_f64(),
                            analysis.frame_count,
                            analysis.total_frames_extracted,
                            analysis.duration_seconds,
                        );
                    }

                    let json = serde_json::to_string_pretty(&analysis).unwrap();
                    if let Some(output_path) = output {
                        if let Err(e) = std::fs::write(&output_path, &json) {
                            eprintln!("Error writing output: {e}");
                            std::process::exit(1);
                        }
                        if verbose {
                            eprintln!("Output written to {output_path}");
                        }
                    } else {
                        println!("{json}");
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Setup { scope, yes } => {
            if let Err(e) = setup::run_setup(scope, yes) {
                eprintln!("Setup error: {e}");
                std::process::exit(1);
            }
        }

        Commands::Mcp => {
            if let Err(e) = tomegane::mcp::run_server() {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
        }
    }
}
