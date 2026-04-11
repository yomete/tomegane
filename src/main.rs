use std::time::Instant;

use clap::Parser;

use tomegane::cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Analyze {
            video_path,
            interval,
            output_dir,
            format,
            base64,
            threshold,
            max_frames,
            output,
            verbose,
        } => {
            if verbose {
                eprintln!("tomegane v{}", env!("CARGO_PKG_VERSION"));
                eprintln!("Video: {video_path}");
                eprintln!("Interval: {interval}s | Format: {format}");
                if let Some(t) = threshold {
                    eprintln!("Threshold: {t}");
                }
                if let Some(m) = max_frames {
                    eprintln!("Max frames: {m}");
                }
            }

            let start = Instant::now();

            let result = tomegane::analyze(
                &video_path,
                interval,
                output_dir.as_deref(),
                &format,
                base64,
                threshold,
                max_frames,
            );

            match result {
                Ok(analysis) => {
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
