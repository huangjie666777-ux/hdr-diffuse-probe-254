use crate::error::{HdrError, HdrResult};
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

pub fn read_pfm_rgb32(path: &std::path::Path) -> HdrResult<(usize, usize, Vec<f32>)> {
    let bytes = std::fs::read(path).map_err(|source| HdrError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let bad = || HdrError::Pfm {
        path: path.to_path_buf(),
        message: "malformed PFM header".to_string(),
    };
    let mut pos = 0usize;
    let mut tokens: Vec<String> = Vec::new();
    while tokens.len() < 4 {
        while pos < bytes.len() && (bytes[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            return Err(bad());
        }
        if bytes[pos] == b'#' {
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }
        let start = pos;
        while pos < bytes.len() && !(bytes[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        tokens.push(String::from_utf8_lossy(&bytes[start..pos]).into_owned());
    }
    if pos < bytes.len() {
        pos += 1; // single whitespace after the scale token
    }
    if tokens[0] != "PF" {
        return Err(HdrError::Pfm {
            path: path.to_path_buf(),
            message: format!("expected color PFM (PF), got {}", tokens[0]),
        });
    }
    let width: usize = tokens[1].parse().map_err(|_| bad())?;
    let height: usize = tokens[2].parse().map_err(|_| bad())?;
    let scale: f64 = tokens[3].parse().map_err(|_| bad())?;
    if width == 0 || height == 0 || !scale.is_finite() || scale == 0.0 {
        return Err(bad());
    }
    let count = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(3))
        .ok_or_else(bad)?;
    let need = count.checked_mul(4).ok_or_else(bad)?;
    if bytes.len() - pos < need {
        return Err(HdrError::Pfm {
            path: path.to_path_buf(),
            message: format!("truncated pixel data: need {need} bytes, have {}", bytes.len() - pos),
        });
    }
    let little_endian = scale < 0.0;
    let mut rgb = vec![0.0f32; count];
    // PFM stores rows bottom-up; restore to top-to-bottom order.
    for y in 0..height {
        let src_row = height - 1 - y;
        for x in 0..width {
            for c in 0..3 {
                let i = pos + ((src_row * width + x) * 3 + c) * 4;
                let b: [u8; 4] = bytes[i..i + 4].try_into().unwrap();
                let v = if little_endian {
                    f32::from_le_bytes(b)
                } else {
                    f32::from_be_bytes(b)
                };
                rgb[(y * width + x) * 3 + c] = v;
            }
        }
    }
    Ok((width, height, rgb))
}
