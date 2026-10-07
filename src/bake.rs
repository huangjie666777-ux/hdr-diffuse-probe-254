use crate::error::{HdrError, HdrResult};
use crate::image::Image8;
use crate::pfm::read_pfm_rgb32;
use crate::probe::{eval_basis, rotate_yaw, LightProbe, SH_BASES};
use std::path::Path;

pub const MAX_SIDE: usize = 512;

// Project a 2:1 equirectangular RGB panorama (top-to-bottom rows) onto the
// 9 real SH bases per channel. `valid` marks pixels with usable radiance;
// invalid pixels are skipped entirely (never filled with black). Pixel
// weights are the exact solid angles of the lat/long cell boundaries.
pub fn project_sh(
    width: usize,
    height: usize,
    radiance: &[f32],
    valid: &[bool],
    yaw_degrees: f64,
) -> HdrResult<([[f64; SH_BASES]; 3], usize)> {
    if width != 2 * height || height == 0 {
        return Err(HdrError::Probe(format!(
            "panorama must be 2:1 equirectangular, got {width}x{height}"
        )));
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(HdrError::Probe(format!(
            "panorama {width}x{height} exceeds the {MAX_SIDE}px per-side limit"
        )));
    }
    if radiance.len() != width * height * 3 || valid.len() != width * height {
        return Err(HdrError::Probe(
            "radiance/validity buffer size mismatch".to_string(),
        ));
    }
    if !yaw_degrees.is_finite() {
        return Err(HdrError::Probe(format!(
            "yaw must be finite degrees, got {yaw_degrees}"
        )));
    }
    let yaw = yaw_degrees.to_radians();
    let d_lon = 2.0 * std::f64::consts::PI / width as f64;
    let mut coeffs = [[0.0f64; SH_BASES]; 3];
    let mut used = 0usize;
    for y in 0..height {
        // Latitude of the pixel center and of the cell boundaries.
        let lat_top = std::f64::consts::FRAC_PI_2
            - (y as f64) * std::f64::consts::PI / height as f64;
        let lat_bot = std::f64::consts::FRAC_PI_2
            - (y as f64 + 1.0) * std::f64::consts::PI / height as f64;
        let lat = std::f64::consts::FRAC_PI_2
            - (y as f64 + 0.5) * std::f64::consts::PI / height as f64;
        let weight = d_lon * (lat_top.sin() - lat_bot.sin());
        let (sin_lat, cos_lat) = lat.sin_cos();
        for x in 0..width {
            let idx = y * width + x;
            if !valid[idx] {
                continue;
            }
            let lon = (x as f64 + 0.5) * d_lon;
            let (sin_lon, cos_lon) = lon.sin_cos();
            let dir = rotate_yaw([cos_lat * cos_lon, sin_lat, cos_lat * sin_lon], yaw);
            let basis = eval_basis(dir);
            for c in 0..3 {
                let l = radiance[idx * 3 + c] as f64;
                for (k, &b) in basis.iter().enumerate() {
                    coeffs[c][k] += weight * l * b;
                }
            }
            used += 1;
        }
    }
    if used == 0 {
        return Err(HdrError::Probe(
            "panorama has no valid pixels to project".to_string(),
        ));
    }
    Ok((coeffs, used))
}

// Bake a light probe from a reconstructed RGB32F PFM panorama and its RGB
// validity mask PNG. A pixel is usable only when every mask channel is 255
// and all three radiance channels are finite and non-negative; any channel
// invalid rejects the whole pixel. Inputs are never modified.
pub fn bake_probe(pfm_path: &Path, mask_path: &Path, yaw_degrees: f64) -> HdrResult<LightProbe> {
    let (width, height, radiance) = read_pfm_rgb32(pfm_path)?;
    if width != 2 * height {
        return Err(HdrError::Probe(format!(
            "{}: expected a 2:1 equirectangular panorama, got {width}x{height}",
            pfm_path.display()
        )));
    }
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(HdrError::Probe(format!(
            "{}: {width}x{height} exceeds the {MAX_SIDE}px per-side limit",
            pfm_path.display()
        )));
    }
    let mask = Image8::load(mask_path)?;
    if mask.width != width || mask.height != height {
        return Err(HdrError::Probe(format!(
            "{}: mask is {}x{}, expected {width}x{height}",
            mask_path.display(),
            mask.width,
            mask.height
        )));
    }
    let mut valid = vec![false; width * height];
    for (i, px) in mask.rgb.iter().enumerate() {
        let mask_ok = px.iter().all(|&m| m == 255);
        let rad_ok = radiance[i * 3..i * 3 + 3]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0);
        valid[i] = mask_ok && rad_ok;
    }
    let (coefficients, used) = project_sh(width, height, &radiance, &valid, yaw_degrees)?;
    Ok(LightProbe {
        coefficients,
        yaw_degrees,
        source_pfm: pfm_path.to_string_lossy().into_owned(),
        source_mask: mask_path.to_string_lossy().into_owned(),
        width,
        height,
        valid_pixels: used,
        skipped_pixels: width * height - used,
    })
}
