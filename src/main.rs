use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::Parser;
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

use tomegane::analysis::AnalysisMode;
use tomegane::cli::{Cli, Commands};
use tomegane::error::Error;
use tomegane::extract::ffmpeg::CropRect;
use tomegane::output::schema::StreamEvent;
use tomegane::setup;
use tomegane::{AnalyzeOptions, ImageFormat};

fn main() -> ExitCode {
    let cli = Cli::parse();

    let verbose = matches!(cli.command, Commands::Analyze { verbose: true, .. });
    init_tracing(verbose);

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(code = e.code(), "{e}");
            eprintln!("Error: {e}");
            ExitCode::from(exit_code_for(&e))
        }
    }
}

fn init_tracing(verbose: bool) {
    let default_level = if verbose { "debug" } else { "warn" };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("tomegane={default_level}")));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .try_init();
}

fn run(cli: Cli) -> Result<(), Error> {
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
        } => run_analyze(AnalyzeArgs {
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
        }),

        Commands::Setup { scope, yes, list } => {
            if list {
                setup::run_list(scope)
            } else {
                setup::run_setup(scope, yes)
            }
        }

        Commands::Mcp => tomegane::mcp::run_server(),
    }
}

struct AnalyzeArgs {
    video_path: String,
    interval: f64,
    output_dir: Option<String>,
    format: String,
    base64: bool,
    crop: Option<String>,
    threshold: Option<f64>,
    max_frames: Option<usize>,
    mode: AnalysisMode,
    output: Option<String>,
    stream: bool,
    verbose: bool,
}

fn run_analyze(args: AnalyzeArgs) -> Result<(), Error> {
    let format: ImageFormat = args.format.parse()?;

    let crop = match args.crop {
        Some(ref spec) => Some(CropRect::parse(spec)?),
        None => None,
    };

    debug!(
        video = %args.video_path,
        interval = args.interval,
        format = %format,
        threshold = ?args.threshold,
        max_frames = ?args.max_frames,
        mode = ?args.mode,
        "starting analysis",
    );

    let options = AnalyzeOptions {
        interval: args.interval,
        output_dir: args.output_dir.clone().map(PathBuf::from),
        format,
        include_base64: args.base64,
        crop,
        threshold: args.threshold,
        max_frames: args.max_frames,
        analysis_mode: args.mode,
    };

    let start = Instant::now();

    let result = if args.stream {
        run_stream(&args.video_path, &options)
    } else {
        tomegane::analyze(&args.video_path, &options)
    }?;

    if args.stream {
        return Ok(());
    }

    if args.verbose {
        let elapsed = start.elapsed();
        info!(
            elapsed_seconds = elapsed.as_secs_f64(),
            frame_count = result.frame_count,
            total_extracted = result.total_frames_extracted,
            duration_seconds = result.duration_seconds,
            "analysis complete",
        );
    }

    let json = serde_json::to_string_pretty(&result)?;
    if let Some(output_path) = args.output {
        std::fs::write(&output_path, &json)?;
        if args.verbose {
            info!(path = %output_path, "wrote output");
        }
    } else {
        println!("{json}");
    }

    Ok(())
}

fn run_stream(
    video_path: &str,
    options: &AnalyzeOptions,
) -> Result<tomegane::output::schema::AnalysisResult, Error> {
    let emit_event = |event: &StreamEvent| -> Result<(), Error> {
        let json = serde_json::to_string(event)?;
        println!("{json}");
        Ok(())
    };

    let summary_options = AnalyzeOptions {
        include_base64: false,
        threshold: None,
        max_frames: Some(1),
        analysis_mode: AnalysisMode::Overview,
        ..options.clone()
    };
    let summary = tomegane::analyze(video_path, &summary_options)?;

    emit_event(&StreamEvent::Started {
        source: summary.source.clone(),
        duration_seconds: summary.duration_seconds,
        total_frames_extracted: summary.total_frames_extracted,
        output_format: summary.output_format.clone(),
    })?;

    let analysis = tomegane::analyze_stream(video_path, options, |frame| {
        emit_event(&StreamEvent::Frame {
            frame: frame.clone(),
        })
    })?;

    emit_event(&StreamEvent::Completed {
        result: analysis.clone(),
    })?;

    Ok(analysis)
}

fn exit_code_for(e: &Error) -> u8 {
    match e {
        Error::InvalidThreshold(_)
        | Error::InvalidFormat(_)
        | Error::InvalidCrop(_)
        | Error::InvalidArgument(_)
        | Error::TimestampOutOfRange { .. }
        | Error::StreamingUnsupportedWithMaxFrames
        | Error::StreamingUnsupportedWithOutput => 2,
        Error::FfmpegMissing => 3,
        Error::VideoNotFound(_)
        | Error::NoFramesExtracted
        | Error::FfmpegFailed { .. }
        | Error::ImageDecode { .. }
        | Error::Io(_) => 4,
        Error::Parse { .. } | Error::Serde(_) => 1,
    }
}
