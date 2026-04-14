use std::path::{Path, PathBuf};

use clap::ValueEnum;
use image::GrayImage;
use rayon::prelude::*;
use serde::Serialize;

use crate::extract::diff;
use crate::output::schema::{ChangeHotspot, FrameDelta, PerformanceInsights, SuspiciousWindow};

const PIXEL_DIFF_THRESHOLD: u8 = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMode {
    #[default]
    Overview,
    Performance,
}

pub fn inspect_performance(
    frame_paths: &[PathBuf],
    interval: f64,
) -> Result<PerformanceInsights, String> {
    if frame_paths.len() < 2 {
        return Ok(PerformanceInsights {
            summary: "Performance mode needs at least two sampled frames to compare movement."
                .to_string(),
            average_change_score: 0.0,
            peak_change_score: 0.0,
            elevated_change_threshold: 0.0,
            frame_deltas: vec![],
            suspicious_windows: vec![],
        });
    }

    let hashes: Vec<_> = frame_paths
        .par_iter()
        .map(|path| diff::phash(path))
        .collect::<Result<Vec<_>, _>>()?;

    let mut previous = load_luma(&frame_paths[0])?;
    let (frame_width, frame_height) = previous.dimensions();
    let frame_area = frame_width as f64 * frame_height as f64;
    let mut frame_deltas = Vec::with_capacity(frame_paths.len() - 1);

    for (idx, path) in frame_paths.iter().enumerate().skip(1) {
        let current = load_luma(path)?;
        if current.dimensions() != (frame_width, frame_height) {
            return Err(format!(
                "Frame {} has different dimensions from the first extracted frame",
                path.display()
            ));
        }

        let diff_metrics = measure_visual_diff(&previous, &current, frame_area);
        frame_deltas.push(FrameDelta {
            from_index: idx - 1,
            to_index: idx,
            start_timestamp_seconds: (idx - 1) as f64 * interval,
            end_timestamp_seconds: idx as f64 * interval,
            change_score: diff::change_score(hashes[idx - 1], hashes[idx]),
            changed_area_ratio: diff_metrics.changed_area_ratio,
            hotspot: diff_metrics.hotspot,
        });

        previous = current;
    }

    let average_change_score = frame_deltas
        .iter()
        .map(|delta| delta.change_score)
        .sum::<f64>()
        / frame_deltas.len() as f64;
    let peak_change_score = frame_deltas
        .iter()
        .map(|delta| delta.change_score)
        .fold(0.0, f64::max);
    let elevated_change_threshold = detect_threshold(&frame_deltas);
    let suspicious_windows =
        collect_suspicious_windows(&frame_deltas, elevated_change_threshold, frame_area);
    let summary = build_summary(
        &suspicious_windows,
        average_change_score,
        peak_change_score,
        elevated_change_threshold,
    );

    Ok(PerformanceInsights {
        summary,
        average_change_score,
        peak_change_score,
        elevated_change_threshold,
        frame_deltas,
        suspicious_windows,
    })
}

fn build_summary(
    suspicious_windows: &[SuspiciousWindow],
    average_change_score: f64,
    peak_change_score: f64,
    elevated_change_threshold: f64,
) -> String {
    let Some(strongest_window) = suspicious_windows.iter().max_by(|a, b| {
        window_strength(a)
            .partial_cmp(&window_strength(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        if peak_change_score < 0.08 {
            return "No obvious jank window surfaced at this sampling interval. If the lag is subtle, try a smaller interval or crop to the interaction region.".to_string();
        }

        return format!(
            "Visual change was present (avg {:.3}, peak {:.3}), but it did not cluster into a strong interaction window above the elevated-change threshold of {:.3}.",
            average_change_score, peak_change_score, elevated_change_threshold
        );
    };

    let hotspot = strongest_window.hotspot.as_ref().map(|region| {
        format!(
            " around x={}, y={}, w={}, h={}",
            region.x, region.y, region.width, region.height
        )
    });

    let hotspot_text = hotspot.unwrap_or_default();

    if let Some(region) = &strongest_window.hotspot {
        if region.coverage_ratio <= 0.25 {
            if strongest_window.sample_count == 1 {
                return format!(
                    "A sharp localized jump shows up from {:.1}s to {:.1}s{}; that can mean a sticky interaction, a missed intermediate state, or a quick layout jump in one UI region.",
                    strongest_window.start_timestamp_seconds,
                    strongest_window.end_timestamp_seconds,
                    hotspot_text
                );
            }

            return format!(
                "Elevated visual churn from {:.1}s to {:.1}s stays concentrated{}; if that interaction felt laggy, this pattern often lines up with repeated rerender or layout work in one UI region.",
                strongest_window.start_timestamp_seconds,
                strongest_window.end_timestamp_seconds,
                hotspot_text
            );
        }

        if region.coverage_ratio <= 0.6 {
            return format!(
                "The strongest window runs from {:.1}s to {:.1}s and repeatedly updates a mid-sized region{}; that looks more like section-level UI work than a full-screen transition.",
                strongest_window.start_timestamp_seconds,
                strongest_window.end_timestamp_seconds,
                hotspot_text
            );
        }
    }

    format!(
        "The strongest window runs from {:.1}s to {:.1}s and changes most of the sampled frame, which looks closer to a view-wide transition or large layout shift than a single control repaint.",
        strongest_window.start_timestamp_seconds, strongest_window.end_timestamp_seconds
    )
}

fn window_strength(window: &SuspiciousWindow) -> f64 {
    window.peak_change_score * 0.65
        + window.average_change_score * 0.25
        + window.average_changed_area_ratio * 0.10
}

fn detect_threshold(frame_deltas: &[FrameDelta]) -> f64 {
    let mut intensities: Vec<f64> = frame_deltas.iter().map(combined_intensity).collect();
    intensities.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let p50 = percentile(&intensities, 0.50);
    let p80 = percentile(&intensities, 0.80);
    let adaptive = p50 + (p80 - p50) * 0.65;

    adaptive.max(0.08)
}

fn percentile(sorted_values: &[f64], percentile: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }

    let percentile = percentile.clamp(0.0, 1.0);
    let index = ((sorted_values.len() - 1) as f64 * percentile).round() as usize;
    sorted_values[index]
}

fn combined_intensity(delta: &FrameDelta) -> f64 {
    delta.change_score * 0.75 + delta.changed_area_ratio * 0.25
}

fn collect_suspicious_windows(
    frame_deltas: &[FrameDelta],
    threshold: f64,
    frame_area: f64,
) -> Vec<SuspiciousWindow> {
    let mut windows = Vec::new();
    let mut current_start: Option<usize> = None;

    for (idx, delta) in frame_deltas.iter().enumerate() {
        let elevated = combined_intensity(delta) >= threshold;
        match (current_start, elevated) {
            (None, true) => current_start = Some(idx),
            (Some(start), false) => {
                windows.push(build_window(&frame_deltas[start..idx], frame_area));
                current_start = None;
            }
            _ => {}
        }
    }

    if let Some(start) = current_start {
        windows.push(build_window(&frame_deltas[start..], frame_area));
    }

    windows
}

fn build_window(deltas: &[FrameDelta], frame_area: f64) -> SuspiciousWindow {
    let start_timestamp_seconds = deltas
        .first()
        .map(|delta| delta.start_timestamp_seconds)
        .unwrap_or(0.0);
    let end_timestamp_seconds = deltas
        .last()
        .map(|delta| delta.end_timestamp_seconds)
        .unwrap_or(start_timestamp_seconds);
    let average_change_score =
        deltas.iter().map(|delta| delta.change_score).sum::<f64>() / deltas.len() as f64;
    let peak_change_score = deltas
        .iter()
        .map(|delta| delta.change_score)
        .fold(0.0, f64::max);
    let average_changed_area_ratio = deltas
        .iter()
        .map(|delta| delta.changed_area_ratio)
        .sum::<f64>()
        / deltas.len() as f64;
    let hotspot = union_hotspots(deltas, frame_area);
    let assessment = classify_window(deltas.len(), average_changed_area_ratio, hotspot.as_ref());

    SuspiciousWindow {
        start_timestamp_seconds,
        end_timestamp_seconds,
        sample_count: deltas.len(),
        average_change_score,
        peak_change_score,
        average_changed_area_ratio,
        hotspot,
        assessment,
    }
}

fn union_hotspots(deltas: &[FrameDelta], frame_area: f64) -> Option<ChangeHotspot> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for delta in deltas {
        let Some(hotspot) = &delta.hotspot else {
            continue;
        };
        min_x = min_x.min(hotspot.x);
        min_y = min_y.min(hotspot.y);
        max_x = max_x.max(hotspot.x + hotspot.width);
        max_y = max_y.max(hotspot.y + hotspot.height);
        found = true;
    }

    if !found {
        return None;
    }

    let width = max_x.saturating_sub(min_x);
    let height = max_y.saturating_sub(min_y);
    let coverage_ratio = (width as f64 * height as f64) / frame_area;

    Some(ChangeHotspot {
        x: min_x,
        y: min_y,
        width,
        height,
        coverage_ratio,
    })
}

fn classify_window(
    sample_count: usize,
    average_changed_area_ratio: f64,
    hotspot: Option<&ChangeHotspot>,
) -> String {
    let Some(hotspot) = hotspot else {
        return "Elevated change without a stable hotspot; this looks like a broad transition or a sampling artifact.".to_string();
    };

    if hotspot.coverage_ratio <= 0.25 {
        if sample_count >= 3 {
            return "Sustained localized churn. If the UI felt sticky here, inspect rerenders or layout work in this region.".to_string();
        }

        return "A short localized jump between samples. Check the interacted control or layout in this region.".to_string();
    }

    if hotspot.coverage_ratio <= 0.6 {
        return "Repeated updates inside a section of the screen. This points to section-level work more than a full-page redraw.".to_string();
    }

    if average_changed_area_ratio > 0.45 {
        return "Most of the frame changed together, which looks more like navigation or a large layout shift.".to_string();
    }

    "Large regions changed together; this is broader than a single control repaint.".to_string()
}

fn load_luma(path: &Path) -> Result<GrayImage, String> {
    image::open(path)
        .map_err(|e| format!("Failed to open image {}: {e}", path.display()))
        .map(|img| img.to_luma8())
}

struct VisualDiffMetrics {
    changed_area_ratio: f64,
    hotspot: Option<ChangeHotspot>,
}

fn measure_visual_diff(
    previous: &GrayImage,
    current: &GrayImage,
    frame_area: f64,
) -> VisualDiffMetrics {
    let mut changed_pixels = 0_u64;
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;

    for (x, y, pixel) in current.enumerate_pixels() {
        let previous_pixel = previous.get_pixel(x, y);
        let difference = pixel[0].abs_diff(previous_pixel[0]);
        if difference < PIXEL_DIFF_THRESHOLD {
            continue;
        }

        changed_pixels += 1;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        found = true;
    }

    let hotspot = if found {
        let width = max_x.saturating_sub(min_x).saturating_add(1);
        let height = max_y.saturating_sub(min_y).saturating_add(1);
        Some(ChangeHotspot {
            x: min_x,
            y: min_y,
            width,
            height,
            coverage_ratio: (width as f64 * height as f64) / frame_area,
        })
    } else {
        None
    };

    VisualDiffMetrics {
        changed_area_ratio: changed_pixels as f64 / frame_area,
        hotspot,
    }
}

#[cfg(test)]
mod tests {
    use image::{GrayImage, Luma};

    use super::*;

    #[test]
    fn performance_mode_finds_localized_window() {
        let tmp = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();

        for (idx, offset) in [0_u32, 6, 12, 12].into_iter().enumerate() {
            let path = tmp.path().join(format!("frame_{idx:04}.png"));
            write_test_frame(&path, offset).unwrap();
            paths.push(path);
        }

        let insights = inspect_performance(&paths, 0.25).unwrap();
        assert!(!insights.suspicious_windows.is_empty());
        assert!(insights.summary.contains("rerender") || insights.summary.contains("region"));

        let hotspot = insights.suspicious_windows[0].hotspot.as_ref().unwrap();
        assert!(hotspot.coverage_ratio < 0.4);
    }

    #[test]
    fn performance_mode_handles_single_frame() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("frame_0001.png");
        write_test_frame(&path, 0).unwrap();

        let insights = inspect_performance(&[path], 0.5).unwrap();
        assert!(insights.suspicious_windows.is_empty());
        assert_eq!(insights.frame_deltas.len(), 0);
    }

    fn write_test_frame(path: &Path, offset: u32) -> Result<(), String> {
        let mut image = GrayImage::from_pixel(48, 48, Luma([255]));

        for x in offset..(offset + 10).min(48) {
            for y in 14..34 {
                image.put_pixel(x, y, Luma([0]));
            }
        }

        image
            .save(path)
            .map_err(|e| format!("Failed to write test frame {}: {e}", path.display()))
    }
}
