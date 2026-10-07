use crate::error::{HdrError, HdrResult};
use crate::image::{Frame, Image8};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct FrameSpec {
    pub path: PathBuf,
    #[serde(alias = "exposure", alias = "exposure_secs")]
    pub exposure_seconds: f64,
}

#[derive(Debug, Deserialize)]
pub struct JobFile {
    pub frames: Vec<FrameSpec>,
    pub output_pfm: Option<PathBuf>,
    pub mask_png: Option<PathBuf>,
    pub report_json: Option<PathBuf>,
}

pub struct Job {
    pub frames: Vec<Frame>,
    pub width: usize,
    pub height: usize,
    pub output_pfm: PathBuf,
    pub mask_png: PathBuf,
    pub report_json: PathBuf,
}

pub fn load_job(job_path: &Path) -> HdrResult<Job> {
    let job_bytes = std::fs::read(job_path).map_err(|source| HdrError::Io {
        path: job_path.to_path_buf(),
        source,
    })?;
    let parsed: JobFile = serde_json::from_slice(&job_bytes).map_err(|e| HdrError::Json {
        path: job_path.to_path_buf(),
        message: e.to_string(),
    })?;

    if parsed.frames.len() < 3 || parsed.frames.len() > 8 {
        return Err(HdrError::Job(format!(
            "expected 3 to 8 frames, found {}",
            parsed.frames.len()
        )));
    }
    for spec in &parsed.frames {
        if !spec.exposure_seconds.is_finite() || spec.exposure_seconds <= 0.0 {
            return Err(HdrError::Job(format!(
                "frame {} has non-finite or non-positive exposure {}",
                spec.path.display(),
                spec.exposure_seconds
            )));
        }
    }
    let distinct: std::collections::HashSet<u64> = parsed
        .frames
        .iter()
        .map(|f| f.exposure_seconds.to_bits())
        .collect();
    if distinct.len() < 2 {
        return Err(HdrError::Job(
            "at least two distinct exposure values are required".to_string(),
        ));
    }

    let base_dir = job_path.parent().unwrap_or_else(|| Path::new("."));
    let resolve = |p: &Path| -> PathBuf {
        if p.is_absolute() { p.to_path_buf() } else { base_dir.join(p) }
    };

    let mut frames = Vec::with_capacity(parsed.frames.len());
    let mut width: Option<usize> = None;
    let mut height: Option<usize> = None;
    for spec in &parsed.frames {
        let full = resolve(&spec.path);
        let image = Image8::load(&full)?;
        if image.width > 512 || image.height > 512 {
            return Err(HdrError::Job(format!(
                "frame {} is {}x{}, each side must be at most 512",
                full.display(),
                image.width,
                image.height
            )));
        }
        match (width, height) {
            (Some(w), Some(h)) if w == image.width && h == image.height => {} 
            (None, None) => {
                width = Some(image.width);
                height = Some(image.height);
            }
            (Some(w), Some(h)) => {
                return Err(HdrError::Job(format!(
                    "frame {} is {}x{}, expected {}x{}",
                    full.display(), image.width, image.height, w, h
                )));
            }
            _ => unreachable!(),
        }
        frames.push(Frame {
            path: full,
            exposure_seconds: spec.exposure_seconds,
            image,
        });
    }

    let or_default = |opt: &Option<PathBuf>, name: &str| -> PathBuf {
        opt.as_ref().map(|p| resolve(p)).unwrap_or_else(|| base_dir.join(name))
    };
    let output_pfm = or_default(&parsed.output_pfm, "hdr_fusion_output.pfm");
    let mask_png = or_default(&parsed.mask_png, "hdr_fusion_mask.png");
    let report_json = or_default(&parsed.report_json, "hdr_fusion_report.json");

    let normalize = |p: &Path| -> PathBuf {
        std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
    };
    for out in [&output_pfm, &mask_png, &report_json] {
        for frame in &frames {
            if normalize(out) == normalize(&frame.path) {
                return Err(HdrError::Job(format!(
                    "output {} would overwrite input frame {}",
                    out.display(),
                    frame.path.display()
                )));
            }
        }
    }

    Ok(Job {
        frames,
        width: width.unwrap(),
        height: height.unwrap(),
        output_pfm,
        mask_png,
        report_json,
    })
}
