use crate::error::{HdrError, HdrResult};
use crate::image::{encode_rgb8_png, Frame, Image8};
use crate::fusion::fuse;
use crate::response::recover_responses;
use crate::bake::project_sh;
use crate::probe::{LightProbe, SH_BASES};

pub const DEMO_WIDTH: usize = 160;
pub const DEMO_HEIGHT: usize = 80; // 2:1 equirectangular panorama

pub struct SyntheticData {
    pub frames: Vec<Frame>,
    pub ground_truth: Vec<[f64; 3]>,
    pub exposures: Vec<f64>,
    pub width: usize,
    pub height: usize,
}

fn response_forward(t: f64) -> f64 {
    let s = t.tanh() / 1.0f64.tanh();
    let mix = 0.25 * t + 0.75 * s;
    0.5 * (1.0 + mix.clamp(-1.0, 1.0))
}


fn ground_truth(width: usize, height: usize) -> Vec<[f64; 3]> {
    let mut data = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let u = x as f64 / (width - 1) as f64;
            let v = y as f64 / (height - 1) as f64;
            let band = ((u * 6.0 + v * 2.0).sin() * 0.5 + 0.5).powi(2);
            let log_r = -3.2 + 7.0 * u + 1.2 * (v - 0.5);
            let log_g = -3.2 + 7.0 * v + 0.8 * (0.5 - u) + 0.4 * (band - 0.5);
            let log_b = -3.2 + 7.0 * (1.0 - u) * (1.0 - v);
            let sun = ((u - 0.82).powi(2) + (v - 0.2).powi(2)).sqrt() < 0.07;
            let mut px = [log_r.exp(), log_g.exp(), log_b.exp()];
            if sun {
                for c in 0..3 { px[c] = 1.0e7; }
            }
            data.push(px);
        }
    }
    data
}

fn render_exposure(truth: &[[f64; 3]], width: usize, height: usize, dt: f64) -> Image8 {
    let mut rgb = Vec::with_capacity(width * height);
    for px in truth {
        let mut out = [0u8; 3];
        for c in 0..3 {
            let t = (px[c] * dt).ln();
            let q = response_forward(t / 2.6).clamp(0.0, 1.0);
            let z = (q * 255.0).round() as i64;
            out[c] = z.clamp(0, 255) as u8;
        }
        rgb.push(out);
    }
    Image8 { width, height, rgb }
}

pub fn build_synthetic() -> SyntheticData {
    let width = DEMO_WIDTH;
    let height = DEMO_HEIGHT;
    let truth = ground_truth(width, height);
    let exposures: Vec<f64> = vec![1.0 / 256.0, 1.0 / 64.0, 1.0 / 16.0, 1.0 / 4.0, 1.0, 4.0];
    let mut frames = Vec::new();
    for (i, &dt) in exposures.iter().enumerate() {
        let image = render_exposure(&truth, width, height, dt);
        frames.push(Frame {
            path: std::path::PathBuf::from(format!("demo_exposure_{}.png", i)),
            exposure_seconds: dt,
            image,
        });
    }
    SyntheticData { frames, ground_truth: truth, exposures, width, height }
}

pub struct SelfTestReport {
    pub log_rmse: f64,
    pub log_max_abs: f64,
    pub compared_pixels: usize,
    pub invalid_sun_pixels: usize,
    pub g_anchor_residual: f64,
}

pub fn run_selftest() -> HdrResult<SelfTestReport> {
    let synth = build_synthetic();
    let responses = recover_responses(&synth.frames, synth.width, synth.height, 128)?;
    let fused = fuse(&synth.frames, &responses)?;

    let mut anchor = 0.0f64;
    for r in &responses { anchor = anchor.max(r.g[128].abs()); }

    let mut sq = 0.0;
    let mut mx: f64 = 0.0;
    let mut count = 0usize;
    let mut invalid_sun = 0usize;
    for (i, truth_px) in synth.ground_truth.iter().enumerate() {
        let is_sun = truth_px[0] > 1.0e6;
        for c in 0..3 {
            let valid = fused.mask[i][c] == 255;
            if is_sun {
                if !valid { invalid_sun += 1; }
                continue;
            }
            if !valid {
                return Err(crate::error::HdrError::Job(format!(
                    "selftest: non-sun pixel {} channel {} marked invalid", i, c
                )));
            }
            let err = (fused.radiance[i * 3 + c] as f64 / truth_px[c]).ln().abs();
            sq += err * err;
            mx = mx.max(err);
            count += 1;
        }
    }
    Ok(SelfTestReport {
        log_rmse: (sq / count as f64).sqrt(),
        log_max_abs: mx,
        compared_pixels: count,
        invalid_sun_pixels: invalid_sun,
        g_anchor_residual: anchor,
    })
}

pub struct ProbeSelfTestReport {
    pub irradiance_max_rel_err: f64,
    pub clamp_checked: bool,
    pub yaw_checked: bool,
}

// In-memory probe checks: a constant panorama must reproduce the analytic
// Lambertian irradiance pi * L for every normal, independent of yaw, and
// negative evaluated irradiance must clamp to zero.
pub fn run_probe_selftest() -> HdrResult<ProbeSelfTestReport> {
    let width = 64usize;
    let height = 32usize;
    let level = [0.5f32, 1.0, 2.0];
    let mut radiance = vec![0.0f32; width * height * 3];
    for px in radiance.chunks_exact_mut(3) {
        px.copy_from_slice(&level);
    }
    let valid = vec![true; width * height];
    let mut max_rel: f64 = 0.0;
    for &yaw in &[0.0f64, 37.5, -123.0] {
        let (coeffs, used) = project_sh(width, height, &radiance, &valid, yaw)?;
        if used != width * height {
            return Err(HdrError::Probe("selftest: pixels dropped".to_string()));
        }
        let probe = LightProbe {
            coefficients: coeffs,
            yaw_degrees: yaw,
            source_pfm: "selftest".to_string(),
            source_mask: "selftest".to_string(),
            width,
            height,
            valid_pixels: used,
            skipped_pixels: 0,
        };
        let normals: Vec<[f64; 3]> = vec![
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.3, -0.5, 0.8],
        ];
        let albedos = vec![[1.0f64, 1.0, 1.0]; normals.len()];
        let out = probe.evaluate(&normals, &albedos)?;
        for rgb in out {
            for c in 0..3 {
                let expect = level[c] as f64;
                let rel = ((rgb[c] - expect) / expect).abs();
                max_rel = max_rel.max(rel);
            }
        }
    }
    // Center-sampled lat/long quadrature is exact for the constant term but
    // only approximate for degree-2, so allow a small discretization error.
    if max_rel > 2.0e-3 {
        return Err(HdrError::Probe(format!(
            "selftest: constant-environment irradiance off by {max_rel:.3e}"
        )));
    }
    // Clamp check: l=1-only probe goes negative on the opposite hemisphere.
    let mut neg_coeffs = [[0.0f64; SH_BASES]; 3];
    neg_coeffs[0][2] = 1.0; // Y10 ~ +y
    let probe = LightProbe {
        coefficients: neg_coeffs,
        yaw_degrees: 0.0,
        source_pfm: "selftest".to_string(),
        source_mask: "selftest".to_string(),
        width,
        height,
        valid_pixels: 0,
        skipped_pixels: 0,
    };
    let out = probe.evaluate(&[[0.0, -1.0, 0.0]], &[[0.8, 0.8, 0.8]])?;
    let clamp_ok = out[0][0] == 0.0;
    if !clamp_ok {
        return Err(HdrError::Probe(
            "selftest: negative irradiance was not clamped".to_string(),
        ));
    }
    Ok(ProbeSelfTestReport {
        irradiance_max_rel_err: max_rel,
        clamp_checked: clamp_ok,
        yaw_checked: true,
    })
}

pub fn write_demo_assets(dir: &std::path::Path) -> HdrResult<SyntheticData> {
    std::fs::create_dir_all(dir).map_err(|source| HdrError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let synth = build_synthetic();
    for (i, frame) in synth.frames.iter().enumerate() {
        let png = encode_rgb8_png(synth.width, synth.height, &frame.image.rgb)?;
        let p = dir.join(format!("demo_exposure_{}.png", i));
        crate::deliver::atomic_write(&p, &png)?;
    }
    let frames_json: Vec<String> = synth
        .exposures
        .iter()
        .enumerate()
        .map(|(i, dt)| format!("    {{ \"path\": \"demo_exposure_{}.png\", \"exposure_seconds\": {} }}", i, dt))
        .collect();
    let job = format!(
        "{{\n  \"frames\": [\n{}\n  ],\n  \"output_pfm\": \"demo_output.pfm\",\n  \"mask_png\": \"demo_mask.png\",\n  \"report_json\": \"demo_report.json\"\n}}\n",
        frames_json.join(",\n"),
    );
    crate::deliver::atomic_write(&dir.join("job.json"), job.as_bytes())?;
    Ok(synth)
}
