use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "tomegane",
    about = "The remote-seeing eye for AI agents — extract smart frames from screen recordings",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Extract frames from a video and output structured JSON
    Analyze {
        /// Path to the video file
        video_path: String,

        /// Frame extraction interval in seconds
        #[arg(short, long, default_value_t = 1.0)]
        interval: f64,

        /// Output directory for extracted frames (default: temp dir)
        #[arg(short, long)]
        output_dir: Option<String>,

        /// Output image format (png or jpg)
        #[arg(short, long, default_value = "png")]
        format: String,

        /// Include base64-encoded images in JSON output
        #[arg(long, default_value_t = false)]
        base64: bool,

        /// Change threshold for smart frame selection (0.0 = keep all, 1.0 = only dramatic changes)
        #[arg(short, long)]
        threshold: Option<f64>,

        /// Maximum number of key frames to return
        #[arg(short, long)]
        max_frames: Option<usize>,

        /// Write JSON output to a file instead of stdout
        #[arg(long)]
        output: Option<String>,
    },
}
