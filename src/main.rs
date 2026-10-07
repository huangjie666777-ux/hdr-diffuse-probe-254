use std::path::PathBuf;

use hdr_probe254::demo::{run_selftest, write_demo_assets};
use hdr_probe254::job::load_job;
use hdr_probe254::pipeline::run;
use hdr_probe254::probe::LightProbe;
use hdr_probe254::probe_job::{load_bake_job, load_queries};

fn print_help() {
    println!("hdr_probe254 - Debevec HDR reconstruction");
    println!("usage:");
    println!("  hdr_probe254 run <job.json>      reconstruct from bracketed exposures");
    println!("  hdr_probe254 selftest            run in-memory synthetic self-test");
    println!("  hdr_probe254 demo [outdir]       write synthetic example and reconstruct it");
    println!("  hdr_probe254 bake <bake_job.json>  bake an SH light probe from a PFM panorama");
    println!("  hdr_probe254 query <probe.json> <queries.json> [out.json]");
    println!("                                   evaluate diffuse shading against a baked probe");
    println!("  hdr_probe254 probedemo [outdir]  panorama -> probe -> query example");
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
        "bake" => cmd_bake(args.get(2).map(PathBuf::from)),
        "query" => cmd_query(
            args.get(2).map(PathBuf::from),
            args.get(3).map(PathBuf::from),
            args.get(4).map(PathBuf::from),
        ),
        "probedemo" => {
            cmd_probedemo(args.get(2).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("probe_demo")))
        }
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

fn cmd_bake(job_path: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let path = job_path.ok_or(
        "missing bake job path; usage: hdr_probe254 bake <bake_job.json>" as &str,
    )?;
    let job = load_bake_job(&path)?;
    let probe = hdr_probe254::probe::bake(&job.pfm, &job.mask_png, job.yaw_degrees)?;
    probe.save(&job.output_json)?;
    println!(
        "baked {}x{} panorama ({}/{} valid pixels, yaw {} deg)",
        probe.source.width,
        probe.source.height,
        probe.source.valid_pixels,
        probe.source.total_pixels,
        job.yaw_degrees
    );
    println!("probe -> {}", job.output_json.display());
    Ok(())
}

fn cmd_query(
    probe_path: Option<PathBuf>,
    query_path: Option<PathBuf>,
    out_path: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let probe_path = probe_path
        .ok_or("missing probe path; usage: hdr_probe254 query <probe.json> <queries.json> [out.json]" as &str)?;
    let query_path = query_path
        .ok_or("missing query path; usage: hdr_probe254 query <probe.json> <queries.json> [out.json]" as &str)?;
    let probe = LightProbe::load(&probe_path)?;
    let queries = load_queries(&query_path)?;
    let results = probe.evaluate_batch(&queries)?;
    let json = serde_json::to_vec_pretty(&results)?;
    match out_path {
        Some(p) => {
            hdr_probe254::deliver::atomic_write(&p, &json)?;
            println!("{} results -> {}", results.len(), p.display());
        }
        None => println!("{}", String::from_utf8(json)?),
    }
    Ok(())
}

fn cmd_probedemo(dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let out = hdr_probe254::probe_demo::run_probe_demo(&dir)?;
    println!("panorama, probe and query results written under {}", dir.display());
    for r in &out.results {
        println!(
            "  normal [{:.2},{:.2},{:.2}] -> irradiance [{:.4},{:.4},{:.4}] radiance [{:.4},{:.4},{:.4}]",
            r.normal[0], r.normal[1], r.normal[2],
            r.irradiance[0], r.irradiance[1], r.irradiance[2],
            r.radiance[0], r.radiance[1], r.radiance[2],
        );
    }
    Ok(())
}
