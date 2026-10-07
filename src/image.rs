use crate::error::{HdrError, HdrResult};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Image8 {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<[u8; 3]>,
}

impl Image8 {
    pub fn load(path: &Path) -> HdrResult<Image8> {
        let bytes = std::fs::read(path).map_err(|source| HdrError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().map_err(|e| HdrError::PngDecode {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| HdrError::PngDecode {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        if info.bit_depth != png::BitDepth::Eight || info.color_type != png::ColorType::Rgb {
            return Err(HdrError::PngDecode {
                path: path.to_path_buf(),
                message: format!("expected 8-bit RGB, got {:?}/{:?}", info.bit_depth, info.color_type),
            });
        }
        let width = info.width as usize;
        let height = info.height as usize;
        let bytes = &buf[..info.buffer_size()];
        if bytes.len() != width * height * 3 {
            return Err(HdrError::PngDecode {
                path: path.to_path_buf(),
                message: format!("unexpected byte count {}", bytes.len()),
            });
        }
        let mut rgb = Vec::with_capacity(width * height);
        for px in bytes.chunks_exact(3) {
            rgb.push([px[0], px[1], px[2]]);
        }
        Ok(Image8 { width, height, rgb })
    }

    #[inline]
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        self.rgb[y * self.width + x]
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub path: PathBuf,
    pub exposure_seconds: f64,
    pub image: Image8,
}

pub fn encode_rgb8_png(width: usize, height: usize, rgb: &[[u8; 3]]) -> HdrResult<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| HdrError::PngEncode(e.to_string()))?;
        let flat: Vec<u8> = rgb.iter().flat_map(|p| p.iter().copied()).collect();
        writer
            .write_image_data(&flat)
            .map_err(|e| HdrError::PngEncode(e.to_string()))?;
    }
    Ok(out)
}
