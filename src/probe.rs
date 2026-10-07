//! Light probe baking (equirect HDR panorama -> 9-term real SH per channel)
//! and diffuse shading queries against a baked probe.

use crate::error::{HdrError, HdrResult};
use crate::image::Image8;
use crate::pfm::read_pfm_rgb32;
use crate::sh;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::path::Path;

pub const PROBE_FORMAT: &str = "hdr_probe254.light_probe";
pub const PROBE_VERSION: u32 = 1;
pub const MAX_SIDE: usize = 512;

pub const COORDINATE_CONVENTION: &str =
    "y-up sphere; longitude increases from +x toward +z; +     equirect row 0 is the +y (top) edge; yaw rotates right-handed about +y";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeSource {
    pub pfm: String,
    pub mask_png: String,
    pub width: usize,
    pub height: usize,
    pub valid_pixels: usize,
    pub total_pixels: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LightProbe {
    pub format: String,
    pub version: u32,
    pub coordinate_convention: String,
    pub basis_order: Vec<String>,
    pub basis_constants: Vec<f64>,
    pub band_convolution: Vec<f64>,
    pub yaw_degrees: f64,
    /// Radiance coefficients, [channel][basis], channels in R, G, B order.
    pub coefficients: [[f64; sh::NUM_BASIS]; 3],
    pub source: ProbeSource,
}

fn probe_err(message: impl Into<String>) -> HdrError {
    HdrError::Probe(message.into())
}

/// Bake a light probe from an RGB32F PFM panorama and its RGB validity mask.
///
/// The panorama must be 2:1 equirectangular with each side at most 512 px.
/// Every radiance value must be finite and non-negative. A pixel contributes
/// only when all three mask channels are 255; masked-out pixels are skipped
/// entirely (never substituted with black). Inputs are never modified.
pub fn bake(pfm_path: &Path, mask_path: &Path, yaw_degrees: f64) -> HdrResult<LightProbe> {
    if !yaw_degrees.is_finite() {
        return Err(probe_err(format!("yaw must be finite, got {yaw_degrees}")));
    }
    let (width, height, rgb) = read_pfm_rgb32(pfm_path)?;
    if width != 2 * height {
        return Err(probe_err(format!(
            "panorama must be 2:1 equirectangular, got {width}x{height}"
        )));
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(probe_err(format!(
            "panorama side exceeds {MAX_SIDE} px: {width}x{height}"
        )));
    }
    let mask = Image8::load(mask_path)?;
    if mask.width != width || mask.height != height {
        return Err(probe_err(format!(
            "mask is {}x{}, panorama is {width}x{height}",
            mask.width, mask.height
        )));
    }
    for (i, &v) in rgb.iter().enumerate() {
        if !v.is_finite() || v < 0.0 {
            let px = i / 3;
            return Err(probe_err(format!(
                "radiance at ({},{}) channel {} is invalid ({v}); +                 radiance must be finite and non-negative",
                px % width,
                px / width,
                i % 3
            )));
        }
    }

    let yaw = yaw_degrees.to_radians();
    let (sy, cy) = yaw.sin_cos();
    let mut coefficients = [[0.0f64; sh::NUM_BASIS]; 3];
    let mut valid_pixels = 0usize;
    let dphi = 2.0 * PI / width as f64;
    for y in 0..height {
        let theta0 = y as f64 / height as f64 * PI;
        let theta1 = (y + 1) as f64 / height as f64 * PI;
        // Exact solid angle of the latitudinal band boundary times dphi.
        let w_lat = theta0.cos() - theta1.cos();
        let theta = (y as f64 + 0.5) / height as f64 * PI;
        let (st, ct) = theta.sin_cos();
        for x in 0..width {
            let idx = y * width + x;
            if mask.rgb[idx] != [255, 255, 255] {
                continue;
            }
            let phi = (x as f64 + 0.5) * dphi;
            let (sp, cp) = phi.sin_cos();
            let dir = [st * cp, ct, st * sp];
            // Environment yaw: right-handed rotation about +y.
            let dir = [
                dir[0] * cy + dir[2] * sy,
                dir[1],
                -dir[0] * sy + dir[2] * cy,
            ];
            let basis = sh::eval(&dir);
            let w = w_lat * dphi;
            for c in 0..3 {
                let r = rgb[idx * 3 + c] as f64;
                for k in 0..sh::NUM_BASIS {
                    coefficients[c][k] += r * basis[k] * w;
                }
            }
            valid_pixels += 1;
        }
    }
    if valid_pixels == 0 {
        return Err(probe_err("mask marks every pixel invalid; nothing to bake"));
    }

    Ok(LightProbe {
        format: PROBE_FORMAT.to_string(),
        version: PROBE_VERSION,
        coordinate_convention: COORDINATE_CONVENTION.to_string(),
        basis_order: sh::BASIS_ORDER.iter().map(|s| s.to_string()).collect(),
        basis_constants: sh::BASIS_CONSTANTS.to_vec(),
        band_convolution: sh::BAND_CONVOLUTION.to_vec(),
        yaw_degrees,
        coefficients,
        source: ProbeSource {
            pfm: pfm_path.display().to_string(),
            mask_png: mask_path.display().to_string(),
            width,
            height,
            valid_pixels,
            total_pixels: width * height,
        },
    })
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct ShadingQuery {
    pub normal: [f64; 3],
    pub albedo: [f64; 3],
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ShadingResult {
    pub normal: [f64; 3],
    pub albedo: [f64; 3],
    pub irradiance: [f64; 3],
    pub radiance: [f64; 3],
}

impl LightProbe {
    pub fn save(&self, path: &Path) -> HdrResult<()> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| probe_err(format!("failed to serialize probe: {e}")))?;
        crate::deliver::atomic_write(path, &bytes)
    }

    pub fn load(path: &Path) -> HdrResult<LightProbe> {
        let bytes = std::fs::read(path).map_err(|source| HdrError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let probe: LightProbe =
            serde_json::from_slice(&bytes).map_err(|e| HdrError::Json {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
        if probe.format != PROBE_FORMAT {
            return Err(probe_err(format!(
                "{}: unsupported format {:?}",
                path.display(),
                probe.format
            )));
        }
        if probe.version != PROBE_VERSION {
            return Err(probe_err(format!(
                "{}: unsupported version {}",
                path.display(),
                probe.version
            )));
        }
        for (c, ch) in probe.coefficients.iter().enumerate() {
            for (k, &v) in ch.iter().enumerate() {
                if !v.is_finite() {
                    return Err(probe_err(format!(
                        "{}: coefficient [{c}][{k}] is not finite",
                        path.display()
                    )));
                }
            }
        }
        Ok(probe)
    }

    /// Diffuse irradiance for a unit normal; negative results clamped to 0.
    pub fn irradiance(&self, unit_normal: &[f64; 3]) -> [f64; 3] {
        let basis = sh::eval(unit_normal);
        let mut out = [0.0f64; 3];
        for c in 0..3 {
            let mut e = 0.0;
            for k in 0..sh::NUM_BASIS {
                e += sh::BAND_CONVOLUTION[sh::BAND_OF_INDEX[k]]
                    * self.coefficients[c][k]
                    * basis[k];
            }
            out[c] = e.max(0.0);
        }
        out
    }

    /// Batch diffuse shading: normals must be finite and non-zero (they are
    /// normalized here), albedo channels must lie in [0, 1]. Outgoing
    /// radiance is albedo * irradiance / pi and is not clamped above.
    pub fn evaluate_batch(&self, queries: &[ShadingQuery]) -> HdrResult<Vec<ShadingResult>> {
        let mut results = Vec::with_capacity(queries.len());
        for (i, q) in queries.iter().enumerate() {
            let n = q.normal;
            if !n.iter().all(|v| v.is_finite()) {
                return Err(probe_err(format!("query {i}: normal is not finite")));
            }
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len == 0.0 {
                return Err(probe_err(format!("query {i}: normal is the zero vector")));
            }
            for (c, &a) in q.albedo.iter().enumerate() {
                if !a.is_finite() || a < 0.0 || a > 1.0 {
                    return Err(probe_err(format!(
                        "query {i}: albedo channel {c} = {a} is outside [0, 1]"
                    )));
                }
            }
            let unit = [n[0] / len, n[1] / len, n[2] / len];
            let irradiance = self.irradiance(&unit);
            let mut radiance = [0.0f64; 3];
            for c in 0..3 {
                radiance[c] = q.albedo[c] * irradiance[c] / PI;
            }
            results.push(ShadingResult {
                normal: unit,
                albedo: q.albedo,
                irradiance,
                radiance,
            });
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pfm::write_pfm_rgb32;

    fn write_constant_pfm(dir: &Path, width: usize, height: usize, e: [f32; 3]) -> std::path::PathBuf {
        let rgb = vec![e; width * height].into_iter().flatten().collect::<Vec<f32>>();
        let path = dir.join("const.pfm");
        write_pfm_rgb32(&path, width, height, &rgb).unwrap();
        path
    }

    fn write_solid_mask(dir: &Path, width: usize, height: usize) -> std::path::PathBuf {
        let path = dir.join("mask.png");
        let bytes = crate::image::encode_rgb8_png(width, height, &vec![[255u8; 3]; width * height])
            .unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn constant_environment_gives_pi_e_irradiance() {
        let dir = std::env::temp_dir().join("hdr_probe254_test_const");
        std::fs::create_dir_all(&dir).unwrap();
        let e = [0.5f32, 1.0, 2.0];
        let pfm = write_constant_pfm(&dir, 256, 128, e);
        let mask = write_solid_mask(&dir, 256, 128);
        let probe = bake(&pfm, &mask, 0.0).unwrap();
        for normal in [[0.0f64, 1.0, 0.0], [1.0, 0.0, 0.0], [0.3, -0.5, 0.8]] {
            let len = (normal[0].powi(2) + normal[1].powi(2) + normal[2].powi(2)).sqrt();
            let n = [normal[0] / len, normal[1] / len, normal[2] / len];
            let irr = probe.irradiance(&n);
            for c in 0..3 {
                let expect = PI * e[c] as f64;
                // l<=2 truncation of a sampled constant field: allow small
                // aliasing error from the discrete grid.
                assert!(
                    (irr[c] - expect).abs() < 2e-3 * expect.max(1.0),
                    "irr[{c}] = {}, expect {expect}",
                    irr[c]
                );
            }
        }
        // round-trip through JSON
        let json = dir.join("probe.json");
        probe.save(&json).unwrap();
        let loaded = LightProbe::load(&json).unwrap();
        let results = loaded
            .evaluate_batch(&[ShadingQuery { normal: [0.0, 2.0, 0.0], albedo: [1.0, 0.5, 0.0] }])
            .unwrap();
        assert!((results[0].radiance[0] - e[0] as f64).abs() < 2e-3);
        assert!((results[0].radiance[1] - 0.5 * e[1] as f64).abs() < 2e-3);
        // invalid queries are rejected
        assert!(loaded
            .evaluate_batch(&[ShadingQuery { normal: [0.0, 0.0, 0.0], albedo: [0.5; 3] }])
            .is_err());
        assert!(loaded
            .evaluate_batch(&[ShadingQuery { normal: [0.0, 1.0, 0.0], albedo: [1.5, 0.0, 0.0] }])
            .is_err());
    }

    #[test]
    fn negative_radiance_is_rejected() {
        let dir = std::env::temp_dir().join("hdr_probe254_test_neg");
        std::fs::create_dir_all(&dir).unwrap();
        let mut rgb = vec![1.0f32; 8 * 4 * 3];
        rgb[0] = -1.0;
        let pfm = dir.join("neg.pfm");
        write_pfm_rgb32(&pfm, 8, 4, &rgb).unwrap();
        let mask = write_solid_mask(&dir, 8, 4);
        assert!(bake(&pfm, &mask, 0.0).is_err());
    }

    #[test]
    fn non_2to1_is_rejected() {
        let dir = std::env::temp_dir().join("hdr_probe254_test_ar");
        std::fs::create_dir_all(&dir).unwrap();
        let pfm = write_constant_pfm(&dir, 16, 16, [1.0; 3]);
        let mask = write_solid_mask(&dir, 16, 16);
        assert!(bake(&pfm, &mask, 0.0).is_err());
    }
}
