use std::path::PathBuf;

use hdr_probe254::bake::bake_probe;
use hdr_probe254::demo::{run_selftest, write_demo_assets};
use hdr_probe254::demo::run_probe_selftest;
use hdr_probe254::job::load_job;
use hdr_probe254::pipeline::run;
use hdr_probe254::probe::{load_probe, save_probe};

fn print_help() {
    println!("hdr_probe254 - Debevec HDR reconstruction");
    println!("usage:");
    println!("  hdr_probe254 run <job.json>      reconstruct from bracketed exposures");
    println!("  hdr_probe254 selftest            run in-memory synthetic self-test");
    println!("  hdr_probe254 demo [outdir]       write synthetic example and reconstruct it");
    println!("  hdr_probe254 bake <panorama.pfm> <mask.png> <probe.json> [yaw_deg]");
    println!("                                   bake an SH light probe from a 2:1 panorama");
    println!("  hdr_probe254 query <probe.json> <nx,ny,nz> [r,g,b]");
    println!("                                   evaluate diffuse outgoing radiance");
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
        "bake" => cmd_bake(&args[2..]),
        "query" => cmd_query(&args[2..]),
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
    let probe_report = run_probe_selftest()?;
    println!("probe selftest passed");
    println!("  constant-env max rel err : {:.3e}", probe_report.irradiance_max_rel_err);
    Ok(())
}

fn cmd_demo(dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let synth = write_demo_assets(&dir)?;
    let job_path = dir.join("job.json");
    let job = load_job(&job_path)?;
    run(&job)?;
    // Panorama -> probe -> query example on the reconstructed panorama.
    let probe_path = dir.join("demo_probe.json");
    let probe = bake_probe(&job.output_pfm, &job.mask_png, 0.0)?;
    save_probe(&probe, &probe_path)?;
    let reloaded = load_probe(&probe_path)?;
    let normals = [
        [0.0f64, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.3, -0.5, 0.8],
    ];
    let albedos = vec![[0.8f64, 0.8, 0.8]; normals.len()];
    let outgoing = reloaded.evaluate(&normals, &albedos)?;
    println!("probe -> {}", probe_path.display());
    println!("  valid pixels {} (skipped {})", probe.valid_pixels, probe.skipped_pixels);
    for (n, rgb) in normals.iter().zip(outgoing.iter()) {
        println!("  normal {:?} -> outgoing radiance {:?}", n, rgb);
    }
    println!("demo assets and reconstruction written under {}", dir.display());
    println!("frames:");
    for f in &synth.frames {
        println!("  {}  exposure={}s", f.path.display(), f.exposure_seconds);
    }
    cmd_selftest()?;
    Ok(())
}

fn cmd_bake(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 3 || args.len() > 4 {
        return Err(
            "usage: hdr_probe254 bake <panorama.pfm> <mask.png> <probe.json> [yaw_deg]".into(),
        );
    }
    let yaw: f64 = match args.get(3) {
        Some(s) => s.parse().map_err(|_| format!("invalid yaw degrees: {s}"))?,
        None => 0.0,
    };
    let probe = bake_probe(
        std::path::Path::new(&args[0]),
        std::path::Path::new(&args[1]),
        yaw,
    )?;
    save_probe(&probe, std::path::Path::new(&args[2]))?;
    println!("probe -> {}", args[2]);
    println!(
        "  {}x{} panorama, {} valid pixels ({} skipped), yaw {} deg",
        probe.width,
        probe.height,
        probe.valid_pixels,
        probe.skipped_pixels,
        probe.yaw_degrees
    );
    Ok(())
}

fn parse_vec3(text: &str, what: &str) -> Result<[f64; 3], Box<dyn std::error::Error>> {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 3 {
        return Err(format!("{what} must be comma-separated x,y,z, got '{text}'").into());
    }
    let mut out = [0.0f64; 3];
    for (i, p) in parts.iter().enumerate() {
        out[i] = p.trim().parse().map_err(|_| format!("invalid {what} component: {p}"))?;
    }
    Ok(out)
}

fn cmd_query(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 || args.len() > 3 {
        return Err(
            "usage: hdr_probe254 query <probe.json> <nx,ny,nz> [albedo_r,albedo_g,albedo_b]".into(),
        );
    }
    let probe = load_probe(std::path::Path::new(&args[0]))?;
    let normal = parse_vec3(&args[1], "normal")?;
    let albedo = match args.get(2) {
        Some(s) => parse_vec3(s, "albedo")?,
        None => [1.0, 1.0, 1.0],
    };
    let out = probe.evaluate(&[normal], &[albedo])?;
    println!(
        "outgoing radiance: [{:.6}, {:.6}, {:.6}]",
        out[0][0],
        out[0][1],
        out[0][2]
    );
    Ok(())
}
