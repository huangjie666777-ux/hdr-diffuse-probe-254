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
