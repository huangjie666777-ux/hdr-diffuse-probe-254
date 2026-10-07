use crate::error::{HdrError, HdrResult};
use std::path::Path;
use std::io::Write;

pub fn write_pfm_rgb32(path: &std::path::Path, width: usize, height: usize, rgb: &[f32]) -> HdrResult<()> {
    let mut file = std::fs::File::create(path).map_err(|source| HdrError::Io {
        path: path.to_path_buf(), source,
    })?;
    let header = format!("PF\n{} {}\n-1.0\n", width, height);
    file.write_all(header.as_bytes()).map_err(|source| HdrError::Io {
        path: path.to_path_buf(), source,
    })?;
    let mut row = Vec::with_capacity(width * 3 * 4);
    for y in (0..height).rev() {
        row.clear();
        for x in 0..width {
            let i = (y * width + x) * 3;
            for c in 0..3 {
                row.extend_from_slice(&rgb[i + c].to_le_bytes());
            }
        }
        file.write_all(&row).map_err(|source| HdrError::Io {
            path: path.to_path_buf(), source,
        })?;
    }
    Ok(())
}

fn pfm_err(path: &Path, message: impl Into<String>) -> HdrError {
    HdrError::Pfm { path: path.to_path_buf(), message: message.into() }
}

/// Read an RGB 32-bit float PFM ("PF", little-endian scale < 0).
/// PFM stores rows bottom-up; the returned buffer is restored to
/// top-to-bottom row order, 3 floats per pixel.
pub fn read_pfm_rgb32(path: &Path) -> HdrResult<(usize, usize, Vec<f32>)> {
    let bytes = std::fs::read(path).map_err(|source| HdrError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut pos = 0usize;
    let mut next_token = || -> HdrResult<String> {
        loop {
            while pos < bytes.len() && (bytes[pos] as char).is_ascii_whitespace() {
                pos += 1;
            }
            if pos < bytes.len() && bytes[pos] == b'#' {
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
                continue;
            }
            break;
        }
        let start = pos;
        while pos < bytes.len() && !(bytes[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        if start == pos {
            return Err(pfm_err(path, "unexpected end of header"));
        }
        String::from_utf8(bytes[start..pos].to_vec())
            .map_err(|_| pfm_err(path, "header is not valid ASCII"))
    };

    let magic = next_token()?;
    if magic != "PF" {
        return Err(pfm_err(path, format!("expected magic PF, found {magic:?}")));
    }
    let width: usize = next_token()?
        .parse()
        .map_err(|_| pfm_err(path, "invalid width"))?;
    let height: usize = next_token()?
        .parse()
        .map_err(|_| pfm_err(path, "invalid height"))?;
    let scale: f64 = next_token()?
        .parse()
        .map_err(|_| pfm_err(path, "invalid scale"))?;
    if width == 0 || height == 0 {
        return Err(pfm_err(path, "zero-sized image"));
    }
    if scale >= 0.0 || !scale.is_finite() {
        return Err(pfm_err(
            path,
            format!("only little-endian PFM (negative scale) is supported, got {scale}"),
        ));
    }
    // exactly one whitespace byte separates the header from the raster
    pos += 1;
    let need = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(3 * 4))
        .ok_or_else(|| pfm_err(path, "image dimensions overflow"))?;
    let raster = bytes
        .get(pos..pos + need)
        .ok_or_else(|| pfm_err(path, "truncated raster data"))?;

    let mut rgb = vec![0.0f32; width * height * 3];
    for y_file in 0..height {
        let y = height - 1 - y_file;
        for x in 0..width {
            let src = (y_file * width + x) * 3 * 4;
            let dst = (y * width + x) * 3;
            for c in 0..3 {
                let b = &raster[src + c * 4..src + c * 4 + 4];
                rgb[dst + c] = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
        }
    }
    Ok((width, height, rgb))
}
