//! fractal — chaos in a terminal.
//!
//! The Mandelbrot set, the Julia set of whatever point you are standing
//! on, the logistic map's road into chaos, and the Lorenz and Hénon
//! attractors. Real pixels where the terminal shows images, braille
//! elsewhere; all of it computed when you press a key and never in between.

mod canvas;
mod sets;

use canvas::{ramp, Field};
use crust::style;
use crust::{seq, Crust, Cursor, Input, Pane, Popup};
use std::io::Write;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const RUST_RGB: (u8, u8, u8) = (247, 76, 0);
const ASK_RGB: (u8, u8, u8) = (255, 200, 120);
const ERR_RGB: (u8, u8, u8) = (255, 120, 100);

#[derive(Clone, Copy, PartialEq)]
enum View {
    Mandelbrot,
    Julia,
    Logistic,
    Lorenz,
    Henon,
}

const VIEWS: [View; 5] = [
    View::Mandelbrot,
    View::Julia,
    View::Logistic,
    View::Lorenz,
    View::Henon,
];

impl View {
    fn ix(&self) -> usize {
        VIEWS.iter().position(|v| v == self).unwrap_or(0)
    }
    fn name(&self) -> &'static str {
        match self {
            View::Mandelbrot => "the Mandelbrot set",
            View::Julia => "a Julia set",
            View::Logistic => "the logistic map",
            View::Lorenz => "the Lorenz attractor",
            View::Henon => "the Hénon map",
        }
    }
    /// What `[` and `]` turn, in this view.
    fn knob(&self) -> &'static str {
        match self {
            View::Mandelbrot | View::Julia => "iterations",
            View::Logistic => "points per column",
            View::Lorenz => "ρ",
            View::Henon => "a",
        }
    }
    /// Where each one starts. `aspect` is the shape of the plot, height
    /// over width in sub-pixels, and the span is widened when needed so
    /// that the whole thing fits on a screen of that shape.
    fn home(&self, aspect: f64) -> Frame {
        // A box worth seeing, and the widest each one gets to be.
        let fit = |cx, cy, w: f64, h: f64| Frame {
            cx,
            cy,
            span: w.max(h / aspect.max(1e-6)),
            ratio: None,
        };
        match self {
            // The set runs to −2 on the left and to ±1.12 top and bottom.
            View::Mandelbrot => fit(-0.75, 0.0, 2.9, 2.5),
            // A Julia set lives inside |z| = 2, but never fills it.
            View::Julia => fit(0.0, 0.0, 3.0, 2.4),
            // r against x, each on its own scale: this one is stretched
            // to the screen rather than kept square.
            View::Logistic => Frame { cx: 3.2, cy: 0.5, span: 1.6, ratio: Some(0.625) },
            View::Lorenz => fit(0.0, 27.0, 52.0, 56.0),
            // x against y again, and the attractor is a flat banana:
            // stretched, it fills the screen instead of a thin strip.
            View::Henon => Frame { cx: 0.0, cy: 0.0, span: 2.9, ratio: Some(0.33) },
        }
    }
}

/// The window onto a view, in that view's own units. Height follows from
/// the width and the shape of the screen, so a braille dot stays square.
#[derive(Clone, Copy)]
struct Frame {
    cx: f64,
    cy: f64,
    span: f64,
    /// Height over width in world units. `None` keeps a braille dot
    /// square, whatever shape the terminal is; `Some` pins the two axes
    /// to each other, for a plot whose axes are not the same thing.
    ratio: Option<f64>,
}

impl Frame {
    fn height(&self, aspect: f64) -> f64 {
        self.span * self.ratio.unwrap_or(aspect)
    }
    /// World coordinates of a sub-pixel.
    fn at(&self, x: usize, y: usize, w: usize, h: usize) -> (f64, f64) {
        let sh = self.height(h as f64 / w as f64);
        (
            self.cx - self.span / 2.0 + self.span * x as f64 / w as f64,
            self.cy + sh / 2.0 - sh * y as f64 / h as f64,
        )
    }
    /// Sub-pixel a world point falls on.
    fn cell(&self, wx: f64, wy: f64, w: usize, h: usize) -> (i64, i64) {
        let sh = self.height(h as f64 / w as f64);
        (
            ((wx - (self.cx - self.span / 2.0)) / self.span * w as f64).round() as i64,
            (((self.cy + sh / 2.0) - wy) / sh * h as f64).round() as i64,
        )
    }
}

struct App {
    view: View,
    frames: [Frame; 5],
    /// The c of the Julia set on show.
    jc: (f64, f64),
    iters: u32,
    density: u32,
    rho: f64,
    henon_a: f64,
    /// Which pair of Lorenz axes to look at.
    proj: usize,
    /// The shape of the plot as it was last drawn.
    aspect: f64,
    chat: Vec<(String, String)>,
    status: Option<(String, (u8, u8, u8))>,
    /// The image display, made on the first draw; the picture is real
    /// pixels where it is supported.
    pixels: Option<glow::Display>,
}

const PROJ: [(&str, usize, usize); 3] = [("x-z", 0, 2), ("x-y", 0, 1), ("y-z", 1, 2)];

fn main() {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-h" | "--help" => {
                println!("fractal — chaos in the terminal (Fe2O3 suite)");
                println!();
                println!("Usage: fractal [mandelbrot|julia|logistic|lorenz|henon]");
                println!();
                println!("The Mandelbrot set, Julia sets, the logistic map's cascade into");
                println!("chaos, and the Lorenz and Hénon attractors, in pixels or in braille.");
                println!("Arrows pan, +/- zoom, j jumps from a point to its Julia set.");
                return;
            }
            "-v" | "--version" => {
                println!("fractal {VERSION}");
                return;
            }
            _ => {}
        }
    }
    let start = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let view = match start.as_str() {
        "julia" | "j" => View::Julia,
        "logistic" | "bifurcation" => View::Logistic,
        "lorenz" => View::Lorenz,
        "henon" | "hénon" => View::Henon,
        _ => View::Mandelbrot,
    };

    Crust::init();
    Crust::set_app_identity("Fractal");
    Crust::clear_screen();
    let (mut cols, mut rows) = Crust::terminal_size();
    let mut footer = Pane::new(1, rows, cols, 1, 250, 236);
    footer.scroll = false;
    let aspect = plot_aspect(cols, rows);

    let mut app = App {
        view,
        frames: [
            View::Mandelbrot.home(aspect),
            View::Julia.home(aspect),
            View::Logistic.home(aspect),
            View::Lorenz.home(aspect),
            View::Henon.home(aspect),
        ],
        // The Douady rabbit: three ears, and three again on every ear.
        jc: (-0.123, 0.745),
        iters: 300,
        density: 260,
        rho: 28.0,
        henon_a: 1.4,
        proj: 0,
        aspect,
        chat: Vec::new(),
        status: None,
        pixels: None,
    };

    (cols, rows) = draw(&mut app, &mut footer);

    loop {
        let Some(key) = Input::getchr(None) else { continue };
        let i = app.view.ix();
        // A step up or down covers the same ground on screen as a step
        // sideways, which in world units depends on the shape of things.
        let aspect = app.aspect;
        match key.as_str() {
            "q" | "Q" => break,
            "RIGHT" | "l" => app.frames[i].cx += app.frames[i].span * 0.15,
            "LEFT" | "h" => app.frames[i].cx -= app.frames[i].span * 0.15,
            "UP" | "k" => app.frames[i].cy += app.frames[i].height(aspect) * 0.15,
            "DOWN" | "j" => app.frames[i].cy -= app.frames[i].height(aspect) * 0.15,
            "+" | "=" | "i" | "PgUP" => app.frames[i].span /= 1.6,
            "-" | "_" | "o" | "PgDOWN" => app.frames[i].span *= 1.6,
            "0" => app.frames[i] = app.view.home(aspect),
            "1" | "2" | "3" | "4" | "5" => {
                app.view = VIEWS[key.parse::<usize>().unwrap_or(1) - 1];
                Crust::clear_screen();
            }
            "TAB" => {
                app.view = VIEWS[(i + 1) % VIEWS.len()];
                Crust::clear_screen();
            }
            // The point you are standing on names a Julia set. Step into
            // it, and step back out to where you were.
            "J" => match app.view {
                View::Mandelbrot => {
                    app.jc = (app.frames[i].cx, app.frames[i].cy);
                    app.frames[View::Julia.ix()] = View::Julia.home(aspect);
                    app.view = View::Julia;
                    Crust::clear_screen();
                }
                View::Julia => {
                    let (jx, jy) = app.jc;
                    let m = View::Mandelbrot.ix();
                    app.frames[m].cx = jx;
                    app.frames[m].cy = jy;
                    app.view = View::Mandelbrot;
                    Crust::clear_screen();
                }
                _ => app.say("that only works on the Mandelbrot set", ERR_RGB),
            },
            "]" | "}" => app.knob(1.0),
            "[" | "{" => app.knob(-1.0),
            "p" => app.proj = (app.proj + 1) % PROJ.len(),
            "e" => match export(&app, cols, rows) {
                Ok(p) => app.say(&format!("wrote {p}"), (140, 220, 140)),
                Err(e) => app.say(&format!("export: {e}"), ERR_RGB),
            },
            "c" => {
                let q = footer.ask_or_cancel("ask claude: ", "");
                print!("{}", Cursor::hide_seq());
                std::io::stdout().flush().ok();
                if let Some(q) = q {
                    if !q.trim().is_empty() {
                        footer.say(&style::rgb(" asking claude…", Some(ASK_RGB), None, ""));
                        std::io::stdout().flush().ok();
                        match ask_claude(&app, q.trim()) {
                            Ok(a) if !a.is_empty() => {
                                app.chat.push((q.trim().to_string(), a.clone()));
                                Crust::clear_screen();
                                let w = cols.saturating_sub(8).min(96);
                                let h = rows.saturating_sub(4).min(34);
                                let mut p = Popup::centered(w, h, 252, 234);
                                p.view(&format!(
                                    "{}\n\n{}\n\n{}",
                                    style::rgb(app.view.name(), Some(ASK_RGB), None, "b"),
                                    style::dim(q.trim()),
                                    a
                                ));
                                Crust::clear_screen();
                            }
                            Ok(_) => app.say("claude returned nothing", ERR_RGB),
                            Err(e) => app.say(&format!("claude: {e}"), ERR_RGB),
                        }
                    }
                }
            }
            "?" => {
                show_help(cols, rows);
                Crust::clear_screen();
            }
            "r" | "C-L" | "RESIZE" => Crust::clear_screen(),
            _ => {}
        }
        (cols, rows) = draw(&mut app, &mut footer);
    }

    Crust::cleanup();
    Crust::clear_screen();
}

impl App {
    fn say(&mut self, msg: &str, rgb: (u8, u8, u8)) {
        self.status = Some((msg.to_string(), rgb));
    }

    fn frame(&self) -> Frame {
        self.frames[self.view.ix()]
    }

    /// One turn of whatever `[` and `]` drive in this view.
    fn knob(&mut self, dir: f64) {
        match self.view {
            View::Mandelbrot | View::Julia => {
                self.iters = if dir > 0.0 {
                    (self.iters * 2).min(20000)
                } else {
                    (self.iters / 2).max(30)
                };
            }
            View::Logistic => {
                self.density = if dir > 0.0 {
                    (self.density * 2).min(4000)
                } else {
                    (self.density / 2).max(20)
                };
            }
            View::Lorenz => self.rho = (self.rho + dir * 0.5).clamp(0.5, 200.0),
            View::Henon => self.henon_a = (self.henon_a + dir * 0.01).clamp(0.0, 2.0),
        }
    }

    /// How hard to iterate here. Deeper zooms need more before a point
    /// can be called a member, and the shallow view should stay instant.
    fn iterations(&self) -> u32 {
        let zoom = (self.view.home(self.aspect).span / self.frame().span).max(1.0);
        let want = self.iters as f64 * (1.0 + zoom.log2() / 4.0);
        (want as u32).clamp(30, 40000)
    }
}

/// The shape of the plot: sub-pixels down over sub-pixels across.
fn plot_aspect(cols: u16, rows: u16) -> f64 {
    (rows.saturating_sub(3).max(1) as f64 * 4.0) / (cols.max(1) as f64 * 2.0)
}

// ─────────────────────────── drawing ─────────────────────────────────

fn draw(app: &mut App, footer: &mut Pane) -> (u16, u16) {
    let (cols, rows) = Crust::terminal_size();
    if cols != footer.w || rows != footer.y {
        footer.w = cols;
        footer.y = rows;
        footer.full_refresh();
        Crust::clear_screen();
    }
    let plot_h = rows.saturating_sub(3).max(1);
    let pixels = app.pixels.get_or_insert_with(glow::Display::new).supported();
    // The shape of the plot, height over width: in pixels when the
    // picture is pixels, else in braille dots taken as square.
    let aspect = if pixels {
        let (bw, bh) = glow::cell_box(cols, plot_h);
        bh as f64 / bw as f64
    } else {
        plot_aspect(cols, rows)
    };
    app.aspect = aspect;

    let mut out = String::with_capacity(cols as usize * rows as usize * 12);
    if pixels {
        // The picture sits over these rows; they only need to be empty.
        if let Some(d) = app.pixels.as_mut() { d.clear(1, 1, cols, rows, cols, rows); }
        for r in 0..plot_h {
            out.push_str(&Cursor::at(1, 2 + r));
            out.push_str(seq::ERASE_EOL);
        }
    } else {
        let cells = compute(app, cols as usize, plot_h as usize);
        for (r, row) in cells.iter().enumerate() {
            out.push_str(&Cursor::at(1, 2 + r as u16));
            let mut cur: Option<(u8, u8, u8)> = None;
            for &(ch, rgb) in row {
                match rgb {
                    Some(c) => {
                        if cur != Some(c) {
                            out.push_str(&style::set_fg_rgb(c.0, c.1, c.2));
                            cur = Some(c);
                        }
                    }
                    None => {
                        if cur.is_some() {
                            out.push_str(style::RESET);
                            cur = None;
                        }
                    }
                }
                out.push(ch);
            }
            out.push_str(style::RESET);
            out.push_str(seq::ERASE_EOL);
        }
    }
    print!("{out}");
    if pixels {
        let canvas = compute_pixels(app, cols as usize, plot_h as usize, None);
        if let Some(d) = app.pixels.as_mut() { d.show_canvas(&canvas, 1, 2); }
    }
    draw_header(app, cols, aspect);
    draw_status(app, cols, rows);
    // The keys row is a Pane, and a Pane skips work when the text has
    // not changed. After a clear_screen that would leave the row blank,
    // so tell it to repaint regardless: one row per keypress, and none
    // at all while the app sits idle.
    footer.full_refresh();
    footer.say(&style::dim(&if cols < 130 {
        format!("←↓↑→ pan · +/- zoom · 1-5 view · J julia · [ ] {} · ? help · q", app.view.knob())
    } else {
        format!(
            "←↓↑→ pan · +/- zoom · 0 reset · 1-5 view · Tab next · J julia here · \
             [ ] {} · p project · e save · c claude · ? help · q",
            app.view.knob()
        )
    }));
    print!("{}", Cursor::hide_seq());
    std::io::stdout().flush().ok();
    (cols, rows)
}

/// Fill `f` for whichever view is on. `detail` is how many pixels of `f`
/// stand where one braille dot would; the scatters get that many more
/// points, so they stay as dense on a fine canvas. Returns whether the
/// field is a scatter of points, and the palette to colour it with.
fn fill(app: &App, f: &mut Field, detail: f64) -> (bool, fn(f32) -> (u8, u8, u8)) {
    let (w, h) = (f.w, f.h);
    let fr = app.frame();
    match app.view {
        View::Mandelbrot | View::Julia => {
            let max = app.iterations();
            let julia = app.view == View::Julia;
            let (jx, jy) = app.jc;
            // Bands of rows, one per core.
            let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, h.max(1));
            let band = h.div_ceil(threads);
            std::thread::scope(|s| {
                for (b, rows) in f.v.chunks_mut(w * band).enumerate() {
                    s.spawn(move || {
                        for (i, v) in rows.iter_mut().enumerate() {
                            let (x, y) = (i % w, b * band + i / w);
                            let (px, py) = fr.at(x, y, w, h);
                            let esc = if julia {
                                sets::julia(px, py, jx, jy, max)
                            } else {
                                sets::mandelbrot(px, py, max)
                            };
                            // Inside reads as full, so the set shows up solid.
                            // Outside, a logarithm: nearly everything escapes in
                            // the first few steps, and a linear scale would leave
                            // all of that crushed into one dark corner. Then a
                            // stiff gamma so the far field goes properly dark.
                            *v = match esc {
                                None => 1.0,
                                Some(mu) => {
                                    let t = (1.0 + mu).ln() / (1.0 + max as f64).ln();
                                    (0.9 * t.powf(2.4)) as f32
                                }
                            };
                        }
                    });
                }
            });
            (false, escape_palette)
        }
        View::Logistic => {
            for x in 0..w {
                let (r, _) = fr.at(x, 0, w, h);
                if !(0.0..=4.0).contains(&r) {
                    continue;
                }
                for v in sets::logistic_orbit(r, 600, app.density) {
                    let (_, y) = fr.cell(0.0, v, w, h);
                    f.hit(x as i64, y);
                }
            }
            f.normalise_log();
            (true, logistic_palette)
        }
        View::Lorenz => {
            let (_, a, b) = PROJ[app.proj];
            // The same stretch of trajectory, in finer steps.
            for p in sets::lorenz(app.rho, (90_000.0 * detail) as usize, 0.004 / detail) {
                let q = [p.0, p.1, p.2];
                let (x, y) = fr.cell(q[a], q[b], w, h);
                f.hit(x, y);
            }
            f.normalise_log();
            (true, lorenz_palette)
        }
        View::Henon => {
            for p in sets::henon(app.henon_a, 0.3, (300_000.0 * detail) as usize) {
                let (x, y) = fr.cell(p.0, p.1, w, h);
                f.hit(x, y);
            }
            f.normalise_log();
            (true, henon_palette)
        }
    }
}

/// The picture as braille cells with a colour each.
fn compute(app: &App, cols: usize, rows: usize) -> Vec<Vec<(char, Option<(u8, u8, u8)>)>> {
    let mut f = Field::new(cols, rows);
    let (points, palette) = fill(app, &mut f, 1.0);
    if points { f.render_points(cols, rows, palette) } else { f.render(cols, rows, palette) }
}

/// The picture in real pixels, on a canvas of `cols` × `rows` cells of
/// `cell` pixels.
fn compute_pixels(app: &App, cols: usize, rows: usize, cell: Option<(u16, u16)>) -> glow::Canvas {
    let mut c = glow::Canvas::sized(cols as u16, rows as u16, cell);
    let mut f = Field::with_size(c.w, c.h);
    let detail = (c.w * c.h) as f64 / (cols * 2 * rows * 4).max(1) as f64;
    let (points, palette) = fill(app, &mut f, detail.clamp(1.0, 32.0));
    f.paint(&mut c, points, palette);
    c
}

fn logistic_palette(v: f32) -> (u8, u8, u8) {
    ramp(lift(v), &[(40, 90, 190), (90, 210, 230), (235, 245, 255)])
}

fn lorenz_palette(v: f32) -> (u8, u8, u8) {
    ramp(lift(v), &[(150, 50, 130), (255, 140, 60), (255, 245, 210)])
}

fn henon_palette(v: f32) -> (u8, u8, u8) {
    ramp(lift(v), &[(40, 160, 90), (170, 230, 110), (245, 255, 220)])
}

/// A scatter is mostly cells visited once or twice, and those carry the
/// shape. Lift them off the floor of the ramp so they read.
fn lift(v: f32) -> f32 {
    (0.25 + 0.75 * v).min(1.0)
}

/// Blue through to white as the escape gets slower, and a flat slate for
/// the points that never escape at all.
fn escape_palette(v: f32) -> (u8, u8, u8) {
    if v > 0.93 {
        return (72, 78, 122);
    }
    ramp(
        v / 0.9,
        &[
            (10, 20, 70),
            (30, 90, 200),
            (60, 200, 210),
            (200, 240, 120),
            (255, 190, 70),
            (255, 250, 235),
        ],
    )
}

fn draw_header(app: &App, cols: u16, aspect: f64) {
    const BAR: (u8, u8, u8) = (38, 38, 38);
    let bg = style::set_bg_rgb(BAR.0, BAR.1, BAR.2);
    let armed = |s: &str| s.replace(style::RESET, &format!("{}{}", style::RESET, bg));
    let fr = app.frame();
    let zoom = app.view.home(aspect).span / fr.span;
    let left = format!(
        " {}  {} ",
        style::rgb("fractal", Some(RUST_RGB), None, "b"),
        style::rgb(app.view.name(), Some((255, 220, 140)), None, "b"),
    );
    let right = format!(
        "{}  ·  {} ",
        match app.view {
            View::Logistic => format!("r {:.4} … {:.4}", fr.cx - fr.span / 2.0, fr.cx + fr.span / 2.0),
            _ => format!("centre {}", complex(fr.cx, fr.cy, fr.span)),
        },
        if zoom >= 2.0 { format!("zoom ×{}", si(zoom)) } else { "unzoomed".to_string() }
    );
    let pad = (cols as usize)
        .saturating_sub(crust::display_width(&left) + crust::display_width(&right));
    let bar = format!("{bg}{}{}{}", armed(&left), " ".repeat(pad.max(1)), armed(&style::dim(&right)));
    print!(
        "{}{}{}{}",
        Cursor::at(1, 1),
        crust::truncate_ansi(&bar, cols as usize),
        style::RESET,
        seq::ERASE_EOL
    );
}

fn draw_status(app: &App, cols: u16, rows: u16) {
    let key = |k: &str| style::rgb(k, Some((120, 170, 220)), None, "");
    let val = |v: &str| style::rgb(v, Some((235, 235, 240)), None, "");
    let fr = app.frame();
    let line = match app.view {
        View::Mandelbrot => format!(
            "{} {}   {} {}   {}",
            key("z → z² + c, from z = 0 ·"),
            val(&format!("{} iterations", app.iterations())),
            key("width"),
            val(&si(fr.span)),
            style::dim("J takes the centre point and draws its Julia set"),
        ),
        View::Julia => format!(
            "{} {}   {} {}   {}",
            key("z → z² + c, c ="),
            val(&complex(app.jc.0, app.jc.1, 0.1)),
            key("·"),
            val(&format!("{} iterations", app.iterations())),
            style::dim("J goes back to that point on the Mandelbrot set"),
        ),
        View::Logistic => format!(
            "{} {}   {} {}   {} {}",
            key("x → r·x·(1−x) ·"),
            val(&format!("{} points per column", app.density)),
            key("δ ="),
            val(&format!("{:.6}", sets::feigenbaum())),
            key("· chaos from r ="),
            val(&format!("{:.6}", sets::accumulation())),
        ),
        View::Lorenz => format!(
            "{} {}   {} {}   {} {}",
            key("σ 10 · β 8/3 · ρ"),
            val(&format!("{:.1}", app.rho)),
            key("·"),
            val(&format!("{} steps of 0.004", 90_000)),
            key("· axes"),
            val(PROJ[app.proj].0),
        ),
        View::Henon => format!(
            "{} {}   {} {}   {}",
            key("x → 1 − a·x² + y,  y → 0.3·x   ·   a ="),
            val(&format!("{:.2}", app.henon_a)),
            key("·"),
            val("300 000 points"),
            style::dim(if app.henon_a > 1.42 { "past its basin: the orbit runs away" } else { "" }),
        ),
    };
    let status = match &app.status {
        Some((msg, rgb)) => style::rgb(&format!("   {msg}"), Some(*rgb), None, ""),
        None => String::new(),
    };
    print!(
        "{} {}{}{}{}",
        Cursor::at(1, rows.saturating_sub(1)),
        crust::truncate_ansi(&format!("{line}{status}"), cols.saturating_sub(2) as usize),
        style::RESET,
        seq::ERASE_EOL,
        Cursor::at(1, rows)
    );
}

/// A complex number, to as many places as the current width warrants.
fn complex(re: f64, im: f64, span: f64) -> String {
    let places = ((-span.log10()).ceil() as i32 + 3).clamp(3, 17) as usize;
    format!(
        "{:.*} {} {:.*}i",
        places,
        re,
        if im < 0.0 { "−" } else { "+" },
        places,
        im.abs()
    )
}

/// A number in the style people say out loud: 1.2k, 3.4M, 5.6e−9.
fn si(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 {
        format!("{:.1}e{}", v / 10f64.powi(a.log10() as i32), a.log10() as i32)
    } else if a >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if a >= 1e3 {
        format!("{:.1}k", v / 1e3)
    } else if a >= 1.0 {
        format!("{v:.2}")
    } else if a >= 1e-3 {
        format!("{v:.5}")
    } else {
        format!("{v:.3e}")
    }
}

fn show_help(cols: u16, rows: u16) {
    let help = format!(
        "{}\n\n  \
         Five pictures of the same idea: repeat something simple, and see\n  \
         what it settles into.\n\n  \
         VIEWS\n    \
           1  the Mandelbrot set   which c leave z → z² + c bounded\n    \
           2  a Julia set          the same map, c held still, z varied\n    \
           3  the logistic map     x → r·x·(1−x), plotted against r\n    \
           4  the Lorenz attractor three equations that never repeat\n    \
           5  the Hénon map        a plane folded onto a fractal\n\n  \
         MOVING\n    \
           ← ↓ ↑ →, h j k l   pan\n    \
           + -, i o           zoom in and out\n    \
           0                  back to where this view started\n    \
           Tab                the next view\n    \
           J                  from a point to its Julia set, and back\n    \
           [ ]                turn this view's knob: iterations, ρ, or a\n    \
           p                  which two Lorenz axes to look at\n\n  \
         THE REST\n    \
           e   save the picture as braille text in ~/fractal.txt\n    \
           c   ask Claude about what is on screen\n    \
           ? q this help · quit\n\n  \
         In glass, or any terminal that shows images, the picture is real\n  \
         pixels. Elsewhere it is braille: a cell is 2×4 dots and one colour,\n  \
         the dots dithered so their number tracks the value there, and the\n  \
         colour the cell's average.\n\n  \
         Feigenbaum's δ and the point where the doubling gives way to chaos\n  \
         are worked out while the app runs, not looked up.\n\n  \
         {}",
        style::rgb(&format!("fractal v{VERSION}"), Some(ASK_RGB), None, "b"),
        style::dim("ESC or q closes this.")
    );
    let w = cols.saturating_sub(8).min(78);
    let h = (help.lines().count() as u16 + 1).min(rows.saturating_sub(4));
    let mut p = Popup::centered(w, h, 252, 234);
    p.view(&help);
}

/// The picture as it stands, in braille, without the colour.
fn export(app: &App, cols: u16, rows: u16) -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let path = format!("{home}/fractal.txt");
    let cells = compute(app, cols as usize, rows.saturating_sub(3).max(1) as usize);
    let mut out = format!("{}\n\n", app.view.name());
    for row in cells {
        let line: String = row.iter().map(|&(c, _)| c).collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| e.to_string())?;
    Ok(path)
}

fn claude_run(prompt: &str, input: &str) -> Result<String, String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("claude")
        .args(["-p", prompt])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => "claude not on PATH".to_string(),
            _ => format!("spawn: {e}"),
        })?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes()).map_err(|e| format!("stdin: {e}"))?;
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("(no message)").chars().take(80).collect());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn ask_claude(app: &App, question: &str) -> Result<String, String> {
    let fr = app.frame();
    let mut ctx = format!("On screen: {}.\n", app.view.name());
    match app.view {
        View::Mandelbrot => ctx.push_str(&format!(
            "Iterating z → z² + c from z = 0. The view is centred on c = {}, \
             {} wide, at {} iterations.\n",
            complex(fr.cx, fr.cy, fr.span),
            si(fr.span),
            app.iterations()
        )),
        View::Julia => ctx.push_str(&format!(
            "Iterating z → z² + c with c = {} fixed, varying the starting z. \
             Centred on {}, {} wide.\n",
            complex(app.jc.0, app.jc.1, 0.1),
            complex(fr.cx, fr.cy, fr.span),
            si(fr.span)
        )),
        View::Logistic => ctx.push_str(&format!(
            "x → r·x·(1−x), plotted for r from {:.6} to {:.6}. Feigenbaum's δ \
             computed here is {:.6}, and the cascade accumulates at r = {:.6}.\n",
            fr.cx - fr.span / 2.0,
            fr.cx + fr.span / 2.0,
            sets::feigenbaum(),
            sets::accumulation()
        )),
        View::Lorenz => ctx.push_str(&format!(
            "The Lorenz system with σ = 10, β = 8/3, ρ = {:.1}, seen along {}.\n",
            app.rho, PROJ[app.proj].0
        )),
        View::Henon => ctx.push_str(&format!(
            "The Hénon map x → 1 − a·x² + y, y → 0.3·x, with a = {:.2}.\n",
            app.henon_a
        )),
    }
    if !app.chat.is_empty() {
        ctx.push_str("\nEarlier in this conversation:\n");
        for (q, a) in &app.chat {
            ctx.push_str(&format!("User: {q}\nYou: {a}\n\n"));
        }
    }
    ctx.push_str(&format!("\nQuestion: {question}\n"));
    claude_run(
        "You are a mathematician answering inside a terminal app about dynamical systems \
         and fractals. Answer in plain text, no markdown, under 200 words unless the \
         question demands more.",
        &ctx,
    )
}
