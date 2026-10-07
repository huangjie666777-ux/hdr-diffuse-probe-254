use crate::error::HdrError;use crate::deliver::atomic_write;
use crate::error::HdrResult;
use crate::fusion::fuse;
use crate::image::{encode_rgb8_png, Frame};
use crate::job::Job;
use crate::pfm::write_pfm_rgb32;
use crate::report::{build_report, write_report};
use crate::response::recover_responses;

pub const MAX_SAMPLE_POINTS: usize = 128;

pub fn run(job: &Job) -> HdrResult<()> {
    for out in [&job.output_pfm, &job.mask_png, &job.report_json] {
        let _ = std::fs::remove_file(out);
    }
    let frames: &[Frame] = &job.frames;
    let responses = recover_responses(frames, job.width, job.height, MAX_SAMPLE_POINTS)?;
    let fused = fuse(frames, &responses)?;

    let pfm_tmp = job.output_pfm.with_extension("pfm.hdrf.tmp");
    let mask_tmp = job.mask_png.with_extension("png.hdrf.tmp");
    write_pfm_rgb32(&pfm_tmp, job.width, job.height, &fused.radiance)?;
    let png_bytes = encode_rgb8_png(job.width, job.height, &fused.mask)?;
    atomic_write(&mask_tmp, &png_bytes)?;
    let report = build_report(
        frames,
        job.width,
        job.height,
        &responses,
        &fused,
        &job.output_pfm,
        &job.mask_png,
    );
    let report_tmp = job.report_json.with_extension("json.hdrf.tmp");
    write_report(&report_tmp, &report)?;

    std::fs::rename(&pfm_tmp, &job.output_pfm).map_err(|source| HdrError::Io {
        path: job.output_pfm.clone(),
        source,
    })?;
    std::fs::rename(&mask_tmp, &job.mask_png).map_err(|source| HdrError::Io {
        path: job.mask_png.clone(),
        source,
    })?;
    std::fs::rename(&report_tmp, &job.report_json).map_err(|source| HdrError::Io {
        path: job.report_json.clone(),
        source,
    })?;
    Ok(())
}
