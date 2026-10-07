# hdr_probe254

Debevec-style HDR reconstruction engine written from scratch in Rust (1.85.1),
using `nalgebra` 0.33.2 for the least-squares/SVD solve and `png` 0.17.16 for image I/O.
No HTTP, no frontend, no registration / de-ghosting / color management.
It also bakes 2:1 equirectangular HDR panoramas into order-2 spherical-harmonic
light probes and evaluates them for diffuse (Lambertian) shading.

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
   value is 0 in every exposure or 255 in every exposure.
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

## Light probes

`bake` reads the reconstruction's RGB32F PFM panorama plus its RGB validity
mask PNG and projects the radiance onto the 9 orthonormalized real spherical
harmonics of bands l = 0..=2, per channel (27 coefficients total).

- The panorama must be 2:1 equirectangular with each side at most 512 px.
  PFM rows are stored bottom-up and are restored to top-to-bottom order;
  the top row is the +y pole. Inputs are never modified.
- A pixel is used only when every mask channel is 255 **and** all three
  radiance channels are finite and non-negative; any invalid channel rejects
  the whole pixel. Rejected pixels are skipped in the integral, never
  filled with black.
- Directions are taken at pixel centers: latitude from +y (up) downward,
  longitude increasing from +x toward +z (right-handed). Each pixel is
  weighted by the exact solid angle of its lat/long cell boundaries
  (`d_lon * (sin lat_top - sin lat_bot)`); no equal-weight or
  luminance-based normalization is applied.
- An optional environment yaw (degrees, right-hand rule about +y) rotates
  the sampling directions before projection.

The probe is saved as reloadable JSON containing the 27 radiance
coefficients, the basis order and constants, the coordinate convention, the
Lambertian convolution weights, and the source panorama (path, size,
valid/skipped pixel counts, yaw).

`query` (or the library API `LightProbe::evaluate`) accepts a batch of
finite, non-zero world-space normals and RGB albedos in [0, 1]. Normals are
normalized; the coefficients are convolved per band with pi, 2pi/3 and
pi/4. Irradiance is clamped to zero only after evaluation, and outgoing
radiance is `albedo * irradiance / pi`; values above 1 are preserved.

Because the input radiance is on a relative scale (see above), probe
coefficients and queried radiances share that same relative scale. The
order-2 SH projection is a **low-frequency approximation**: it reproduces
the constant term exactly (up to lat/long quadrature error, ~5e-4 at
64x32) but blurs high-frequency lighting detail such as small bright
sources, which is the intended trade-off for diffuse irradiance.

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

# bake a light probe from a reconstructed 2:1 panorama and its mask
./target/release/hdr_probe254 bake panorama.pfm mask.png probe.json [yaw_deg]

# evaluate diffuse outgoing radiance for a normal and albedo
./target/release/hdr_probe254 query probe.json 0,1,0 0.8,0.8,0.8
```

The demo scene is a 160x80 (2:1) synthetic panorama spanning a wide
log-radiance range and includes a region so
bright it saturates even the longest exposure; those sun channels are
reported invalid in the mask (0) rather than guessed. On the reference
machine the self-test reconstructs non-saturated channels with a log-radiance
RMSE around 0.01 and `|g[128]|` around 5e-12.
The demo continues through the full example: it bakes `demo_probe.json`
from the reconstructed panorama and mask, reloads it, and prints diffuse
queries for a few normals. The self-test additionally checks that a
constant environment reproduces the analytic irradiance `pi * L` for
arbitrary yaw and that negative evaluated irradiance clamps to zero.

## Source layout

- `src/image.rs` - 8-bit RGB PNG decoding and mask encoding
- `src/job.rs` - job JSON parsing and input validation
- `src/response.rs` - sampling and Debevec joint solve with SVD rank check
- `src/fusion.rs` - weighted log-domain radiance fusion and validity mask
- `src/pfm.rs` - RGB32F PFM writer and reader
- `src/probe.rs` - real SH bases, probe JSON, batch diffuse evaluation
- `src/bake.rs` - panorama validation and SH projection with solid-angle weights
- `src/report.rs`, `src/deliver.rs` - JSON report, atomic staging
- `src/pipeline.rs` - end-to-end orchestration
- `src/demo.rs` - synthetic nonlinear-response example and self-test
- `src/main.rs` - CLI (`run`, `selftest`, `demo`, `bake`, `query`)
