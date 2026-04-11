use std::path::Path;

use image::imageops::FilterType;
use rayon::prelude::*;

/// A perceptual hash — 64-bit fingerprint of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PHash(pub u64);

/// Compute the perceptual hash (pHash) of an image file.
///
/// Algorithm:
/// 1. Resize to 8x8 grayscale
/// 2. Compute the mean pixel value
/// 3. Set each bit to 1 if the pixel is above the mean, 0 otherwise
///
/// This produces a 64-bit hash that is robust to scaling and minor changes
/// but sensitive to meaningful visual differences.
pub fn phash(image_path: &Path) -> Result<PHash, String> {
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image {}: {e}", image_path.display()))?;

    // Resize to 8x8 and convert to grayscale
    let small = img.resize_exact(8, 8, FilterType::Lanczos3).to_luma8();

    // Compute mean pixel value
    let pixels: Vec<u8> = small.pixels().map(|p| p.0[0]).collect();
    let mean: f64 = pixels.iter().map(|&p| p as f64).sum::<f64>() / pixels.len() as f64;

    // Build hash: 1 if above mean, 0 otherwise
    let mut hash: u64 = 0;
    for (i, &pixel) in pixels.iter().enumerate() {
        if pixel as f64 > mean {
            hash |= 1 << i;
        }
    }

    Ok(PHash(hash))
}

/// Compute the hamming distance between two perceptual hashes.
/// Returns a value between 0 (identical) and 64 (completely different).
pub fn hamming_distance(a: PHash, b: PHash) -> u32 {
    (a.0 ^ b.0).count_ones()
}

/// Compute a normalized change score between two hashes.
/// Returns a value between 0.0 (identical) and 1.0 (completely different).
pub fn change_score(a: PHash, b: PHash) -> f64 {
    hamming_distance(a, b) as f64 / 64.0
}

/// Select key frames from a list of image paths based on perceptual difference.
///
/// Returns indices of frames that represent significant visual changes.
/// - `threshold`: minimum change_score (0.0-1.0) to consider a frame "different enough"
/// - `max_frames`: optional cap on the number of frames returned
///
/// The first frame is always included. Subsequent frames are included only if
/// their change_score relative to the last *included* frame exceeds the threshold.
pub fn select_key_frames(
    frame_paths: &[impl AsRef<Path> + Sync],
    threshold: f64,
    max_frames: Option<usize>,
) -> Result<Vec<(usize, f64)>, String> {
    if frame_paths.is_empty() {
        return Ok(vec![]);
    }

    // Compute all hashes in parallel using rayon
    let hashes: Vec<PHash> = frame_paths
        .par_iter()
        .map(|path| phash(path.as_ref()))
        .collect::<Result<Vec<_>, _>>()?;

    // Sequential selection: compare against last *included* frame
    let mut selected: Vec<(usize, f64)> = vec![(0, 0.0)];
    let mut last_hash = hashes[0];

    for (i, &current_hash) in hashes.iter().enumerate().skip(1) {
        let score = change_score(last_hash, current_hash);

        if score >= threshold {
            selected.push((i, score));
            last_hash = current_hash;
        }
    }

    // If max_frames is set and we have too many, keep the most significant changes
    if let Some(max) = max_frames
        && selected.len() > max
    {
        // Always keep the first frame
        let first = selected[0];
        let mut rest: Vec<(usize, f64)> = selected[1..].to_vec();
        // Sort by change_score descending — keep the biggest changes
        rest.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        rest.truncate(max - 1);
        // Re-sort by index to maintain chronological order
        rest.push(first);
        rest.sort_by_key(|&(idx, _)| idx);
        selected = rest;
    }

    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_images_have_zero_distance() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");

        // Extract two copies of the same frame
        let tmp = tempfile::tempdir().unwrap();
        crate::extract::ffmpeg::extract_frames(&fixture, tmp.path(), 5.0, "png").unwrap();

        let frame = tmp.path().join("frame_0001.png");
        let hash1 = phash(&frame).unwrap();
        let hash2 = phash(&frame).unwrap();

        assert_eq!(hamming_distance(hash1, hash2), 0);
        assert_eq!(change_score(hash1, hash2), 0.0);
    }

    #[test]
    fn hamming_distance_is_symmetric() {
        let a = PHash(0b1010_1010);
        let b = PHash(0b0101_0101);
        assert_eq!(hamming_distance(a, b), hamming_distance(b, a));
    }

    #[test]
    fn max_hamming_distance_is_64() {
        let a = PHash(0);
        let b = PHash(u64::MAX);
        assert_eq!(hamming_distance(a, b), 64);
        assert_eq!(change_score(a, b), 1.0);
    }

    #[test]
    fn change_score_is_normalized() {
        let a = PHash(0);
        let b = PHash(0b1111); // 4 bits different
        let score = change_score(a, b);
        assert!((score - 4.0 / 64.0).abs() < f64::EPSILON);
    }

    #[test]
    fn select_key_frames_always_includes_first() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();
        crate::extract::ffmpeg::extract_frames(&fixture, tmp.path(), 1.0, "png").unwrap();

        let mut frames: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
            .collect();
        frames.sort();

        let selected = select_key_frames(&frames, 0.0, None).unwrap();
        assert_eq!(selected[0].0, 0, "First frame should always be selected");
        assert_eq!(selected[0].1, 0.0, "First frame should have score 0.0");
    }

    #[test]
    fn high_threshold_selects_fewer_frames() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();
        crate::extract::ffmpeg::extract_frames(&fixture, tmp.path(), 1.0, "png").unwrap();

        let mut frames: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
            .collect();
        frames.sort();

        let low = select_key_frames(&frames, 0.0, None).unwrap();
        let high = select_key_frames(&frames, 0.5, None).unwrap();

        assert!(
            high.len() <= low.len(),
            "Higher threshold should select fewer or equal frames: {} vs {}",
            high.len(),
            low.len()
        );
    }

    #[test]
    fn max_frames_caps_output() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test_video.mp4");
        let tmp = tempfile::tempdir().unwrap();
        crate::extract::ffmpeg::extract_frames(&fixture, tmp.path(), 1.0, "png").unwrap();

        let mut frames: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
            .collect();
        frames.sort();

        let selected = select_key_frames(&frames, 0.0, Some(3)).unwrap();
        assert!(
            selected.len() <= 3,
            "Should cap at max_frames=3, got {}",
            selected.len()
        );
    }

    #[test]
    fn empty_input_returns_empty() {
        let frames: Vec<&Path> = vec![];
        let selected = select_key_frames(&frames, 0.1, None).unwrap();
        assert!(selected.is_empty());
    }
}
