//! vdf — visual disk free.
//!
//! Like `df -h`, but disk-centric: every physical disk gets a colour-gradient
//! capacity bar with the volumes mounted from it listed underneath.

mod disk;
mod render;
mod units;
mod volume;

use std::collections::HashMap;
use std::io::{IsTerminal, Write};

use clap::{Parser, ValueEnum};
use render::{ColorMode, Line, Opts, Painter, Palette, Row, Theme};
use units::percent;
use volume::Volume;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum SortKey {
    Mount,
    Size,
    Used,
    Avail,
    Pct,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
    #[value(name = "256")]
    Ansi256,
    Truecolor,
}

/// Visual disk free: df -h with colour-gradient usage bars.
///
/// By default each physical disk is shown with its own capacity bar, and the
/// volumes mounted from it are listed underneath.
#[derive(Parser, Debug)]
#[command(name = "vdf", version, about, long_about = None)]
struct Args {
    /// Show only the filesystems containing these paths
    paths: Vec<String>,

    /// Physical disks only (no volume rows)
    #[arg(short = 'd', long, conflicts_with = "flat")]
    disks: bool,

    /// Flat list of volumes, without grouping by disk (classic df layout)
    #[arg(short = 'f', long)]
    flat: bool,

    /// Include pseudo, hidden and duplicate filesystems
    #[arg(short, long)]
    all: bool,

    /// Also show device and filesystem type
    #[arg(short, long)]
    long: bool,

    /// Show volume names instead of mount points (macOS)
    #[arg(short, long)]
    names: bool,

    /// Show inode usage instead of block usage (implies --flat)
    #[arg(short, long)]
    inodes: bool,

    /// Powers of 1000 instead of 1024
    #[arg(short = 'H', long)]
    si: bool,

    /// Only local filesystems
    #[arg(short = 'L', long)]
    local: bool,

    /// Only show this filesystem type (repeatable)
    #[arg(short = 't', long = "type", value_name = "TYPE")]
    types: Vec<String>,

    /// Hide this filesystem type (repeatable)
    #[arg(short = 'x', long = "exclude-type", value_name = "TYPE")]
    exclude_types: Vec<String>,

    /// Sort volumes [default: system order]
    #[arg(short, long, value_enum, default_value_t = SortKey::None, hide_default_value = true)]
    sort: SortKey,

    /// Reverse the sort order
    #[arg(short, long)]
    reverse: bool,

    /// Add a grand-total row
    #[arg(short = 'T', long)]
    total: bool,

    /// Gradient colours [default: heat, or $VDF_THEME]
    #[arg(long, value_enum)]
    theme: Option<Theme>,

    /// Colour output
    #[arg(long, value_enum, default_value_t = ColorChoice::Auto)]
    color: ColorChoice,

    /// Plain ASCII bar characters
    #[arg(long)]
    ascii: bool,

    /// Total output width [default: terminal width]
    #[arg(short, long, default_value_t = 0, hide_default_value = true)]
    width: usize,

    /// Maximum bar width in columns
    #[arg(long, default_value_t = 80, value_name = "N")]
    bar_max: usize,

    /// Maximum width of the name column
    #[arg(long, default_value_t = 32, value_name = "N")]
    max_name: usize,

    /// Flag usage at or above this percentage with "!"
    #[arg(long, default_value_t = 90, value_name = "PCT")]
    warn: u64,

    /// Omit the header row
    #[arg(long)]
    no_header: bool,

    /// Omit the legend
    #[arg(long)]
    no_legend: bool,

    /// Machine-readable output
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Grouped,
    Disks,
    Flat,
}

fn detect_color(choice: ColorChoice) -> ColorMode {
    match choice {
        ColorChoice::Never => return ColorMode::None,
        ColorChoice::Truecolor => return ColorMode::TrueColor,
        ColorChoice::Ansi256 => return ColorMode::Ansi256,
        ColorChoice::Auto => {
            if !std::io::stdout().is_terminal()
                || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
                || std::env::var("TERM").is_ok_and(|t| t == "dumb")
            {
                return ColorMode::None;
            }
        }
        ColorChoice::Always => {}
    }
    let ct = std::env::var("COLORTERM")
        .unwrap_or_default()
        .to_lowercase();
    let tp = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let known = [
        "iTerm.app",
        "WezTerm",
        "vscode",
        "ghostty",
        "Hyper",
        "Tabby",
        "WarpTerminal",
        "rio",
    ];
    if ct == "truecolor" || ct == "24bit" || known.contains(&tp.as_str()) {
        ColorMode::TrueColor
    } else {
        ColorMode::Ansi256
    }
}

fn terminal_width() -> usize {
    if let Some(n) = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse::<usize>().ok())
        .filter(|&n| n > 0)
    {
        return n;
    }
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0
    {
        return ws.ws_col as usize;
    }
    100
}

/// Block or inode numbers for a volume: (size, used, avail, other).
fn numbers(v: &Volume, inodes: bool) -> (u64, u64, u64, u64) {
    if inodes {
        (v.files, v.files.saturating_sub(v.ffree), v.ffree, 0)
    } else {
        (
            v.size,
            v.used,
            v.avail,
            v.size.saturating_sub(v.used + v.avail),
        )
    }
}

fn volume_row(v: &Volume, a: &Args, prefix: &str) -> Row {
    let (size, used, avail, other) = numbers(v, a.inodes);
    let named = a.names && !v.name.is_empty();
    Row {
        prefix: prefix.into(),
        label: if named {
            v.name.clone()
        } else {
            v.mount.clone()
        },
        trunc_left: !named,
        strong: true,
        device: v.device.clone(),
        fstype: v.fstype.clone(),
        size,
        used,
        avail,
        other,
        unalloc: 0,
        pct: percent(used, avail),
        unknown: false,
    }
}

/// Physical-disk usage: free = space available to the filesystems on it
/// (each APFS container / partition counted once); used = everything else
/// that is partitioned.
fn disk_row(d: &disk::Disk, on_disk: &[&Volume], label: String, prefix: &str) -> Row {
    let mut pools: HashMap<String, u64> = HashMap::new();
    for v in on_disk {
        let key = if v.fstype == "apfs" {
            volume::pool_key(v, v.size, v.avail)
        } else {
            v.device.clone()
        };
        let e = pools.entry(key).or_default();
        *e = (*e).max(v.avail);
    }
    let unalloc = d.unallocated();
    let free = pools.values().sum::<u64>().min(d.size - unalloc);
    let used = d.size - unalloc - free;
    let unknown = on_disk.is_empty();
    Row {
        prefix: prefix.into(),
        label,
        trunc_left: false,
        strong: false,
        device: format!("/dev/{}", d.id),
        fstype: "disk".into(),
        size: d.size,
        used: if unknown { 0 } else { used },
        avail: if unknown { 0 } else { free },
        other: if unknown { d.size - unalloc } else { 0 },
        unalloc,
        pct: if unknown {
            0
        } else {
            percent(used, free + unalloc)
        },
        unknown,
    }
}

fn sort_volumes(vols: &mut [&Volume], a: &Args) {
    let key = |v: &Volume| -> (u64, String) {
        let (size, used, avail, _) = numbers(v, a.inodes);
        match a.sort {
            SortKey::Size => (size, String::new()),
            SortKey::Used => (used, String::new()),
            SortKey::Avail => (avail, String::new()),
            SortKey::Pct => (percent(used, avail), String::new()),
            SortKey::Mount => (0, v.mount.clone()),
            SortKey::None => (0, String::new()),
        }
    };
    if a.sort != SortKey::None {
        if a.reverse {
            vols.sort_by_key(|v| std::cmp::Reverse(key(v)));
        } else {
            vols.sort_by_key(|v| key(v));
        }
    } else if a.reverse {
        vols.reverse();
    }
}

fn total_row(rows: &[Row], disk_view: bool, vols: &[&Volume], a: &Args) -> Row {
    let (size, avail, unalloc) = if disk_view {
        (
            rows.iter().map(|r| r.size).sum(),
            rows.iter().map(|r| r.avail).sum(),
            rows.iter().map(|r| r.unalloc).sum(),
        )
    } else {
        // Volumes in one APFS container (or pool) share size and free space.
        let mut pools: HashMap<String, (u64, u64)> = HashMap::new();
        for v in vols {
            let (s, _, av, _) = numbers(v, a.inodes);
            let e = pools.entry(volume::pool_key(v, s, av)).or_default();
            *e = (e.0.max(s), e.1.max(av));
        }
        (
            pools.values().map(|p| p.0).sum(),
            pools.values().map(|p| p.1).sum(),
            0,
        )
    };
    let used: u64 = rows.iter().map(|r| r.used).sum();
    Row {
        label: if disk_view {
            "all disks".into()
        } else {
            "total".into()
        },
        strong: true,
        device: "total".into(),
        fstype: "-".into(),
        size,
        used,
        avail,
        other: if disk_view {
            0
        } else {
            size.saturating_sub(used + avail)
        },
        unalloc,
        pct: percent(used, avail + unalloc),
        ..Default::default()
    }
}

fn json_output(shown: &[&Volume], dm: &disk::DiskMap, all: &[Volume], a: &Args) -> String {
    use serde_json::{Value, json};
    let disks: Vec<Value> = dm
        .disks
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let on: Vec<&Volume> = all
                .iter()
                .filter(|v| dm.disk_of(&v.device) == Some(i))
                .collect();
            let r = disk_row(d, &on, String::new(), "");
            json!({
                "id": d.id, "model": d.model, "interconnect": d.interconnect,
                "internal": d.internal, "solid_state": d.solid_state, "virtual": d.is_virtual,
                "size": r.size,
                "used": (!r.unknown).then_some(r.used),
                "free": (!r.unknown).then_some(r.avail),
                "unallocated": r.unalloc,
                "use_percent": (!r.unknown).then_some(r.pct),
                "mounts": on.iter().map(|v| v.mount.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    let volumes: Vec<Value> = shown
        .iter()
        .map(|v| {
            let (size, used, avail, other) = numbers(v, a.inodes);
            json!({
                "filesystem": v.device, "mount": v.mount,
                "name": if v.name.is_empty() { Value::Null } else { v.name.clone().into() },
                "type": v.fstype,
                "disk": dm.disk_of(&v.device).map(|i| dm.disks[i].id.clone()),
                "size": size, "used": used, "avail": avail, "other": other,
                "use_percent": percent(used, avail),
                "unit": if a.inodes { "inodes" } else { "bytes" },
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "disks": disks, "volumes": volumes })).unwrap()
}

fn run(a: Args) -> Result<i32, String> {
    let all = volume::discover();

    let mut shown: Vec<&Volume> = if !a.paths.is_empty() {
        let mut picked: Vec<usize> = Vec::new();
        for path in &a.paths {
            let i = volume::volume_for_path(path, &all)?;
            if !picked.contains(&i) {
                picked.push(i);
            }
        }
        picked.into_iter().map(|i| &all[i]).collect()
    } else if a.all {
        all.iter().collect()
    } else {
        let keep: Vec<String> = volume::default_filter(all.clone())
            .into_iter()
            .map(|v| v.mount)
            .collect();
        all.iter().filter(|v| keep.contains(&v.mount)).collect()
    };
    if a.local {
        shown.retain(|v| v.local);
    }
    if !a.types.is_empty() {
        shown.retain(|v| a.types.contains(&v.fstype));
    }
    if !a.exclude_types.is_empty() {
        shown.retain(|v| !a.exclude_types.contains(&v.fstype));
    }
    sort_volumes(&mut shown, &a);

    let mut view = if a.inodes || a.flat || !a.paths.is_empty() {
        View::Flat
    } else if a.disks {
        View::Disks
    } else {
        View::Grouped
    };
    let dm = if view != View::Flat || a.json {
        disk::discover()
    } else {
        disk::DiskMap::default()
    };
    if dm.disks.is_empty() && view != View::Flat {
        view = View::Flat; // no disk information on this system
    }

    if a.json {
        println!("{}", json_output(&shown, &dm, &all, &a));
        return Ok(0);
    }

    let mut lines: Vec<Line> = Vec::new();
    let mut top_rows: Vec<Row> = Vec::new();
    match view {
        View::Flat => {
            for v in &shown {
                let r = volume_row(v, &a, "");
                top_rows.push(r.clone());
                lines.push(Line::Row(r));
            }
        }
        View::Disks | View::Grouped => {
            let mut placed = vec![false; shown.len()];
            for (i, d) in dm.disks.iter().enumerate() {
                let on_disk: Vec<&Volume> = all
                    .iter()
                    .filter(|v| dm.disk_of(&v.device) == Some(i))
                    .collect();
                let members: Vec<usize> = (0..shown.len())
                    .filter(|&k| dm.disk_of(&shown[k].device) == Some(i))
                    .collect();
                if !a.all && members.is_empty() && (d.is_virtual || !a.types.is_empty() || a.local)
                {
                    continue;
                }
                members.iter().for_each(|&k| placed[k] = true);
                if view == View::Disks {
                    let label = if d.model.is_empty() {
                        d.id.clone()
                    } else {
                        format!("{} {}", d.id, d.model)
                    };
                    let mut r = disk_row(d, &on_disk, label, "");
                    r.strong = true;
                    top_rows.push(r.clone());
                    lines.push(Line::Row(r));
                    continue;
                }
                if !lines.is_empty() {
                    lines.push(Line::Blank);
                }
                lines.push(Line::Title(d.id.clone(), d.caption()));
                let label = if d.is_virtual {
                    "disk image"
                } else {
                    "entire disk"
                };
                let r = disk_row(d, &on_disk, label.into(), "  ");
                top_rows.push(r.clone());
                lines.push(Line::Row(r));
                for (n, &k) in members.iter().enumerate() {
                    let glyph = if n + 1 == members.len() {
                        "  └─ "
                    } else {
                        "  ├─ "
                    };
                    lines.push(Line::Row(volume_row(shown[k], &a, glyph)));
                }
                if members.is_empty() {
                    lines.push(Line::Title("  └─".into(), "no mounted volumes".into()));
                }
            }
            let rest: Vec<&Volume> = (0..shown.len())
                .filter(|&k| !placed[k])
                .map(|k| shown[k])
                .collect();
            if view == View::Grouped && !rest.is_empty() {
                if !lines.is_empty() {
                    lines.push(Line::Blank);
                }
                lines.push(Line::Title(
                    "other".into(),
                    "network, virtual and unmapped filesystems".into(),
                ));
                for (n, v) in rest.iter().enumerate() {
                    let glyph = if n + 1 == rest.len() {
                        "  └─ "
                    } else {
                        "  ├─ "
                    };
                    lines.push(Line::Row(volume_row(v, &a, glyph)));
                }
            }
        }
    }

    if lines.is_empty() {
        eprintln!("vdf: no filesystems to show");
        return Ok(1);
    }
    if a.total {
        let t = total_row(&top_rows, view != View::Flat, &shown, &a);
        lines.push(Line::Rule);
        lines.push(Line::Row(t));
    }

    let theme = a.theme.or_else(|| {
        std::env::var("VDF_THEME")
            .ok()
            .and_then(|t| Theme::from_str(&t, true).ok())
    });
    let opts = Opts {
        cols: if a.width > 0 {
            a.width
        } else {
            terminal_width()
        },
        head_name: match view {
            View::Disks => "Disk".into(),
            View::Grouped => "Disk / volume".into(),
            View::Flat if a.names => "Volume".into(),
            View::Flat => "Mounted on".into(),
        },
        long: a.long,
        inodes: a.inodes,
        si: a.si,
        ascii: a.ascii,
        bar_max: a.bar_max,
        max_name: a.max_name.max(8),
        warn: a.warn,
        header: !a.no_header,
        legend: !a.no_legend,
    };
    let painter = Painter {
        mode: detect_color(a.color),
    };
    let text = render::render(
        &lines,
        &opts,
        &Palette::new(theme.unwrap_or(Theme::Heat)),
        &painter,
    );

    let mut out = std::io::stdout().lock();
    // Ignore EPIPE (e.g. `vdf | head`).
    let _ = writeln!(out, "{text}").and_then(|_| out.flush());
    Ok(0)
}

fn main() {
    let code = match run(Args::parse()) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("vdf: {msg}");
            1
        }
    };
    std::process::exit(code);
}
