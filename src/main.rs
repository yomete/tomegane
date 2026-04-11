use std::time::Instant;

use clap::Parser;

use tomegane::cli::{Cli, Commands};
use tomegane::extract::ffmpeg::CropRect;
use tomegane::output::schema::StreamEvent;

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
                        interval,
                        output_dir.as_deref(),
                        &format,
                        false,
                        crop,
                        None,
                        Some(1),
                    )?;

                    emit_event(&StreamEvent::Started {
                        source: summary.source.clone(),
                        duration_seconds: summary.duration_seconds,
                        total_frames_extracted: summary.total_frames_extracted,
                        output_format: summary.output_format.clone(),
                    })?;

                    let analysis = tomegane::analyze_stream(
                        &video_path,
                        interval,
                        output_dir.as_deref(),
                        &format,
                        base64,
                        crop,
                        threshold,
                        max_frames,
                        |frame| {
                            emit_event(&StreamEvent::Frame {
                                frame: frame.clone(),
                            })
                        },
                    )?;

                    emit_event(&StreamEvent::Completed {
                        result: analysis.clone(),
                    })?;

                    Ok(analysis)
                })()
            } else {
                tomegane::analyze(
                    &video_path,
                    interval,
                    output_dir.as_deref(),
                    &format,
                    base64,
                    crop,
                    threshold,
                    max_frames,
                )
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

        Commands::Mcp => {
            if let Err(e) = tomegane::mcp::run_server() {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
        }
    }
}
