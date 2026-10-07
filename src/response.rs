use crate::error::{HdrError, HdrResult};
use crate::image::Frame;
use nalgebra::{linalg::SVD, DMatrix, DVector};

pub const CHANNELS: [char; 3] = ['R', 'G', 'B'];
pub const LAMBDA: f64 = 10.0;

#[inline]
pub fn weight(z: u8) -> f64 {
    z.min(255 - z) as f64
}

#[derive(Clone, Debug)]
pub struct SamplePoint {
    pub x: usize,
    pub y: usize,
}

#[derive(Clone, Debug)]
pub struct ChannelResponse {
    pub channel: char,
    pub g: Vec<f64>,
    pub samples: Vec<SamplePoint>,
    pub log_radiance: Vec<f64>,
    pub rank: usize,
    pub needed_rank: usize,
    pub sigma_min: f64,
    pub sigma_max: f64,
    pub data_weighted_rms: f64,
    pub data_weighted_max_abs: f64,
    pub smoothness_rms: f64,
}

fn candidate_grid(width: usize, height: usize, max_points: usize) -> Vec<(usize, usize)> {
    let ncols =
        (((max_points as f64) * (width as f64) / (height as f64)).sqrt()).ceil() as usize;
    let ncols = ncols.max(1);
    let nrows = (((max_points as f64) / (ncols as f64)).ceil() as usize).max(1);
    let mut points = Vec::with_capacity(ncols * nrows);
    for iy in 0..nrows {
        for ix in 0..ncols {
            let x = ((ix as f64 + 0.5) * (width as f64) / (ncols as f64)).floor() as usize;
            let y = ((iy as f64 + 0.5) * (height as f64) / (nrows as f64)).floor() as usize;
            points.push((x.min(width - 1), y.min(height - 1)));
        }
    }
    points.truncate(max_points);
    points
}

pub fn solve_channel(
    frames: &[Frame],
    width: usize,
    height: usize,
    channel_index: usize,
    max_points: usize,
) -> HdrResult<ChannelResponse> {
    let channel = CHANNELS[channel_index];
    let n_exp = frames.len();
    let ln_dt: Vec<f64> = frames.iter().map(|f| f.exposure_seconds.ln()).collect();

    let mut samples: Vec<SamplePoint> = Vec::new();
    let mut sample_values: Vec<Vec<u8>> = Vec::new();
    'outer: for (x, y) in candidate_grid(width, height, max_points) {
        let mut values = Vec::with_capacity(n_exp);
        let mut all_zero = true;
        let mut all_max = true;
        for frame in frames {
            let z = frame.image.pixel(x, y)[channel_index];
            values.push(z);
            if z != 0 { all_zero = false; }
            if z != 255 { all_max = false; }
        }
        // Reject points with no usable exposure at all, including points
        // that mix 0 and 255 across the bracket: every weight is zero
        // there, so their log-radiance unknown would be unconstrained.
        if all_zero || all_max || values.iter().all(|&z| weight(z) == 0.0) { continue; }
        samples.push(SamplePoint { x, y });
        sample_values.push(values);
        if samples.len() >= max_points { break 'outer; }
    }

    if samples.is_empty() {
        return Err(HdrError::Rank { channel, rank: 0, needed: 256, min_singular: 0.0, max_singular: 0.0 });
    }

    let p = samples.len();
    let unknowns = 256 + p;
    let mut rows: Vec<Vec<(usize, f64)>> = Vec::new();
    let mut rhs: Vec<f64> = Vec::new();

    for (i, values) in sample_values.iter().enumerate() {
        for (j, &z) in values.iter().enumerate() {
            let w = weight(z);
            if w == 0.0 { continue; }
            rows.push(vec![(z as usize, w), (256 + i, -w)]);
            rhs.push(w * ln_dt[j]);
        }
    }

    for z in 1..=254u16 {
        let w = LAMBDA * weight(z as u8);
        rows.push(vec![((z - 1) as usize, w), (z as usize, -2.0 * w), ((z + 1) as usize, w)]);
        rhs.push(0.0);
    }

    rows.push(vec![(128, 1.0)]);
    rhs.push(0.0);

    let m = rows.len();
    let mut a = DMatrix::<f64>::zeros(m, unknowns);
    let mut b = DVector::<f64>::zeros(m);
    for (row, (entries, &bv)) in rows.iter().zip(rhs.iter()).enumerate() {
        for &(col, val) in entries { a[(row, col)] = val; }
        b[row] = bv;
    }

    let svd = SVD::new_unordered(a.clone(), true, true);
    let sing = svd.singular_values.clone();
    let sigma_max = sing[0];
    let tol = (m.max(unknowns) as f64) * f64::EPSILON * sigma_max;
    let rank = sing.iter().filter(|&&s| s > tol).count();
    if rank < unknowns {
        return Err(HdrError::Rank { channel, rank, needed: unknowns, min_singular: sing[rank], max_singular: sigma_max });
    }
    let sigma_min = sing[unknowns - 1];

    let x = svd.solve(&b, tol).map_err(|_| HdrError::Rank {
        channel, rank, needed: unknowns, min_singular: sigma_min, max_singular: sigma_max,
    })?;

    let mut data_count = 0usize;
    let mut data_sq = 0.0;
    let mut data_max: f64 = 0.0;
    for (i, values) in sample_values.iter().enumerate() {
        for (j, &z) in values.iter().enumerate() {
            let w = weight(z);
            if w == 0.0 { continue; }
            let r = w * (x[z as usize] - x[256 + i] - ln_dt[j]);
            data_sq += r * r;
            data_max = data_max.max(r.abs());
            data_count += 1;
        }
    }
    let mut smooth_sq = 0.0;
    for z in 1..=254usize {
        let r = LAMBDA * weight(z as u8) * (x[z - 1] - 2.0 * x[z] + x[z + 1]);
        smooth_sq += r * r;
    }

    let g = (0..256).map(|z| x[z]).collect::<Vec<f64>>();
    let log_radiance = (0..p).map(|i| x[256 + i]).collect::<Vec<f64>>();

    Ok(ChannelResponse {
        channel, g, samples, log_radiance, rank, needed_rank: unknowns,
        sigma_min, sigma_max,
        data_weighted_rms: (data_sq / data_count.max(1) as f64).sqrt(),
        data_weighted_max_abs: data_max,
        smoothness_rms: (smooth_sq / 254.0).sqrt(),
    })
}

pub fn recover_responses(frames: &[Frame], width: usize, height: usize, max_points: usize)
    -> HdrResult<[ChannelResponse; 3]>
{
    Ok([
        solve_channel(frames, width, height, 0, max_points)?,
        solve_channel(frames, width, height, 1, max_points)?,
        solve_channel(frames, width, height, 2, max_points)?,
    ])
}
