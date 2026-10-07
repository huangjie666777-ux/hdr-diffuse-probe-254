//! Real, orthonormal spherical harmonics up to order 2 (9 basis functions).
//!
//! Coordinate convention: the sphere is y-up; longitude phi increases from
//! +x toward +z. A unit direction is
//! d = (sin t cos p, cos t, sin t sin p) with colatitude t measured from
//! +y and longitude p in [0, 2pi).
//!
//! Basis order (l, m), m ascending within each band:
//!   0: (0, 0)
//!   1: (1,-1) ~ z, 2: (1,0) ~ y, 3: (1,1) ~ x
//!   4: (2,-2) ~ xz, 5: (2,-1) ~ yz, 6: (2,0) ~ 3y^2-1,
//!   7: (2, 1) ~ xy, 8: (2,2) ~ x^2-z^2

use std::f64::consts::PI;

pub const NUM_BASIS: usize = 9;

pub const BASIS_ORDER: [&str; NUM_BASIS] = [
    "(0,0)", "(1,-1)", "(1,0)", "(1,1)", "(2,-2)", "(2,-1)", "(2,0)", "(2,1)", "(2,2)",
];

/// Normalization constant of each basis function, in BASIS_ORDER order.
pub const BASIS_CONSTANTS: [f64; NUM_BASIS] = [
    0.28209479177387814, // 1 / (2 sqrt(pi))
    0.4886025119029199,  // sqrt(3 / (4 pi))
    0.4886025119029199,
    0.4886025119029199,
    1.0925484305920792,  // sqrt(15 / pi) / 2
    1.0925484305920792,
    0.31539156525352005, // sqrt(5 / pi) / 4
    1.0925484305920792,
    0.5462742152960396,  // sqrt(15 / pi) / 4
];

/// Frequency band (l) of each basis index.
pub const BAND_OF_INDEX: [usize; NUM_BASIS] = [0, 1, 1, 1, 2, 2, 2, 2, 2];

/// Lambertian convolution weight per band: pi, 2pi/3, pi/4.
pub const BAND_CONVOLUTION: [f64; 3] = [PI, 2.0 * PI / 3.0, PI / 4.0];

/// Evaluate all 9 basis functions at a unit direction [x, y, z].
pub fn eval(d: &[f64; 3]) -> [f64; NUM_BASIS] {
    let (x, y, z) = (d[0], d[1], d[2]);
    [
        BASIS_CONSTANTS[0],
        BASIS_CONSTANTS[1] * z,
        BASIS_CONSTANTS[2] * y,
        BASIS_CONSTANTS[3] * x,
        BASIS_CONSTANTS[4] * x * z,
        BASIS_CONSTANTS[5] * y * z,
        BASIS_CONSTANTS[6] * (3.0 * y * y - 1.0),
        BASIS_CONSTANTS[7] * x * y,
        BASIS_CONSTANTS[8] * (x * x - z * z),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_is_orthonormal_on_sphere_grid() {
        // Numerically integrate Y_i * Y_j over a fine equirect grid.
        let w = 360usize;
        let h = 180usize;
        let mut gram = [[0.0f64; NUM_BASIS]; NUM_BASIS];
        for iy in 0..h {
            let t0 = iy as f64 / h as f64 * PI;
            let t1 = (iy + 1) as f64 / h as f64 * PI;
            let w_lat = t0.cos() - t1.cos();
            let t = (iy as f64 + 0.5) / h as f64 * PI;
            for ix in 0..w {
                let p = (ix as f64 + 0.5) / w as f64 * 2.0 * PI;
                let d = [t.sin() * p.cos(), t.cos(), t.sin() * p.sin()];
                let y = eval(&d);
                let dw = w_lat * (2.0 * PI / w as f64);
                for i in 0..NUM_BASIS {
                    for j in 0..NUM_BASIS {
                        gram[i][j] += y[i] * y[j] * dw;
                    }
                }
            }
        }
        for i in 0..NUM_BASIS {
            for j in 0..NUM_BASIS {
                let expect = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (gram[i][j] - expect).abs() < 1e-3,
                    "gram[{i}][{j}] = {}",
                    gram[i][j]
                );
            }
        }
    }
}
