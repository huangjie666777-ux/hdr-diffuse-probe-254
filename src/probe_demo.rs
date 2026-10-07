//! End-to-end example: synthetic equirect panorama -> baked SH probe ->
//! batch diffuse shading queries.

use crate::error::HdrResult;
use crate::image::encode_rgb8_png;
use crate::pfm::write_pfm_rgb32;
use crate::probe::{bake, LightProbe, ShadingQuery};
use std::f64::consts::PI;
use std::path::Path;

pub const PANO_WIDTH: usize = 128;
pub const PANO_HEIGHT: usize = 64;

/// Synthetic sky: warm horizon-to-zenith gradient plus a small bright sun
/// disk and a dim greenish ground bounce.
pub fn synthetic_panorama(width: usize, height: usize) -> Vec<f32> {
    let mut rgb = vec![0.0f32; width * height * 3];
    let sun_dir = {
        let theta = 0.35 * PI;
        let phi = 1.2 * PI;
        [theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin()]
    };
    for y in 0..height {
        let theta = (y as f64 + 0.5) / height as f64 * PI;
        let (st, ct) = theta.sin_cos();
        for x in 0..width {
            let phi = (x as f64 + 0.5) / width as f64 * 2.0 * PI;
            let (sp, cp) = phi.sin_cos();
            let dir = [st * cp, ct, st * sp];
            let sky = (ct * 0.5 + 0.5).max(0.0);
            let mut px = [
                0.25 + 0.55 * sky,
                0.30 + 0.45 * sky,
                0.40 + 0.35 * sky,
            ];
            if ct < 0.0 {
                // ground bounce
                px = [0.10, 0.16, 0.08];
            }
            let cos_sun = dir[0] * sun_dir[0] + dir[1] * sun_dir[1] + dir[2] * sun_dir[2];
            if cos_sun > 0.999 {
                px = [40.0, 36.0, 30.0];
            }
            let i = (y * width + x) * 3;
            rgb[i] = px[0] as f32;
            rgb[i + 1] = px[1] as f32;
            rgb[i + 2] = px[2] as f32;
        }
    }
    rgb
}

pub struct ProbeDemoOutput {
    pub probe: LightProbe,
    pub results: Vec<crate::probe::ShadingResult>,
}

/// Write panorama.pfm + panorama_mask.png, bake probe.json, run the example
/// queries and write query_results.json under 'dir'.
pub fn run_probe_demo(dir: &Path) -> HdrResult<ProbeDemoOutput> {
    std::fs::create_dir_all(dir).map_err(|source| crate::error::HdrError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let rgb = synthetic_panorama(PANO_WIDTH, PANO_HEIGHT);
    let pfm_path = dir.join("panorama.pfm");
    write_pfm_rgb32(&pfm_path, PANO_WIDTH, PANO_HEIGHT, &rgb)?;

    let mask_path = dir.join("panorama_mask.png");
    let mask = vec![[255u8; 3]; PANO_WIDTH * PANO_HEIGHT];
    crate::deliver::atomic_write(&mask_path, &encode_rgb8_png(PANO_WIDTH, PANO_HEIGHT, &mask)?)?;

    let probe = bake(&pfm_path, &mask_path, 0.0)?;
    probe.save(&dir.join("probe.json"))?;

    let queries = [
        ShadingQuery { normal: [0.0, 1.0, 0.0], albedo: [0.8, 0.8, 0.8] },
        ShadingQuery { normal: [0.0, -1.0, 0.0], albedo: [0.5, 0.5, 0.5] },
        ShadingQuery { normal: [1.0, 0.0, 0.0], albedo: [0.9, 0.3, 0.2] },
        ShadingQuery { normal: [0.0, 0.3, 1.0], albedo: [0.2, 0.4, 0.9] },
    ];
    let results = probe.evaluate_batch(&queries)?;
    let json = serde_json::to_vec_pretty(&results)
        .map_err(|e| crate::error::HdrError::Probe(e.to_string()))?;
    crate::deliver::atomic_write(&dir.join("query_results.json"), &json)?;

    Ok(ProbeDemoOutput { probe, results })
}
