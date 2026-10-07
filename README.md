# hdr_probe254

Debevec-style HDR reconstruction engine written from scratch in Rust (1.85.1),
using `nalgebra` 0.33.2 for the least-squares/SVD solve and `png` 0.17.16 for image I/O.
No HTTP, no frontend, no registration / de-ghosting / color management.

## Input

A JSON job file lists 3 to 8 aligned, static 8-bit RGB PNGs of identical size
(each side <= 512 px) with finite positive exposure times in seconds, at least
two distinct values:

```json
{
  "frames": [
    { "path": "img_1_250s.png", "exposure_seconds": 0.004 },
    { "path": "img_1_60s.png",  "exposure_seconds": 0.0166667 },
    { "path": "img_1_15s.png",  "exposure_seconds": 0.0666667 },
    { "path": "img_1_4s.png",   "exposure_seconds": 0.25 },
    { "path": "img_1s.png",     "exposure_seconds": 1.0 }
  ],
  "output_pfm": "out.pfm",
  "mask_png": "mask.png",
  "report_json": "report.json"
}
```

Paths are resolved relative to the job file. Output paths are optional and
default to `hdr_fusion_output.pfm`, `hdr_fusion_mask.png` and
`hdr_fusion_report.json` next to the job file. Input images are never modified.

## Algorithm

1. **Sampling.** For each channel, at most 128 points are picked on a uniform
   2-D cell-centered grid covering the image. A point is discarded when its
   value is 0 in every exposure or 255 in every exposure, or when its values
   mix 0 and 255 across exposures (simultaneously clipped at both ends).
2. **Response recovery (Debevec & Malik).** The 256 log inverse-response
   entries `g[z]` and per-sample log radiances `ln E_i` are solved jointly:
   - data rows: `w(z) * (g[z] - ln E_i - ln dt) = 0`,
   - smoothness rows: `10 * w(z) * (g[z-1] - 2 g[z] + g[z+1]) = 0`,
   - anchor row: `g[128] = 0`,
   - with the triangle/hat weight `w(z) = min(z, 255 - z)`.
   The over-determined system is solved by SVD (`nalgebra`). Its numerical
   rank must equal the number of unknowns (rank threshold
   `max(m,n) * eps * sigma_max`); otherwise the material is not identifiable
   and the run fails with a rank error. No fixed gamma or built-in HDR
   function is used.
3. **Fusion.** Per pixel and channel, with the same hat weight,
   `ln E = sum w(z) (g[z] - ln dt) / sum w(z)`, then `E = exp(ln E)`.
   Results are not normalized to any maximum and are not clipped to [0, 1].
   When every exposure at a channel is 0/255 (`w = 0`), the value is written
   as 0 and flagged invalid in the mask. Any non-finite result is an error.

## Outputs

- RGB 32-bit little-endian floating point PFM (bottom-up rows, scale `-1.0`).
- Same-size RGB mask PNG: channel = 255 when valid, 0 when invalid.
- JSON report containing, for each channel: all 256 `g` values, the sample
  coordinates actually used, their fitted log radiances, SVD rank / singular
  values, the achieved weighted data residual RMS and maximum absolute
  residual, the smoothness residual RMS, per-channel radiance statistics,
  plus the exposure sources (paths, exposure seconds, log exposure).

Outputs are staged via temporary files and renamed into place together; a
failed reconstruction never leaves a partial delivery behind.

## Relative scale

The anchor `g[128] = 0` fixes the additive degree of freedom in log space, so
radiance is on a **relative** linear scale: multiplying all true radiances by
a constant `c` corresponds to shifting every exposure time by `1/c` and leaves
the 8-bit input unchanged. Ratios of reconstructed radiance values are
meaningful (and exposure-independent); absolute photometric units (cd/m^2)
are not recoverable without calibration.

## Light probes (SH baking and diffuse queries)

Reconstructed (or any) RGB32F PFM panoramas can be baked into a 9-term
order-2 real spherical-harmonics light probe for diffuse material shading.

### Baking

```sh
./target/release/hdr_probe254 bake bake_job.json
```

```json
{
  "pfm": "panorama.pfm",
  "mask_png": "panorama_mask.png",
  "yaw_degrees": 0.0,
  "output_json": "light_probe.json"
}
```

- The panorama must be 2:1 equirectangular, each side at most 512 px. PFM
  rows are stored bottom-up and are restored to top-to-bottom on read.
- Every radiance value must be finite and non-negative; a single invalid
  channel value aborts the bake. A pixel contributes only when all three
  mask channels are 255; masked-out pixels are skipped entirely, never
  filled with black. Input files are never modified.
- Each pixel is projected from its center direction (y-up sphere, longitude
  increasing from +x toward +z) onto the 9 orthonormal real SH basis
  functions, weighted by the exact solid angle of its longitude/latitude
  cell boundaries (no equal-weight or luminance normalization).
  `yaw_degrees` rotates the environment right-handed about +y.
- The probe JSON stores the 27 radiance coefficients (RGB x 9), the basis
  order and normalization constants, the band convolution weights, the
  coordinate convention, and the source image paths, and can be reloaded
  with `LightProbe::load`.

### Querying

```sh
./target/release/hdr_probe254 query light_probe.json queries.json [out.json]
```

```json
{ "queries": [
  { "normal": [0.0, 1.0, 0.0], "albedo": [0.8, 0.8, 0.8] }
] }
```

Normals must be finite and non-zero (they are normalized internally);
albedo channels must lie in [0, 1]. Irradiance is the SH reconstruction
convolved per band with pi, 2pi/3, pi/4 (Ramamoorthi-Hanrahan Lambertian
kernel); only the final irradiance is clamped at 0. Outgoing radiance is
`albedo * irradiance / pi` and is **not** clamped above, so values > 1 are
preserved. The library entry point is `LightProbe::evaluate_batch`.

### Low-frequency approximation

An order-2 SH probe captures only the 0th/1st/2nd frequency bands of the
environment. That is a good model for smooth sky/ambient light, but sharp
features (sun disks, small bright windows) are spread into the low bands:
expect soft, directionally smooth irradiance and some ringing, not hard
shadows or specular detail. Coefficients inherit the **relative** radiance
scale of the reconstruction, so irradiance/radiance outputs share that
unknown global factor.

### Panorama-to-probe example

```sh
./target/release/hdr_probe254 probedemo [outdir]
```

Writes a synthetic sky+sun equirect `panorama.pfm` plus mask, bakes
`probe.json`, and evaluates a few normal/albedo queries into
`query_results.json`.

## Usage

```sh
cargo build --release

# reconstruct a bracketed exposure set described by job.json
./target/release/hdr_probe254 run path/to/job.json

# in-memory self-test against a synthetic scene with a known nonlinear
# (tanh-based, non-gamma) response curve
./target/release/hdr_probe254 selftest

# write the synthetic exposures + job into ./demo and reconstruct them
./target/release/hdr_probe254 demo [outdir]

# bake an SH light probe from a 2:1 RGB32F PFM panorama + RGB validity mask
./target/release/hdr_probe254 bake bake_job.json

# evaluate diffuse shading for a batch of normal/albedo queries
./target/release/hdr_probe254 query light_probe.json queries.json [out.json]

# panorama -> probe -> query example
./target/release/hdr_probe254 probedemo [outdir]
```

The demo scene spans a wide log-radiance range and includes a region so
bright it saturates even the longest exposure; those sun channels are
reported invalid in the mask (0) rather than guessed. On the reference
machine the self-test reconstructs non-saturated channels with a log-radiance
RMSE around 0.01 and `|g[128]|` around 5e-12.

## Source layout

- `src/image.rs` - 8-bit RGB PNG decoding and mask encoding
- `src/job.rs` - job JSON parsing and input validation
- `src/response.rs` - sampling and Debevec joint solve with SVD rank check
- `src/fusion.rs` - weighted log-domain radiance fusion and validity mask
- `src/pfm.rs` - RGB32F PFM writer and reader
- `src/report.rs`, `src/deliver.rs` - JSON report, atomic staging
- `src/pipeline.rs` - end-to-end orchestration
- `src/sh.rs` - orthonormal real SH basis (order <= 2) and band constants
- `src/probe.rs` - probe baking, probe JSON load/save, batch shading queries
- `src/probe_job.rs` - bake-job and query-file JSON parsing
- `src/probe_demo.rs` - synthetic panorama -> probe -> query example
- `src/demo.rs` - synthetic nonlinear-response example and self-test
- `src/main.rs` - CLI (`run`, `selftest`, `demo`, `bake`, `query`, `probedemo`)
