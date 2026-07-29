//! A braille canvas with a value behind every dot.
//!
//! Everything this app draws is a scalar field: how fast a point escapes,
//! or how often a trajectory passes through. So the drawing happens in
//! two steps. Fill a `Field` at sub-pixel resolution, then hand it a
//! palette and get back cells.
//!
//! A braille cell is 2×4 dots, and one colour. That leaves a choice: the
//! dots carry eight times the detail, the colour only one value per cell.
//! Rather than pick, the renderer dithers. A dot lights when its own
//! value clears an ordered threshold, so the density of lit dots inside a
//! cell tracks the field, and the cell's colour comes from the average.
//! Fine structure lands in the dots, the broad shape in the colour.

/// Braille dot bit per sub-pixel. Rows 0-2 use bits 0,1,2 / 3,4,5; the
/// bottom row uses 6,7.
const DOTS: [[u8; 2]; 4] = [
    [0x01, 0x08],
    [0x02, 0x10],
    [0x04, 0x20],
    [0x40, 0x80],
];

/// A 4×2 ordered dither matrix, nudged off the ends so a value of 0
/// lights nothing and a value of 1 lights everything.
const BAYER: [[f32; 2]; 4] = [
    [0.0625, 0.5625],
    [0.8125, 0.3125],
    [0.1875, 0.6875],
    [0.9375, 0.4375],
];

/// A scalar per sub-pixel, in whatever units the caller likes.
pub struct Field {
    /// Sub-pixels across and down: twice and four times the cell counts.
    pub w: usize,
    pub h: usize,
    pub v: Vec<f32>,
}

impl Field {
    pub fn new(cols: usize, rows: usize) -> Field {
        Field { w: cols * 2, h: rows * 4, v: vec![0.0; cols * 2 * rows * 4] }
    }

    pub fn set(&mut self, x: usize, y: usize, val: f32) {
        if x < self.w && y < self.h {
            self.v[y * self.w + x] = val;
        }
    }

    /// Count one more visit to this sub-pixel.
    pub fn hit(&mut self, x: i64, y: i64) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        self.v[y as usize * self.w + x as usize] += 1.0;
    }

    pub fn max(&self) -> f32 {
        self.v.iter().copied().fold(0.0, f32::max)
    }

    /// Rescale so the busiest sub-pixel reads 1. A logarithm first,
    /// because a trajectory spends wildly uneven time in different
    /// places and the rare corners would otherwise vanish.
    pub fn normalise_log(&mut self) {
        let m = self.max();
        if m <= 0.0 {
            return;
        }
        let s = 1.0 / (1.0 + m).ln();
        for v in self.v.iter_mut() {
            *v = (1.0 + *v).ln() * s;
        }
    }

    /// The cells, as (glyph, colour), with the dots dithered against the
    /// field. Right for a picture that is bright everywhere, like an
    /// escape-time fractal.
    pub fn render(
        &self,
        cols: usize,
        rows: usize,
        palette: impl Fn(f32) -> (u8, u8, u8),
    ) -> Vec<Vec<(char, Option<(u8, u8, u8)>)>> {
        self.render_with(cols, rows, true, palette)
    }

    /// The same, but every sub-pixel that was hit at all lights up.
    /// Right for a scatter of points, where dithering would throw away
    /// the lonely ones that carry the shape.
    pub fn render_points(
        &self,
        cols: usize,
        rows: usize,
        palette: impl Fn(f32) -> (u8, u8, u8),
    ) -> Vec<Vec<(char, Option<(u8, u8, u8)>)>> {
        self.render_with(cols, rows, false, palette)
    }

    fn render_with(
        &self,
        cols: usize,
        rows: usize,
        dither: bool,
        palette: impl Fn(f32) -> (u8, u8, u8),
    ) -> Vec<Vec<(char, Option<(u8, u8, u8)>)>> {
        let mut out = Vec::with_capacity(rows);
        for cy in 0..rows {
            let mut row = Vec::with_capacity(cols);
            for cx in 0..cols {
                let (mut bits, mut sum) = (0u8, 0.0f32);
                for dy in 0..4 {
                    for dx in 0..2 {
                        let (x, y) = (cx * 2 + dx, cy * 4 + dy);
                        let v = if x < self.w && y < self.h { self.v[y * self.w + x] } else { 0.0 };
                        sum += v;
                        let lit = if dither { v > BAYER[dy][dx] } else { v > 0.0 };
                        if lit {
                            bits |= DOTS[dy][dx];
                        }
                    }
                }
                if bits == 0 {
                    row.push((' ', None));
                } else {
                    let ch = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
                    row.push((ch, Some(palette(sum / 8.0))));
                }
            }
            out.push(row);
        }
        out
    }
}

/// A colour ramp through a list of stops.
pub fn ramp(t: f32, stops: &[(u8, u8, u8)]) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (t as usize).min(stops.len() - 2);
    let f = t - i as f32;
    let (a, b) = (stops[i], stops[i + 1]);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f) as u8;
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field of ones lights every dot; a field of zeroes lights none.
    #[test]
    fn the_extremes_are_solid_and_empty() {
        let mut f = Field::new(4, 2);
        let cells = f.render(4, 2, |_| (1, 2, 3));
        assert!(cells.iter().flatten().all(|&(c, _)| c == ' '));
        for v in f.v.iter_mut() {
            *v = 1.0;
        }
        let cells = f.render(4, 2, |_| (1, 2, 3));
        assert!(cells.iter().flatten().all(|&(c, _)| c == '⣿'));
    }

    /// Half brightness lights about half the dots, and the colour the
    /// cell gets is the colour of its average.
    #[test]
    fn a_grey_cell_is_half_lit() {
        let mut f = Field::new(1, 1);
        for v in f.v.iter_mut() {
            *v = 0.5;
        }
        let cells = f.render(1, 1, |v| ((v * 200.0) as u8, 0, 0));
        let (ch, rgb) = cells[0][0];
        assert_eq!(ch.to_string().chars().next().unwrap() as u32 & 0xff00, 0x2800);
        assert_eq!(rgb, Some((100, 0, 0)));
        let lit = (ch as u32 - 0x2800).count_ones();
        assert_eq!(lit, 4, "half a cell should be four dots, got {lit}");
    }

    /// A single visit survives the point renderer, and would not
    /// survive the dithered one.
    #[test]
    fn a_lone_point_still_shows() {
        let mut f = Field::new(1, 1);
        for _ in 0..200 {
            f.hit(0, 0); // somewhere the trajectory keeps coming back to
        }
        f.hit(1, 2); // and somewhere it passed exactly once
        f.normalise_log();
        let dot = DOTS[2][1] as u32;
        let bits = |c: char| c as u32 - 0x2800;
        assert_eq!(bits(f.render(1, 1, |_| (1, 1, 1))[0][0].0) & dot, 0);
        assert_ne!(bits(f.render_points(1, 1, |_| (1, 1, 1))[0][0].0) & dot, 0);
    }

    #[test]
    fn counting_and_normalising() {
        let mut f = Field::new(2, 1);
        for _ in 0..100 {
            f.hit(0, 0);
        }
        f.hit(3, 3);
        f.hit(-1, 0); // off the edge, ignored
        assert_eq!(f.max(), 100.0);
        f.normalise_log();
        assert!((f.max() - 1.0).abs() < 1e-6);
        // The single visit still shows: that is what the log is for.
        assert!(f.v[3 * f.w + 3] > 0.14, "{}", f.v[3 * f.w + 3]);
    }
}
