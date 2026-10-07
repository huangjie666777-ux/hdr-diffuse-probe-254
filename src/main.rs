use std::path::PathBuf;

use hdr_probe254::demo::{run_selftest, write_demo_assets};
use hdr_probe254::job::load_job;
use hdr_probe254::pipeline::run;

fn print_help() {
    println!("hdr_probe254 - Debevec HDR reconstruction");
    println!("usage:");
    println!("  hdr_probe254 run <job.json>      reconstruct from bracketed exposures");
    println!("  hdr_probe254 selftest            run in-memory synthetic self-test");
    println!("  hdr_probe254 demo [outdir]       write synthetic example and reconstruct it");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_help();
        std::process::exit(2);
    }
    let code = match args[1].as_str() {
        "run" => cmd_run(args.get(2).map(PathBuf::from)),
        "selftest" => cmd_selftest(),
        "demo" => cmd_demo(args.get(2).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("demo"))),
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            std::process::exit(2);
        }
    };
    match code {
        Ok(()) => {} 
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    }
}

fn cmd_run(job_path: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let path = job_path.ok_or(
        "missing job.json path; usage: hdr_probe254 run <job.json>" as &str,
    )?;
    let job = load_job(&path)?;
    println!(
        "loaded {} frames, {}x{}",
        job.frames.len(),
        job.width,
        job.height
    );
    run(&job)?;
    println!("PFM   -> {}", job.output_pfm.display());
    println!("mask  -> {}", job.mask_png.display());
    println!("report-> {}", job.report_json.display());
    Ok(())
}

fn cmd_selftest() -> Result<(), Box<dyn std::error::Error>> {
    let report = run_selftest()?;
    println!("selftest passed");
    println!("  compared channel samples : {}", report.compared_pixels);
    println!("  log-radiance RMSE        : {:.6}", report.log_rmse);
    println!("  log-radiance max |error| : {:.6}", report.log_max_abs);
    println!("  |g[128]|                  : {:.3e}", report.g_anchor_residual);
    println!("  saturated sun samples invalid (channels): {}", report.invalid_sun_pixels);
    Ok(())
}

fn cmd_demo(dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let synth = write_demo_assets(&dir)?;
    let job_path = dir.join("job.json");
    let job = load_job(&job_path)?;
    run(&job)?;
    println!("demo assets and reconstruction written under {}", dir.display());
    println!("frames:");
    for f in &synth.frames {
        println!("  {}  exposure={}s", f.path.display(), f.exposure_seconds);
    }
    cmd_selftest()?;
    Ok(())
}
