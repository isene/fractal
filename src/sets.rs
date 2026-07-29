//! The maths. Nothing here knows about a terminal.

use std::sync::OnceLock;

/// How far z has to run before we call it gone. Larger than the usual 2,
/// because the smooth iteration count is only smooth once |z| is well
/// past the escape radius.
const BAILOUT: f64 = 65536.0;

/// Iterate z → z² + c and report how long it took to escape, as a
/// fractional count. `None` means it never did within `max`.
///
/// The fraction is what keeps the bands from stepping: subtracting
/// log₂(log|z|) removes the jump between one iteration and the next.
pub fn escape(mut zx: f64, mut zy: f64, cx: f64, cy: f64, max: u32) -> Option<f64> {
    for i in 0..max {
        let (x2, y2) = (zx * zx, zy * zy);
        if x2 + y2 > BAILOUT {
            let mag = 0.5 * (x2 + y2).ln();
            return Some(i as f64 + 1.0 - mag.ln() / std::f64::consts::LN_2);
        }
        zy = 2.0 * zx * zy + cy;
        zx = x2 - y2 + cx;
    }
    None
}

/// The Mandelbrot set: the c for which z → z² + c, started at zero,
/// stays bounded.
pub fn mandelbrot(cx: f64, cy: f64, max: u32) -> Option<f64> {
    // The main cardioid and the big bulb to its left are known to be in
    // the set. Testing them outright saves iterating to the cap over the
    // largest part of the picture.
    let q = (cx - 0.25) * (cx - 0.25) + cy * cy;
    if q * (q + (cx - 0.25)) <= 0.25 * cy * cy {
        return None;
    }
    if (cx + 1.0) * (cx + 1.0) + cy * cy <= 0.0625 {
        return None;
    }
    escape(0.0, 0.0, cx, cy, max)
}

/// A Julia set: the same iteration with c held fixed and the starting
/// point varied. Every point of the Mandelbrot set names one.
pub fn julia(zx: f64, zy: f64, cx: f64, cy: f64, max: u32) -> Option<f64> {
    escape(zx, zy, cx, cy, max)
}

/// x → r·x·(1 − x), the map that taught everyone what chaos looks like.
pub fn logistic(r: f64, x: f64) -> f64 {
    r * x * (1.0 - x)
}

/// Where the map settles for one r: `keep` values, after `skip` have
/// been thrown away to let the transient die.
pub fn logistic_orbit(r: f64, skip: u32, keep: u32) -> Vec<f64> {
    // Not ½: at r = 4 that point lands on 1, then on 0, and sits there.
    let mut x = 0.4;
    for _ in 0..skip {
        x = logistic(r, x);
    }
    (0..keep)
        .map(|_| {
            x = logistic(r, x);
            x
        })
        .collect()
}

/// The r at which the cycle of length 2ⁿ is superstable, meaning it
/// passes exactly through x = ½. These sit inside the windows the
/// period-doubling cascade opens, and their spacing gives Feigenbaum's
/// constant.
///
/// Found by walking up from the previous one until f^(2ⁿ)(½) − ½ changes
/// sign, then bisecting.
pub fn superstable() -> &'static Vec<f64> {
    static R: OnceLock<Vec<f64>> = OnceLock::new();
    R.get_or_init(|| {
        // Period 1 is superstable at r = 2: r·½·½ = ½.
        let mut out = vec![2.0];
        let mut gap = 1.0;
        for n in 1..10 {
            let g = |r: f64| {
                let mut x = 0.5;
                for _ in 0..(1u32 << n) {
                    x = logistic(r, x);
                }
                x - 0.5
            };
            let step = gap / 50.0;
            // Start clear of the previous root: r is a root of this one
            // too, since a 2ⁿ⁻¹-cycle repeats after 2ⁿ steps as well.
            let mut r = out[out.len() - 1] + step * 0.5;
            let mut prev = g(r);
            let mut found = None;
            for _ in 0..2000 {
                let r2 = r + step;
                let cur = g(r2);
                if (prev < 0.0) != (cur < 0.0) {
                    let (mut lo, mut hi) = (r, r2);
                    for _ in 0..80 {
                        let mid = 0.5 * (lo + hi);
                        if (g(lo) < 0.0) != (g(mid) < 0.0) {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                    }
                    found = Some(0.5 * (lo + hi));
                    break;
                }
                r = r2;
                prev = cur;
            }
            match found {
                Some(f) => {
                    gap = f - out[out.len() - 1];
                    out.push(f);
                }
                None => break,
            }
        }
        out
    })
}

/// Feigenbaum's δ, as this program works it out: the ratio of one gap in
/// the cascade to the next. It converges on 4.669201…, a number that
/// turns up in every system that doubles its way into chaos.
pub fn feigenbaum() -> f64 {
    let r = superstable();
    let n = r.len();
    (r[n - 2] - r[n - 3]) / (r[n - 1] - r[n - 2])
}

/// Where the doubling runs out and chaos starts, near 3.5699456.
pub fn accumulation() -> f64 {
    let r = superstable();
    // Extrapolate the geometric tail past the last gap we resolved.
    let n = r.len();
    let gap = r[n - 1] - r[n - 2];
    let d = feigenbaum();
    r[n - 1] + gap / (d - 1.0)
}

/// The Lorenz system, integrated with fourth-order Runge-Kutta. σ = 10
/// and β = 8/3 are Lorenz's own; ρ is the knob. Below about 24.74 the
/// trajectory settles onto one wing, above it never settles at all.
pub fn lorenz(rho: f64, steps: usize, dt: f64) -> Vec<(f64, f64, f64)> {
    const SIGMA: f64 = 10.0;
    const BETA: f64 = 8.0 / 3.0;
    let d = |p: (f64, f64, f64)| {
        (
            SIGMA * (p.1 - p.0),
            p.0 * (rho - p.2) - p.1,
            p.0 * p.1 - BETA * p.2,
        )
    };
    let add = |p: (f64, f64, f64), k: (f64, f64, f64), s: f64| {
        (p.0 + k.0 * s, p.1 + k.1 * s, p.2 + k.2 * s)
    };
    let mut p = (0.1, 0.0, 0.0);
    let mut out = Vec::with_capacity(steps);
    for i in 0..steps {
        let k1 = d(p);
        let k2 = d(add(p, k1, dt / 2.0));
        let k3 = d(add(p, k2, dt / 2.0));
        let k4 = d(add(p, k3, dt));
        p = (
            p.0 + dt / 6.0 * (k1.0 + 2.0 * k2.0 + 2.0 * k3.0 + k4.0),
            p.1 + dt / 6.0 * (k1.1 + 2.0 * k2.1 + 2.0 * k3.1 + k4.1),
            p.2 + dt / 6.0 * (k1.2 + 2.0 * k2.2 + 2.0 * k3.2 + k4.2),
        );
        // Let the transient fall onto the attractor before recording.
        if i > 400 {
            out.push(p);
        }
    }
    out
}

/// The Hénon map, the two-line system that folds a plane onto a fractal.
/// a = 1.4, b = 0.3 are the values Hénon used.
pub fn henon(a: f64, b: f64, n: usize) -> Vec<(f64, f64)> {
    let (mut x, mut y) = (0.0, 0.0);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let nx = 1.0 - a * x * x + y;
        y = b * x;
        x = nx;
        if !x.is_finite() || x.abs() > 1e6 {
            break; // outside its basin: the orbit runs away
        }
        if i > 100 {
            out.push((x, y));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classic membership checks: the origin and −1 are in the set,
    /// 1 and 0.4+0.6i are not.
    #[test]
    fn the_set_knows_its_own() {
        assert!(mandelbrot(0.0, 0.0, 500).is_none());
        assert!(mandelbrot(-1.0, 0.0, 500).is_none());
        assert!(mandelbrot(-0.5, 0.5, 500).is_none());
        // The Douady rabbit's c, in the period-3 bulb.
        assert!(mandelbrot(-0.123, 0.745, 500).is_none());
        assert!(mandelbrot(1.0, 0.0, 500).is_some());
        assert!(mandelbrot(0.4, 0.6, 500).is_some());
        // Everything past |c| = 2 is gone at once.
        assert!(mandelbrot(2.5, 0.0, 500).unwrap() < 3.0);
    }

    /// The escape count grows as you walk in towards the boundary.
    #[test]
    fn the_boundary_takes_longer() {
        let a = mandelbrot(0.4, 0.0, 5000).unwrap();
        let b = mandelbrot(0.26, 0.0, 5000).unwrap();
        assert!(b > a, "{b} should outlast {a}");
        // 0.25 is the cusp of the cardioid, and never escapes.
        assert!(mandelbrot(0.2499, 0.0, 5000).is_none());
    }

    /// A Julia set for a c inside the Mandelbrot set is connected, so
    /// the origin belongs to it; for a c outside it is dust.
    #[test]
    fn julia_follows_its_c() {
        assert!(julia(0.0, 0.0, -0.123, 0.745, 1000).is_none());
        assert!(julia(0.0, 0.0, 0.4, 0.6, 1000).is_some());
    }

    /// The logistic map: one fixed point, then two, then four.
    #[test]
    fn the_cascade_doubles() {
        let period = |r: f64| {
            let o = logistic_orbit(r, 4000, 64);
            let last = *o.last().unwrap();
            (1..=32)
                .find(|&p| (o[o.len() - 1 - p] - last).abs() < 1e-9)
                .unwrap_or(0)
        };
        assert_eq!(period(2.8), 1);
        assert_eq!(period(3.2), 2);
        assert_eq!(period(3.5), 4);
        assert_eq!(period(3.55), 8);
        // r = 4 fills the interval: no period at all.
        assert_eq!(period(4.0), 0);
    }

    /// Feigenbaum's constant, out of the cascade rather than out of a
    /// table. Same for the accumulation point.
    #[test]
    fn feigenbaum_comes_out_right() {
        let r = superstable();
        assert!((r[0] - 2.0).abs() < 1e-12);
        // The period-2 window is superstable at 1 + √5.
        assert!((r[1] - (1.0 + 5f64.sqrt())).abs() < 1e-9, "{}", r[1]);
        assert!(r.len() >= 8, "only found {} of them", r.len());
        assert!((feigenbaum() - 4.669201).abs() < 0.001, "{}", feigenbaum());
        assert!((accumulation() - 3.5699456).abs() < 1e-4, "{}", accumulation());
    }

    /// The Lorenz attractor is bounded, and it uses both wings.
    #[test]
    fn lorenz_stays_on_its_wings() {
        let p = lorenz(28.0, 20000, 0.005);
        assert!(p.len() > 19000);
        assert!(p.iter().all(|q| q.0.abs() < 60.0 && q.2 < 90.0 && q.2 > -1.0));
        assert!(p.iter().any(|q| q.0 > 5.0) && p.iter().any(|q| q.0 < -5.0));
        // Under ρ = 24.74 it gives up and settles on one point.
        let calm = lorenz(14.0, 40000, 0.005);
        let tail = &calm[calm.len() - 200..];
        let spread = tail.iter().map(|q| q.0).fold(f64::MIN, f64::max)
            - tail.iter().map(|q| q.0).fold(f64::MAX, f64::min);
        assert!(spread < 0.5, "still wandering by {spread}");
    }

    /// The Hénon attractor sits in a small box and never leaves it.
    #[test]
    fn henon_folds_into_its_box() {
        let p = henon(1.4, 0.3, 5000);
        assert!(p.len() > 4800);
        assert!(p.iter().all(|q| q.0.abs() < 1.5 && q.1.abs() < 0.5));
        // Push a past its basin and the orbit runs away instead.
        assert!(henon(2.5, 0.3, 5000).len() < 500);
    }
}
