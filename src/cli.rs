use clap::{Parser, Subcommand};

use crate::analysis::AnalysisMode;
use crate::setup::SetupScope;

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

        /// Crop to a specific region of interest using x,y,w,h
        #[arg(long)]
        crop: Option<String>,

        /// Change threshold for smart frame selection (0.0 = keep all, 1.0 = only dramatic changes)
        #[arg(short, long)]
        threshold: Option<f64>,

        /// Maximum number of key frames to return (incompatible with --stream)
        #[arg(short, long, conflicts_with = "stream")]
        max_frames: Option<usize>,

        /// Analysis mode: overview for frame extraction, performance for jank-oriented insights
        #[arg(long, value_enum, default_value_t = AnalysisMode::Overview)]
        mode: AnalysisMode,

        /// Write JSON output to a file instead of stdout (incompatible with --stream)
        #[arg(long, conflicts_with = "stream")]
        output: Option<String>,

        /// Stream JSON events to stdout as frames are selected
        #[arg(long, default_value_t = false)]
        stream: bool,

        /// Print progress and debug info to stderr
        #[arg(short, long, default_value_t = false)]
        verbose: bool,
    },

    /// Detect supported MCP clients and help install tomegane into them
    Setup {
        /// Where to install the MCP configuration
        #[arg(long, value_enum, default_value_t = SetupScope::User)]
        scope: SetupScope,

        /// Install without interactive confirmation prompts
        #[arg(long, default_value_t = false, conflicts_with = "list")]
        yes: bool,

        /// Only report detected clients and status; do not modify any config
        #[arg(long, default_value_t = false)]
        list: bool,
    },

    /// Start the MCP server (JSON-RPC over stdin/stdout)
    Mcp,
}
