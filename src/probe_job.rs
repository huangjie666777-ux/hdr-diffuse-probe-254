//! JSON job parsing for probe baking and batch shading queries.

use crate::error::{HdrError, HdrResult};
use crate::probe::ShadingQuery;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct BakeJobFile {
    pub pfm: PathBuf,
    pub mask_png: PathBuf,
    pub yaw_degrees: Option<f64>,
    pub output_json: Option<PathBuf>,
}

pub struct BakeJob {
    pub pfm: PathBuf,
    pub mask_png: PathBuf,
    pub yaw_degrees: f64,
    pub output_json: PathBuf,
}

pub fn load_bake_job(job_path: &Path) -> HdrResult<BakeJob> {
    let bytes = std::fs::read(job_path).map_err(|source| HdrError::Io {
        path: job_path.to_path_buf(),
        source,
    })?;
    let parsed: BakeJobFile = serde_json::from_slice(&bytes).map_err(|e| HdrError::Json {
        path: job_path.to_path_buf(),
        message: e.to_string(),
    })?;
    let base_dir = job_path.parent().unwrap_or_else(|| Path::new("."));
    let resolve = |p: &Path| -> PathBuf {
        if p.is_absolute() { p.to_path_buf() } else { base_dir.join(p) }
    };
    let yaw = parsed.yaw_degrees.unwrap_or(0.0);
    if !yaw.is_finite() {
        return Err(HdrError::Job(format!("yaw_degrees must be finite, got {yaw}")));
    }
    Ok(BakeJob {
        pfm: resolve(&parsed.pfm),
        mask_png: resolve(&parsed.mask_png),
        yaw_degrees: yaw,
        output_json: parsed
            .output_json
            .map(|p| resolve(&p))
            .unwrap_or_else(|| base_dir.join("light_probe.json")),
    })
}

#[derive(Debug, Deserialize)]
pub struct QueryFile {
    pub queries: Vec<ShadingQuery>,
}

pub fn load_queries(path: &Path) -> HdrResult<Vec<ShadingQuery>> {
    let bytes = std::fs::read(path).map_err(|source| HdrError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: QueryFile = serde_json::from_slice(&bytes).map_err(|e| HdrError::Json {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    if parsed.queries.is_empty() {
        return Err(HdrError::Job("query file contains no queries".to_string()));
    }
    Ok(parsed.queries)
}
