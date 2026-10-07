use crate::error::{HdrError, HdrResult};
use crate::image::Frame;
use crate::response::{weight, ChannelResponse};

pub struct FusionResult {
    pub radiance: Vec<f32>,
    pub mask: Vec<[u8; 3]>,
    pub invalid_count: usize,
    pub stats: [ChannelStats; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct ChannelStats {
    pub valid_pixels: usize,
    pub invalid_pixels: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
}

pub fn fuse(frames: &[Frame], responses: &[ChannelResponse; 3]) -> HdrResult<FusionResult> {
    let first = &frames[0].image;
    let n = first.width * first.height;
    let mut radiance = vec![0.0f32; n * 3];
    let mut mask = vec![[0u8; 3]; n];
    let mut invalid_count = 0usize;
    let mut stats = [ChannelStats {
        valid_pixels: 0,
        invalid_pixels: 0,
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        mean: 0.0,
    }; 3];

    for c in 0..3 {
        let g = &responses[c].g;
        let mut sum = 0.0;
        for y in 0..first.height {
            for x in 0..first.width {
                let idx = y * first.width + x;
                let mut acc = 0.0f64;
                let mut wsum = 0.0f64;
                for frame in frames {
                    let z = frame.image.rgb[idx][c];
                    let w = weight(z);
                    if w > 0.0 {
                        acc += w * (g[z as usize] - frame.exposure_seconds.ln());
                        wsum += w;
                    }
                }
                if wsum <= 0.0 {
                    mask[idx][c] = 0;
                    radiance[idx * 3 + c] = 0.0;
                    stats[c].invalid_pixels += 1;
                    invalid_count += 1;
                    continue;
                }
                let log_e = acc / wsum;
                let e = log_e.exp();
                if !e.is_finite() {
                    return Err(HdrError::NonFinite {
                        channel: crate::response::CHANNELS[c],
                        x,
                        y,
                        value: e,
                    });
                }
                let ef = e as f32;
                if !ef.is_finite() {
                    return Err(HdrError::NonFinite {
                        channel: crate::response::CHANNELS[c],
                        x,
                        y,
                        value: ef as f64,
                    });
                }
                radiance[idx * 3 + c] = ef;
                mask[idx][c] = 255;
                stats[c].valid_pixels += 1;
                stats[c].min = stats[c].min.min(e);
                stats[c].max = stats[c].max.max(e);
                sum += e;
            }
        }
        stats[c].mean = sum / stats[c].valid_pixels.max(1) as f64;
        if stats[c].valid_pixels == 0 {
            stats[c].min = 0.0;
            stats[c].max = 0.0;
            stats[c].mean = 0.0;
        }
    }

    Ok(FusionResult { radiance, mask, invalid_count, stats })
}
