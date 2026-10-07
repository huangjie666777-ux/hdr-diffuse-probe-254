use crate::error::{HdrError, HdrResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

// Orthonormalized real spherical harmonics, bands l = 0..=2 (9 bases).
// Coordinate convention: right-handed, +y is up (the polar axis); longitude
// is measured from +x toward +z. Basis order and constants:
//   Y00  = C0
//   Y1-1 = C1 * z        Y10 = C1 * y          Y11 = C1 * x
//   Y2-2 = C2 * x*z      Y2-1 = C2 * y*z       Y20 = C3 * (3y^2 - 1)
//   Y21  = C2 * x*y      Y22  = C4 * (x^2 - z^2)
pub const SH_C0: f64 = 0.282_094_791_773_878_14; // 1/2 * sqrt(1/pi)
pub const SH_C1: f64 = 0.488_602_511_902_919_9; // sqrt(3/(4*pi))
pub const SH_C2: f64 = 1.092_548_430_592_079_2; // 1/2 * sqrt(15/pi)
pub const SH_C3: f64 = 0.315_391_565_253_520_05; // 1/4 * sqrt(5/pi)
pub const SH_C4: f64 = 0.546_274_215_296_039_6; // 1/4 * sqrt(15/pi)

pub const SH_BASES: usize = 9;
pub const SH_BASIS_ORDER: [&str; SH_BASES] = [
    "Y00", "Y1-1", "Y10", "Y11", "Y2-2", "Y2-1", "Y20", "Y21", "Y22",
];

// Lambertian convolution coefficients per band (Ramamoorthi & Hanrahan).
pub const CONVOLUTION_BANDS: [f64; 3] = [
    std::f64::consts::PI,
    2.0 * std::f64::consts::PI / 3.0,
    std::f64::consts::FRAC_PI_4,
];

#[inline]
fn band_of(index: usize) -> usize {
    match index {
        0 => 0,
        1..=3 => 1,
        _ => 2,
    }
}

pub fn eval_basis(direction: [f64; 3]) -> [f64; SH_BASES] {
    let [x, y, z] = direction;
    [
        SH_C0,
        SH_C1 * z,
        SH_C1 * y,
        SH_C1 * x,
        SH_C2 * x * z,
        SH_C2 * y * z,
        SH_C3 * (3.0 * y * y - 1.0),
        SH_C2 * x * y,
        SH_C4 * (x * x - z * z),
    ]
}

// Right-hand rotation about +y, angle in radians.
pub fn rotate_yaw(direction: [f64; 3], yaw_radians: f64) -> [f64; 3] {
    let (s, c) = yaw_radians.sin_cos();
    let [x, y, z] = direction;
    [c * x + s * z, y, c * z - s * x]
}

#[derive(Clone, Debug)]
pub struct LightProbe {
    pub coefficients: [[f64; SH_BASES]; 3],
    pub yaw_degrees: f64,
    pub source_pfm: String,
    pub source_mask: String,
    pub width: usize,
    pub height: usize,
    pub valid_pixels: usize,
    pub skipped_pixels: usize,
}

impl LightProbe {
    // Batch diffuse query. Normals must be finite and non-zero (they are
    // normalized internally); albedo channels must lie in [0, 1]. Returns one
    // outgoing-radiance RGB triple per normal: albedo * irradiance / pi.
    // Negative irradiance is clamped to zero after evaluation; values above
    // 1 are preserved.
    pub fn evaluate(
        &self,
        normals: &[[f64; 3]],
        albedos: &[[f64; 3]],
    ) -> HdrResult<Vec<[f64; 3]>> {
        if normals.len() != albedos.len() {
            return Err(HdrError::Probe(format!(
                "normals/albedos length mismatch: {} vs {}",
                normals.len(),
                albedos.len()
            )));
        }
        let mut out = Vec::with_capacity(normals.len());
        for (i, (&n, &albedo)) in normals.iter().zip(albedos.iter()).enumerate() {
            let len2 = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
            if !len2.is_finite() || len2 <= 0.0 {
                return Err(HdrError::Probe(format!(
                    "normal {i} is not finite and non-zero: {n:?}"
                )));
            }
            for (c, &a) in albedo.iter().enumerate() {
                if !a.is_finite() || !(0.0..=1.0).contains(&a) {
                    return Err(HdrError::Probe(format!(
                        "albedo {i} channel {c} must be in [0, 1], got {a}"
                    )));
                }
            }
            let inv = 1.0 / len2.sqrt();
            let basis = eval_basis([n[0] * inv, n[1] * inv, n[2] * inv]);
            let mut rgb = [0.0f64; 3];
            for c in 0..3 {
                let mut irradiance = 0.0;
                for (k, &b) in basis.iter().enumerate() {
                    irradiance += CONVOLUTION_BANDS[band_of(k)] * self.coefficients[c][k] * b;
                }
                let irradiance = irradiance.max(0.0);
                rgb[c] = albedo[c] * irradiance / std::f64::consts::PI;
            }
            out.push(rgb);
        }
        Ok(out)
    }
}

#[derive(Serialize, Deserialize)]
struct ProbeJson {
    format: String,
    version: u32,
    coordinate_convention: String,
    basis_order: Vec<String>,
    basis_constants: serde_json::Value,
    convolution_per_band: Vec<f64>,
    yaw_degrees: f64,
    source: SourceJson,
    coefficients: CoeffJson,
}

#[derive(Serialize, Deserialize)]
struct SourceJson {
    pfm: String,
    mask: String,
    width: usize,
    height: usize,
    valid_pixels: usize,
    skipped_pixels: usize,
}

#[derive(Serialize, Deserialize)]
struct CoeffJson {
    r: Vec<f64>,
    g: Vec<f64>,
    b: Vec<f64>,
}

const PROBE_FORMAT: &str = "hdr_probe254.light_probe";
const CONVENTION: &str = "right-handed, +y up (polar axis); longitude increases from +x toward +z; source panorama is a 2:1 equirectangular map whose top row is the +y pole";

pub fn probe_to_json(probe: &LightProbe) -> serde_json::Value {
    let basis_constants = serde_json::json!({
        "C0": SH_C0, "C1": SH_C1, "C2": SH_C2, "C3": SH_C3, "C4": SH_C4,
        "definitions": [
            "Y00 = C0",
            "Y1-1 = C1*z", "Y10 = C1*y", "Y11 = C1*x",
            "Y2-2 = C2*x*z", "Y2-1 = C2*y*z", "Y20 = C3*(3*y^2-1)",
            "Y21 = C2*x*y", "Y22 = C4*(x^2-z^2)"
        ]
    });
    serde_json::to_value(ProbeJson {
        format: PROBE_FORMAT.to_string(),
        version: 1,
        coordinate_convention: CONVENTION.to_string(),
        basis_order: SH_BASIS_ORDER.iter().map(|s| s.to_string()).collect(),
        basis_constants,
        convolution_per_band: CONVOLUTION_BANDS.to_vec(),
        yaw_degrees: probe.yaw_degrees,
        source: SourceJson {
            pfm: probe.source_pfm.clone(),
            mask: probe.source_mask.clone(),
            width: probe.width,
            height: probe.height,
            valid_pixels: probe.valid_pixels,
            skipped_pixels: probe.skipped_pixels,
        },
        coefficients: CoeffJson {
            r: probe.coefficients[0].to_vec(),
            g: probe.coefficients[1].to_vec(),
            b: probe.coefficients[2].to_vec(),
        },
    })
    .expect("probe serialization is infallible")
}

pub fn save_probe(probe: &LightProbe, path: &Path) -> HdrResult<()> {
    let text = serde_json::to_string_pretty(&probe_to_json(probe))
        .map_err(|e| HdrError::Probe(format!("probe JSON encode failed: {e}")))?;
    crate::deliver::atomic_write(path, text.as_bytes())
}

pub fn load_probe(path: &Path) -> HdrResult<LightProbe> {
    let bytes = std::fs::read(path).map_err(|source| HdrError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: ProbeJson = serde_json::from_slice(&bytes).map_err(|e| HdrError::Json {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    if parsed.format != PROBE_FORMAT || parsed.version != 1 {
        return Err(HdrError::Probe(format!(
            "{}: unsupported probe format {} version {}",
            path.display(),
            parsed.format,
            parsed.version
        )));
    }
    let mut coefficients = [[0.0f64; SH_BASES]; 3];
    for (c, chan) in [&parsed.coefficients.r, &parsed.coefficients.g, &parsed.coefficients.b]
        .iter()
        .enumerate()
    {
        if chan.len() != SH_BASES || chan.iter().any(|v| !v.is_finite()) {
            return Err(HdrError::Probe(format!(
                "{}: channel {c} must hold {SH_BASES} finite coefficients",
                path.display()
            )));
        }
        coefficients[c].copy_from_slice(chan);
    }
    if !parsed.yaw_degrees.is_finite() {
        return Err(HdrError::Probe(format!(
            "{}: yaw_degrees must be finite",
            path.display()
        )));
    }
    Ok(LightProbe {
        coefficients,
        yaw_degrees: parsed.yaw_degrees,
        source_pfm: parsed.source.pfm,
        source_mask: parsed.source.mask,
        width: parsed.source.width,
        height: parsed.source.height,
        valid_pixels: parsed.source.valid_pixels,
        skipped_pixels: parsed.source.skipped_pixels,
    })
}
