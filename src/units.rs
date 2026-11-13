//! Number formatting in the style of `df -h`.

/// `df -h` style size: 512B, 6.0M, 11G, 460G.
pub fn human(n: u64, si: bool) -> String {
    let base = if si { 1000.0 } else { 1024.0 };
    if (n as f64) < base {
        return format!("{n}B");
    }
    const UNITS: [char; 6] = ['K', 'M', 'G', 'T', 'P', 'E'];
    let mut x = n as f64;
    let mut i = 0usize;
    x /= base;
    while x >= base && i < UNITS.len() - 1 {
        x /= base;
        i += 1;
    }
    if x < 9.95 {
        return format!("{x:.1}{}", UNITS[i]);
    }
    let r = x.round_ties_even();
    if r >= base && i < UNITS.len() - 1 {
        return format!("1.0{}", UNITS[i + 1]);
    }
    format!("{}{}", r as u64, UNITS[i])
}

/// Compact count for inode numbers: 451k, 2.8M, 1.2G.
pub fn human_count(n: u64) -> String {
    if n < 1000 {
        return n.to_string();
    }
    let mut x = n as f64;
    for u in ['k', 'M', 'G', 'T', 'P'] {
        x /= 1000.0;
        if x < 1000.0 {
            return if x < 10.0 {
                format!("{x:.1}{u}")
            } else {
                format!("{}{u}", x.round() as u64)
            };
        }
    }
    format!("{}E", x as u64)
}

/// Use% the way df computes it: used / (used + avail), rounded up.
pub fn percent(used: u64, avail: u64) -> u64 {
    let tot = used as u128 + avail as u128;
    if tot == 0 {
        return 0;
    }
    ((used as u128 * 100).div_ceil(tot)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human(0, false), "0B");
        assert_eq!(human(1023, false), "1023B");
        assert_eq!(human(1024, false), "1.0K");
        assert_eq!(human(6_291_456, false), "6.0M");
        assert_eq!(human(12_185_608_192, false), "11G");
        assert_eq!(human(494_384_795_648, false), "460G");
        assert_eq!(human(1023 * 1024 * 1024 + 1000 * 1024, false), "1.0G");
        assert_eq!(human(1_000_000, true), "1.0M");
    }

    #[test]
    fn counts() {
        assert_eq!(human_count(999), "999");
        assert_eq!(human_count(451_498), "451k");
        assert_eq!(human_count(2_787_220), "2.8M");
    }

    #[test]
    fn percent_rounds_up_like_df() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(1, 999), 1);
        assert_eq!(percent(11, 112), 9);
        assert_eq!(percent(12, 112), 10);
        assert_eq!(percent(5, 0), 100);
    }
}
