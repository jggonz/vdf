//! Colour, bars and table layout.

use crate::units::{human, human_count};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

// ───────────────────────────────── colour ─────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Theme {
    Heat,
    Ocean,
    Sunset,
    Forest,
    Rainbow,
    Mono,
}

type Rgb = [u8; 3];

const HEAT: &[(f64, Rgb)] = &[
    (0.00, [0, 200, 160]),
    (0.40, [120, 220, 70]),
    (0.62, [245, 215, 50]),
    (0.80, [250, 140, 40]),
    (1.00, [240, 50, 90]),
];
const OCEAN: &[(f64, Rgb)] = &[
    (0.00, [40, 220, 200]),
    (0.50, [30, 140, 230]),
    (1.00, [110, 60, 220]),
];
const SUNSET: &[(f64, Rgb)] = &[
    (0.00, [255, 200, 80]),
    (0.45, [250, 110, 90]),
    (0.75, [210, 60, 140]),
    (1.00, [120, 50, 170]),
];
const FOREST: &[(f64, Rgb)] = &[
    (0.00, [180, 230, 100]),
    (0.55, [60, 180, 90]),
    (1.00, [20, 100, 80]),
];
const MONO: &[(f64, Rgb)] = &[(0.00, [110, 110, 120]), (1.00, [235, 235, 245])];

pub struct Palette(Option<&'static [(f64, Rgb)]>);

impl Palette {
    pub fn new(theme: Theme) -> Self {
        Palette(match theme {
            Theme::Heat => Some(HEAT),
            Theme::Ocean => Some(OCEAN),
            Theme::Sunset => Some(SUNSET),
            Theme::Forest => Some(FOREST),
            Theme::Mono => Some(MONO),
            Theme::Rainbow => None,
        })
    }

    /// Colour at position `t` in 0..=1.
    pub fn at(&self, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let Some(stops) = self.0 else {
            return hsv(0.83 * t, 0.75, 1.0);
        };
        for w in stops.windows(2) {
            let ((p0, c0), (p1, c1)) = (w[0], w[1]);
            if t <= p1 {
                let k = if p1 == p0 { 0.0 } else { (t - p0) / (p1 - p0) };
                let mix =
                    |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * k).round_ties_even() as u8;
                return [mix(c0[0], c1[0]), mix(c0[1], c1[1]), mix(c0[2], c1[2])];
            }
        }
        stops[stops.len() - 1].1
    }
}

fn hsv(h: f64, s: f64, v: f64) -> Rgb {
    let i = ((h * 6.0) as i64).rem_euclid(6);
    let f = h * 6.0 - (h * 6.0).trunc();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    None,
    Ansi256,
    TrueColor,
}

pub struct Painter {
    pub mode: ColorMode,
}

impl Painter {
    pub fn on(&self) -> bool {
        self.mode != ColorMode::None
    }

    fn to256(c: Rgb) -> u8 {
        let (mx, mn) = (*c.iter().max().unwrap(), *c.iter().min().unwrap());
        if mx - mn < 12 {
            let avg = (c[0] as f64 + c[1] as f64 + c[2] as f64) / 3.0;
            let lvl = ((avg - 8.0) / 10.0).round_ties_even().clamp(0.0, 23.0) as u8;
            return 232 + lvl;
        }
        let q = |v: u8| -> u8 {
            if v < 48 {
                0
            } else if v < 115 {
                1
            } else {
                (v - 35) / 40
            }
        };
        16 + 36 * q(c[0]) + 6 * q(c[1]) + q(c[2])
    }

    fn code(&self, c: Rgb, layer: u8) -> String {
        match self.mode {
            ColorMode::None => String::new(),
            ColorMode::TrueColor => format!("\x1b[{layer};2;{};{};{}m", c[0], c[1], c[2]),
            ColorMode::Ansi256 => format!("\x1b[{layer};5;{}m", Self::to256(c)),
        }
    }

    pub fn fg(&self, c: Rgb) -> String {
        self.code(c, 38)
    }

    pub fn bg(&self, c: Rgb) -> String {
        self.code(c, 48)
    }

    pub fn sgr(&self, code: u8) -> String {
        if self.on() {
            format!("\x1b[{code}m")
        } else {
            String::new()
        }
    }

    pub fn reset(&self) -> &'static str {
        if self.on() { "\x1b[0m" } else { "" }
    }
}

// ───────────────────────────────── text ─────────────────────────────────

pub fn disp_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn cw(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

fn pad(s: String, width: usize) -> String {
    let w = disp_width(&s);
    s + &" ".repeat(width.saturating_sub(w))
}

/// Truncate from the left with an ellipsis so the tail stays visible.
pub fn fit_left(s: &str, width: usize) -> String {
    if disp_width(s) <= width {
        return pad(s.to_string(), width);
    }
    let mut out: Vec<char> = Vec::new();
    let mut w = 0;
    for c in s.chars().rev() {
        if w + cw(c) > width.saturating_sub(1) {
            break;
        }
        w += cw(c);
        out.push(c);
    }
    out.reverse();
    pad(format!("…{}", out.into_iter().collect::<String>()), width)
}

/// Truncate from the right with an ellipsis.
pub fn fit_right(s: &str, width: usize) -> String {
    if disp_width(s) <= width {
        return pad(s.to_string(), width);
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        if w + cw(c) > width.saturating_sub(1) {
            break;
        }
        w += cw(c);
        out.push(c);
    }
    pad(out + "…", width)
}

// ───────────────────────────────── bars ─────────────────────────────────

const EIGHTHS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
pub const TRACK: Rgb = [52, 54, 62]; // empty part of the bar (background)
pub const OTHER: Rgb = [118, 120, 132]; // used by sibling volumes / reserved
pub const UNALLOC: Rgb = [150, 152, 165]; // not partitioned

/// One bar: `used` (gradient) then `other` (hatched) then free (track),
/// with `unalloc` (dotted) at the far right. All scaled to `size`.
#[allow(clippy::too_many_arguments)]
pub fn render_bar(
    width: usize,
    used: u64,
    other: u64,
    unalloc: u64,
    size: u64,
    pal: &Palette,
    p: &Painter,
    ascii: bool,
) -> String {
    if width == 0 {
        return String::new();
    }
    let w = width as f64;
    let frac = |n: u64| {
        if size == 0 {
            0.0
        } else {
            n as f64 / size as f64 * w
        }
    };
    let u = frac(used).min(w);
    let o = frac(other).max(0.0);
    let un = (frac(unalloc).round_ties_even() as usize).min(width);

    let mut full = u.floor() as usize;
    let mut part = ((u - full as f64) * 8.0).round_ties_even() as usize;
    if part == 8 {
        full += 1;
        part = 0;
    }
    if used > 0 && full == 0 && part == 0 {
        part = 1; // never hide a non-empty volume completely
    }
    if ascii {
        full += usize::from(part >= 4);
        part = 0;
        if used > 0 && full == 0 {
            full = 1;
        }
    }
    full = full.min(width);

    let color = p.on();
    let bg = p.bg(TRACK);
    let mut out = String::new();
    for i in 0..full {
        out += &p.fg(pal.at((i as f64 + 0.5) / w));
        out += &bg;
        out.push(if ascii { '#' } else { '█' });
    }
    let mut pos = full;
    if part > 0 && pos < width {
        out += &p.fg(pal.at((pos as f64 + 0.5) / w));
        out += &bg;
        out.push(EIGHTHS[part]);
        pos += 1;
    }

    let free_end = (width - un).max(pos);
    let o_end = free_end.min(pos.max((u + o).round_ties_even() as usize));
    if o_end > pos {
        let ch = if ascii {
            "+"
        } else if color {
            "░"
        } else {
            "▒"
        };
        out += &p.fg(OTHER);
        out += &bg;
        out += &ch.repeat(o_end - pos);
        pos = o_end;
    }
    if pos < free_end {
        let ch = if ascii {
            "."
        } else if color {
            " "
        } else {
            "░"
        };
        out += &bg;
        out += &ch.repeat(free_end - pos);
        pos = free_end;
    }
    if pos < width {
        let ch = if ascii { "-" } else { "·" };
        out += &p.fg(UNALLOC);
        out += &bg;
        out += &ch.repeat(width - pos);
    }
    out += p.reset();
    out
}

// ───────────────────────────────── table ─────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Row {
    /// Tree glyphs / indent, never truncated.
    pub prefix: String,
    pub label: String,
    /// Truncate the label from the left (paths) rather than the right (names).
    pub trunc_left: bool,
    pub strong: bool,
    pub device: String,
    pub fstype: String,
    pub size: u64,
    pub used: u64,
    pub avail: u64,
    pub other: u64,
    pub unalloc: u64,
    pub pct: u64,
    /// Usage can't be known (no mounted filesystem on this disk).
    pub unknown: bool,
}

#[derive(Debug, Clone)]
pub enum Line {
    /// Group heading: bold name, dim caption.
    Title(String, String),
    Row(Row),
    Rule,
    Blank,
}

pub struct Opts {
    pub cols: usize,
    pub head_name: String,
    pub long: bool,
    pub inodes: bool,
    pub si: bool,
    pub ascii: bool,
    pub bar_max: usize,
    pub max_name: usize,
    pub warn: u64,
    pub header: bool,
    pub legend: bool,
}

pub fn render(lines: &[Line], o: &Opts, pal: &Palette, p: &Painter) -> String {
    let fmt = |n: u64| {
        if o.inodes {
            human_count(n)
        } else {
            human(n, o.si)
        }
    };
    let (dim, bold, reset) = (p.sgr(2), p.sgr(1), p.reset());
    let rows: Vec<&Row> = lines
        .iter()
        .filter_map(|l| if let Line::Row(r) = l { Some(r) } else { None })
        .collect();

    let widest = rows
        .iter()
        .map(|r| disp_width(&r.prefix) + disp_width(&r.label))
        .max()
        .unwrap_or(1);
    let name_w = disp_width(&o.head_name).max(widest).min(o.max_name);
    let (mut dev_w, mut type_w) = (0, 0);
    if o.long {
        dev_w = rows
            .iter()
            .map(|r| disp_width(&r.device))
            .max()
            .unwrap_or(0)
            .clamp(10, 22);
        type_w = rows
            .iter()
            .map(|r| disp_width(&r.fstype))
            .max()
            .unwrap_or(0)
            .clamp(4, 10);
    }

    let nums_w = 4 + 2 + 6 + 1 + 6 + 1 + 6; // Use%  Used Size Avail
    let left_w = name_w + 2 + if o.long { dev_w + 2 + type_w + 2 } else { 0 };
    let bar_w = (o.cols as isize - left_w as isize - 2 - nums_w as isize - 1)
        .clamp(10, o.bar_max.max(10) as isize) as usize;
    let table_w = left_w + bar_w + nums_w + 2;

    let left_cols = |name: String, dev: &str, ty: &str| {
        let mut s = String::new();
        if o.long {
            s += &format!(
                "{dim}{}{reset}  {dim}{}{reset}  ",
                fit_left(dev, dev_w),
                fit_right(ty, type_w)
            );
        }
        s + &name
    };

    let mut out: Vec<String> = Vec::new();
    if o.header {
        let mut scale: Vec<char> = vec![' '; bar_w];
        if bar_w >= 14 {
            let put = |s: &mut Vec<char>, at: usize, txt: &str| {
                for (k, c) in txt.chars().enumerate() {
                    if at + k < s.len() {
                        s[at + k] = c;
                    }
                }
            };
            put(&mut scale, 0, "0%");
            put(&mut scale, bar_w / 2 - 1, "50%");
            put(&mut scale, bar_w - 4, "100%");
        }
        let (u, s, a) = if o.inodes {
            ("IUsed", "Inodes", "IFree")
        } else {
            ("Used", "Size", "Avail")
        };
        let hdr = left_cols(
            fit_right(&o.head_name, name_w),
            &fit_right("Filesystem", dev_w),
            &fit_right("Type", type_w),
        );
        out.push(format!(
            "{dim}{hdr}  {}  {:>4}  {u:>6} {s:>6} {a:>6}{reset}",
            scale.into_iter().collect::<String>(),
            "Use%"
        ));
    }

    let mut any_other = false;
    let mut any_unalloc = false;
    let mut any_unknown = false;
    for line in lines {
        match line {
            Line::Blank => out.push(String::new()),
            Line::Rule => out.push(format!("{dim}{}{reset}", "─".repeat(table_w))),
            Line::Title(name, caption) => {
                let room = table_w.saturating_sub(disp_width(name) + 2);
                if caption.is_empty() || room < 4 {
                    out.push(format!("{bold}{name}{reset}"));
                } else {
                    let cap = fit_right(caption, room);
                    out.push(format!(
                        "{bold}{name}{reset}  {dim}{}{reset}",
                        cap.trim_end()
                    ));
                }
            }
            Line::Row(r) => {
                if r.unknown {
                    any_unknown = true;
                } else {
                    any_other |= r.other > r.size / 100;
                }
                any_unalloc |= r.unalloc > 0;
                let room = name_w.saturating_sub(disp_width(&r.prefix));
                let label = if r.trunc_left {
                    fit_left(&r.label, room)
                } else {
                    fit_right(&r.label, room)
                };
                let label = if r.strong {
                    format!("{bold}{label}{reset}")
                } else {
                    label
                };
                let name = if r.prefix.is_empty() {
                    label
                } else {
                    format!("{dim}{}{reset}{label}", r.prefix)
                };
                let bar = render_bar(bar_w, r.used, r.other, r.unalloc, r.size, pal, p, o.ascii);
                let flag = if r.pct >= o.warn && !r.unknown {
                    format!("{}{bold}!{reset}", p.fg(pal.at(1.0)))
                } else {
                    " ".into()
                };
                let pct = if r.unknown {
                    format!("{dim}{:>4}{reset}{flag}", "--")
                } else {
                    format!(
                        "{}{bold}{:>3}%{reset}{flag}",
                        p.fg(pal.at(r.pct as f64 / 100.0)),
                        r.pct
                    )
                };
                let (used, avail) = if r.unknown {
                    ("-".to_string(), "-".to_string())
                } else {
                    (fmt(r.used), fmt(r.avail))
                };
                let nums = format!(
                    "{used:>6} {dim}{:>6}{reset} {bold}{avail:>6}{reset}",
                    fmt(r.size)
                );
                out.push(format!(
                    "{}  {bar}  {pct} {nums}",
                    left_cols(name, &r.device, &r.fstype)
                ));
            }
        }
    }

    if o.legend && (any_other || any_unalloc || any_unknown) {
        let hatch = match (any_other, any_unknown) {
            (true, true) => "reserved / other volumes / not mounted",
            (false, true) => "not mounted (usage unknown)",
            _ => "reserved / other volumes in container",
        };
        let mut items: Vec<String> = Vec::new();
        if p.on() {
            let sw = |c: Rgb, ch: &str| format!("{}{}{ch}{reset}", p.fg(c), p.bg(TRACK));
            items.push(format!(
                "{}{}{} used",
                sw(pal.at(0.15), "█"),
                sw(pal.at(0.5), "█"),
                sw(pal.at(0.85), "█")
            ));
            if any_other || any_unknown {
                items.push(format!("{} {hatch}", sw(OTHER, "░░")));
            }
            items.push(format!("{}  {reset} free", p.bg(TRACK)));
            if any_unalloc {
                items.push(format!("{} unallocated", sw(UNALLOC, "··")));
            }
        } else {
            let (u, ot, f, un) = if o.ascii {
                ("#", "+", ".", "-")
            } else {
                ("█", "▒", "░", "·")
            };
            items.push(format!("{u} used"));
            if any_other || any_unknown {
                items.push(format!("{ot} {hatch}"));
            }
            items.push(format!("{f} free"));
            if any_unalloc {
                items.push(format!("{un} unallocated"));
            }
        }
        out.push(String::new());
        out.push(items.join("   "));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Painter {
        Painter {
            mode: ColorMode::None,
        }
    }

    #[test]
    fn bar_width_is_exact() {
        let pal = Palette::new(Theme::Heat);
        for (used, other, unalloc) in [
            (0, 0, 0),
            (1, 0, 0),
            (50, 25, 0),
            (100, 0, 0),
            (30, 0, 20),
            (99, 1, 0),
        ] {
            let s = render_bar(40, used, other, unalloc, 100, &pal, &plain(), false);
            assert_eq!(
                disp_width(&s),
                40,
                "used={used} other={other} unalloc={unalloc}: {s:?}"
            );
        }
    }

    #[test]
    fn bar_segments() {
        let pal = Palette::new(Theme::Heat);
        assert_eq!(
            render_bar(10, 50, 0, 0, 100, &pal, &plain(), false),
            "█████░░░░░"
        );
        assert_eq!(
            render_bar(10, 50, 20, 0, 100, &pal, &plain(), false),
            "█████▒▒░░░"
        );
        assert_eq!(
            render_bar(10, 50, 0, 20, 100, &pal, &plain(), false),
            "█████░░░··"
        );
        assert_eq!(
            render_bar(10, 55, 0, 0, 100, &pal, &plain(), false),
            "█████▌░░░░"
        );
        assert_eq!(
            render_bar(10, 1, 0, 0, 1000, &pal, &plain(), false),
            "▏░░░░░░░░░"
        );
        assert_eq!(
            render_bar(10, 50, 20, 0, 100, &pal, &plain(), true),
            "#####++..."
        );
    }

    #[test]
    fn truncation() {
        assert_eq!(fit_left("/System/Volumes/Data", 10), "…umes/Data");
        assert_eq!(fit_right("Macintosh HD", 8), "Macinto…");
        assert_eq!(fit_right("abc", 5), "abc  ");
    }

    #[test]
    fn gradient_endpoints() {
        let pal = Palette::new(Theme::Heat);
        assert_eq!(pal.at(0.0), [0, 200, 160]);
        assert_eq!(pal.at(1.0), [240, 50, 90]);
        assert_eq!(pal.at(2.0), [240, 50, 90]);
    }

    #[test]
    fn ansi256_mapping() {
        assert_eq!(Painter::to256([0, 0, 0]), 232);
        assert_eq!(Painter::to256([255, 0, 0]), 196);
        assert_eq!(Painter::to256([52, 54, 62]), 237);
    }
}
