//! Discovering mounted filesystems and their usage.

use std::ffi::CString;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Volume {
    pub device: String,
    pub mount: String,
    pub fstype: String,
    /// Human volume name (macOS APFS), e.g. "Macintosh HD". May be empty.
    pub name: String,
    pub size: u64,
    pub used: u64,
    pub avail: u64,
    pub files: u64,
    pub ffree: u64,
    pub local: bool,
    /// The OS marks this volume as "don't show to users" (macOS MNT_DONTBROWSE).
    pub hidden_hint: bool,
}

pub const PSEUDO_FS: &[&str] = &[
    "autofs",
    "binfmt_misc",
    "bpf",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devfs",
    "devpts",
    "devtmpfs",
    "efivarfs",
    "fdescfs",
    "fusectl",
    "fuse.gvfsd-fuse",
    "fuse.portal",
    "fuse.snapfuse",
    "hugetlbfs",
    "kernfs",
    "linprocfs",
    "mqueue",
    "none",
    "nsfs",
    "proc",
    "procfs",
    "pstore",
    "ptyfs",
    "ramfs",
    "rpc_pipefs",
    "securityfs",
    "selinuxfs",
    "squashfs",
    "sysfs",
    "tmpfs",
    "tracefs",
];

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub const REMOTE_FS: &[&str] = &[
    "afpfs",
    "cifs",
    "fuse.sshfs",
    "ncpfs",
    "nfs",
    "nfs4",
    "smb3",
    "smbfs",
    "sshfs",
    "webdav",
    "9p",
];

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn is_remote(fstype: &str) -> bool {
    REMOTE_FS.contains(&fstype)
}

// ───────────────────────────── macOS ─────────────────────────────

/// Uses getfsstat(2) + getattrlist(2) so the numbers match macOS `df`.
///
/// APFS volumes share one container, so statfs' `f_blocks - f_bfree` is the
/// *container's* usage. macOS `df` reports `ATTR_VOL_SPACEUSED` instead, the
/// space used by that one volume, and so do we.
#[cfg(target_os = "macos")]
pub fn discover() -> Vec<Volume> {
    use std::ffi::CStr;

    fn cstr(arr: &[libc::c_char]) -> String {
        let bytes: Vec<u8> = arr
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn vol_attr(path: &CStr, bit: libc::attrgroup_t) -> Option<Vec<u8>> {
        let mut al: libc::attrlist = unsafe { std::mem::zeroed() };
        al.bitmapcount = 5; // ATTR_BIT_MAP_COUNT
        al.volattr = libc::ATTR_VOL_INFO | bit;
        let mut out = vec![0u8; 1024];
        let rc = unsafe {
            libc::getattrlist(
                path.as_ptr(),
                (&mut al as *mut libc::attrlist).cast(),
                out.as_mut_ptr().cast(),
                out.len(),
                0,
            )
        };
        (rc == 0).then_some(out)
    }

    let n = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if n <= 0 {
        return Vec::new();
    }
    let cap = n as usize + 8;
    let mut buf: Vec<libc::statfs> = Vec::with_capacity(cap);
    let bytes = (cap * std::mem::size_of::<libc::statfs>()) as libc::c_int;
    let n = unsafe { libc::getfsstat(buf.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
    if n <= 0 {
        return Vec::new();
    }
    // SAFETY: getfsstat initialised the first `n` entries (n <= cap).
    unsafe { buf.set_len((n as usize).min(cap)) };

    buf.iter()
        .map(|s| {
            let bsize = s.f_bsize as u64;
            let fstype = cstr(&s.f_fstypename);
            let mount = cstr(&s.f_mntonname);
            let mut used = s.f_blocks.saturating_sub(s.f_bfree) * bsize;
            let mut name = String::new();
            if fstype == "apfs" {
                if let Ok(cpath) = CString::new(mount.clone()) {
                    if let Some(raw) = vol_attr(&cpath, libc::ATTR_VOL_SPACEUSED) {
                        used = i64::from_ne_bytes(raw[4..12].try_into().unwrap()).max(0) as u64;
                    }
                    if let Some(raw) = vol_attr(&cpath, libc::ATTR_VOL_NAME) {
                        let off = i32::from_ne_bytes(raw[4..8].try_into().unwrap());
                        let len = u32::from_ne_bytes(raw[8..12].try_into().unwrap()) as usize;
                        let start = (4 + off as isize).max(0) as usize;
                        if let Some(field) = raw.get(start..(start + len).min(raw.len())) {
                            let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
                            name = String::from_utf8_lossy(&field[..end]).into_owned();
                        }
                    }
                }
            }
            Volume {
                device: cstr(&s.f_mntfromname),
                mount,
                fstype,
                name,
                size: s.f_blocks * bsize,
                used,
                avail: s.f_bavail * bsize,
                files: s.f_files,
                ffree: s.f_ffree,
                local: s.f_flags & libc::MNT_LOCAL as u32 != 0,
                hidden_hint: s.f_flags & libc::MNT_DONTBROWSE as u32 != 0,
            }
        })
        .collect()
}

// ───────────────────────── Linux / other Unix ─────────────────────────

#[cfg(not(target_os = "macos"))]
pub fn discover() -> Vec<Volume> {
    mount_table()
        .into_iter()
        .map(|(device, mount, fstype)| {
            let mut v = Volume {
                local: !is_remote(&fstype),
                device,
                mount,
                fstype,
                ..Default::default()
            };
            if let Some(st) = statvfs(&v.mount) {
                (v.size, v.used, v.avail, v.files, v.ffree) = st;
            }
            v
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn mount_table() -> Vec<(String, String, String)> {
    if let Ok(text) = std::fs::read_to_string("/proc/self/mounts") {
        return parse_proc_mounts(&text);
    }
    let out = std::process::Command::new("mount").output();
    match out {
        Ok(o) => parse_mount_output(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

/// Returns (size, used, avail, files, ffree).
#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_cast)] // field widths differ between platforms
fn statvfs(path: &str) -> Option<(u64, u64, u64, u64, u64)> {
    let c = CString::new(path).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let fr = if st.f_frsize != 0 {
        st.f_frsize as u64
    } else {
        st.f_bsize as u64
    };
    let blocks = st.f_blocks as u64;
    let bfree = st.f_bfree as u64;
    Some((
        blocks * fr,
        blocks.saturating_sub(bfree) * fr,
        st.f_bavail as u64 * fr,
        st.f_files as u64,
        st.f_ffree as u64,
    ))
}

/// Decode the `\040`-style octal escapes used in /proc/self/mounts.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn unescape_octal(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && i + 3 < b.len()
            && b[i + 1..=i + 3].iter().all(|c| (b'0'..=b'7').contains(c))
        {
            let v = (b[i + 1] - b'0') as u32 * 64
                + (b[i + 2] - b'0') as u32 * 8
                + (b[i + 3] - b'0') as u32;
            out.push(v as u8);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn parse_proc_mounts(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let (dev, mnt, ty) = (it.next()?, it.next()?, it.next()?);
            Some((unescape_octal(dev), unescape_octal(mnt), ty.to_string()))
        })
        .collect()
}

/// Parse `mount` output in either Linux (`dev on /mnt type ext4 (rw)`) or
/// BSD/macOS (`dev on /mnt (apfs, local)`) style.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn parse_mount_output(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((dev, rest)) = line.split_once(" on ") else {
            continue;
        };
        let Some(paren) = rest.rfind(" (") else {
            continue;
        };
        if !rest.ends_with(')') {
            continue;
        }
        let (head, opts) = (&rest[..paren], &rest[paren + 2..rest.len() - 1]);
        if let Some(tpos) = head.rfind(" type ") {
            out.push((
                dev.into(),
                head[..tpos].into(),
                head[tpos + 6..].trim().into(),
            ));
        } else {
            let ty = opts.split(',').next().unwrap_or("").trim();
            out.push((dev.into(), head.into(), ty.into()));
        }
    }
    out
}

// ───────────────────────────── filtering ─────────────────────────────

/// Keep what a person would call a "drive".
pub fn default_filter(vols: Vec<Volume>) -> Vec<Volume> {
    let mut seen = std::collections::HashSet::new();
    vols.into_iter()
        .filter(|v| {
            if PSEUDO_FS.contains(&v.fstype.as_str()) || v.size == 0 {
                return false;
            }
            if cfg!(target_os = "macos") {
                // Hide APFS system helper volumes but keep the user Data volume.
                return !(v.hidden_hint && v.mount != "/System/Volumes/Data");
            }
            if ["/snap/", "/var/lib/docker/", "/run/"]
                .iter()
                .any(|p| v.mount.starts_with(p))
            {
                return false;
            }
            // Bind mounts and btrfs subvolumes repeat the same device + numbers.
            !v.device.starts_with('/') || seen.insert((v.device.clone(), v.size, v.avail))
        })
        .collect()
}

fn under(real: &str, mount: &str) -> bool {
    real == mount || real.starts_with(&format!("{}/", mount.trim_end_matches('/')))
}

/// Index of the volume holding `path`.
pub fn volume_for_path(path: &str, vols: &[Volume]) -> Result<usize, String> {
    let err = |e: std::io::Error| {
        let msg = e.to_string();
        let msg = msg.split(" (os error").next().unwrap_or(&msg).to_string();
        format!("{path}: {msg}")
    };
    let dev = std::fs::metadata(path).map_err(err)?.dev();
    let real = std::fs::canonicalize(path)
        .map_err(err)?
        .to_string_lossy()
        .into_owned();

    let same_dev: Vec<usize> = (0..vols.len())
        .filter(|&i| {
            std::fs::metadata(Path::new(&vols[i].mount))
                .map(|m| m.dev() == dev)
                .unwrap_or(false)
        })
        .collect();
    let longest = |idx: &mut dyn Iterator<Item = usize>| idx.max_by_key(|&i| vols[i].mount.len());

    if same_dev.is_empty() {
        return longest(&mut (0..vols.len()).filter(|&i| under(&real, &vols[i].mount)))
            .ok_or_else(|| format!("{path}: no filesystem found"));
    }
    let prefixed: Vec<usize> = same_dev
        .iter()
        .copied()
        .filter(|&i| under(&real, &vols[i].mount))
        .collect();
    let pool = if prefixed.is_empty() {
        same_dev
    } else {
        prefixed
    };
    Ok(longest(&mut pool.into_iter()).unwrap())
}

/// Key identifying the storage pool a volume draws from, so totals count a
/// shared APFS container (or identical-looking pool) only once.
pub fn pool_key(v: &Volume, size: u64, avail: u64) -> String {
    if v.fstype == "apfs" {
        if let Some(rest) = v.device.strip_prefix("/dev/disk") {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            let tail = &rest[digits.len()..];
            if !digits.is_empty()
                && tail.starts_with('s')
                && tail[1..].starts_with(|c: char| c.is_ascii_digit())
            {
                return format!("disk{digits}");
            }
        }
    }
    format!("{size}:{avail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_unescape() {
        assert_eq!(unescape_octal(r"/mnt/My\040Drive"), "/mnt/My Drive");
        assert_eq!(unescape_octal(r"plain"), "plain");
        assert_eq!(unescape_octal(r"trailing\04"), r"trailing\04");
    }

    #[test]
    fn proc_mounts() {
        let m = parse_proc_mounts("/dev/sda1 / ext4 rw 0 0\nproc /proc proc rw 0 0\n");
        assert_eq!(m[0], ("/dev/sda1".into(), "/".into(), "ext4".into()));
        assert_eq!(m[1].2, "proc");
    }

    #[test]
    fn mount_output_both_styles() {
        let m = parse_mount_output(
            "/dev/disk3s1 on /System/Volumes/Data (apfs, local, journaled)\n\
             map auto_home on /System/Volumes/Data/home (autofs, automounted)\n\
             /dev/sdb1 on /media/My Disk type vfat (rw,nosuid)\n",
        );
        assert_eq!(
            m[0],
            (
                "/dev/disk3s1".into(),
                "/System/Volumes/Data".into(),
                "apfs".into()
            )
        );
        assert_eq!(
            m[1],
            (
                "map auto_home".into(),
                "/System/Volumes/Data/home".into(),
                "autofs".into()
            )
        );
        assert_eq!(
            m[2],
            ("/dev/sdb1".into(), "/media/My Disk".into(), "vfat".into())
        );
    }

    #[test]
    fn apfs_pool_keys() {
        let v = |d: &str| Volume {
            device: d.into(),
            fstype: "apfs".into(),
            ..Default::default()
        };
        assert_eq!(pool_key(&v("/dev/disk3s3s1"), 1, 2), "disk3");
        assert_eq!(pool_key(&v("/dev/disk3s1"), 1, 2), "disk3");
        assert_eq!(pool_key(&v("/dev/disk12s4"), 1, 2), "disk12");
        assert_eq!(pool_key(&v("/dev/disk3"), 1, 2), "1:2");
    }

    #[test]
    fn filter_drops_pseudo_and_empty() {
        let mk = |m: &str, t: &str, size| Volume {
            mount: m.into(),
            fstype: t.into(),
            size,
            ..Default::default()
        };
        let kept = default_filter(vec![
            mk("/", "ext4", 10),
            mk("/dev", "devfs", 10),
            mk("/x", "ext4", 0),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].mount, "/");
    }

    #[test]
    fn discovers_root() {
        let vols = discover();
        assert!(
            vols.iter().any(|v| v.mount == "/"),
            "root filesystem not found"
        );
        let i = volume_for_path("/", &vols).unwrap();
        assert_eq!(vols[i].mount, "/");
    }
}
