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
        } => {
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
                    let json = serde_json::to_string_pretty(&analysis).unwrap();
                    if let Some(output_path) = output {
                        if let Err(e) = std::fs::write(&output_path, &json) {
                            eprintln!("Error writing output: {e}");
                            std::process::exit(1);
                        }
                        eprintln!("Output written to {output_path}");
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
    }
}
