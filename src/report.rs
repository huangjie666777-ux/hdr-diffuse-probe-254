use crate::fusion::FusionResult;
use crate::image::Frame;
use crate::response::ChannelResponse;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct Point { x: usize, y: usize }

#[derive(Serialize)]
struct FrameInfo {
    path: String,
    exposure_seconds: f64,
    log_exposure: f64,
}

#[derive(Serialize)]
struct ChannelReport {
    channel: char,
    g: Vec<f64>,
    sample_coordinates: Vec<Point>,
    sample_log_radiance: Vec<f64>,
    rank: usize,
    needed_rank: usize,
    sigma_min: f64,
    sigma_max: f64,
    data_weighted_rms: f64,
    data_weighted_max_abs: f64,
    smoothness_rms: f64,
    valid_pixels: usize,
    invalid_pixels: usize,
    radiance_min: f64,
    radiance_max: f64,
    radiance_mean: f64,
}

#[derive(Serialize)]
pub struct FullReport {
    width: usize,
    height: usize,
    frame_count: usize,
    invalid_channel_pixels: usize,
    frames: Vec<FrameInfo>,
    responses: Vec<ChannelReport>,
    output_pfm: String,
    mask_png: String,
}

pub fn build_report(
    frames: &[Frame],
    width: usize,
    height: usize,
    responses: &[ChannelResponse; 3],
    fusion: &FusionResult,
    pfm_path: &Path,
    mask_path: &Path,
) -> FullReport {
    FullReport {
        width,
        height,
        frame_count: frames.len(),
        invalid_channel_pixels: fusion.invalid_count,
        frames: frames
            .iter()
            .map(|f| FrameInfo {
                path: f.path.display().to_string(),
                exposure_seconds: f.exposure_seconds,
                log_exposure: f.exposure_seconds.ln(),
            })
            .collect(),
        responses: responses
            .iter()
            .enumerate()
            .map(|(c, r)| ChannelReport {
                channel: r.channel,
                g: r.g.clone(),
                sample_coordinates: r.samples.iter().map(|s| Point { x: s.x, y: s.y }).collect(),
                sample_log_radiance: r.log_radiance.clone(),
                rank: r.rank,
                needed_rank: r.needed_rank,
                sigma_min: r.sigma_min,
                sigma_max: r.sigma_max,
                data_weighted_rms: r.data_weighted_rms,
                data_weighted_max_abs: r.data_weighted_max_abs,
                smoothness_rms: r.smoothness_rms,
                valid_pixels: fusion.stats[c].valid_pixels,
                invalid_pixels: fusion.stats[c].invalid_pixels,
                radiance_min: fusion.stats[c].min,
                radiance_max: fusion.stats[c].max,
                radiance_mean: fusion.stats[c].mean,
            })
            .collect(),
        output_pfm: pfm_path.display().to_string(),
        mask_png: mask_path.display().to_string(),
    }
}

pub fn write_report(path: &Path, report: &FullReport) -> crate::error::HdrResult<()> {
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|e| crate::error::HdrError::PngEncode(e.to_string()))?;
    crate::deliver::atomic_write(path, &bytes)
}

