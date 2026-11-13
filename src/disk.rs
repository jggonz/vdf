//! Physical disks (the media under the mounted volumes).
//!
//! macOS: walks the IOKit registry (IOMedia objects and their parents), so an
//! APFS volume such as `disk3s1` resolves through its container `disk3` and
//! physical store `disk0s2` to the physical disk `disk0`.
//! Linux: reads /sys/block, following partitions and device-mapper / md
//! slaves back to the whole disk.

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Disk {
    /// Kernel name: `disk0`, `sda`, `nvme0n1`.
    pub id: String,
    pub model: String,
    pub size: u64,
    /// Sum of partition sizes; equals `size` when there is no partition table.
    pub partitioned: u64,
    /// Some(true) internal, Some(false) external/removable, None unknown.
    pub internal: Option<bool>,
    pub solid_state: Option<bool>,
    /// "PCI-Express", "USB", "SATA", "Apple Fabric", ...
    pub interconnect: String,
    /// Disk images and other non-physical media.
    pub is_virtual: bool,
}

impl Disk {
    /// Space not covered by any partition (ignores GPT/alignment slack).
    pub fn unallocated(&self) -> u64 {
        let gap = self.size.saturating_sub(self.partitioned);
        if gap < 64 << 20 { 0 } else { gap }
    }

    pub fn caption(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.model.is_empty() {
            parts.push(self.model.clone());
        }
        if self.is_virtual {
            parts.push("disk image".into());
        } else {
            match self.internal {
                Some(true) => parts.push("internal".into()),
                Some(false) => parts.push("external".into()),
                None => {}
            }
            match self.solid_state {
                Some(true) => parts.push("SSD".into()),
                Some(false) => parts.push("HDD".into()),
                None => {}
            }
        }
        if !self.interconnect.is_empty() && !self.is_virtual {
            parts.push(self.interconnect.clone());
        }
        parts.join(" · ")
    }
}

#[derive(Debug, Default)]
pub struct DiskMap {
    pub disks: Vec<Disk>,
    /// Every known block-device name (disks, partitions, containers) → index in `disks`.
    owner: HashMap<String, usize>,
}

impl DiskMap {
    /// Which physical disk holds the filesystem on `device` (e.g. `/dev/disk3s1`).
    pub fn disk_of(&self, device: &str) -> Option<usize> {
        #[allow(unused_mut)]
        let mut name = device.strip_prefix("/dev/")?.to_string();
        #[cfg(target_os = "linux")]
        if let Ok(real) = std::fs::canonicalize(device) {
            if let Some(base) = real.file_name() {
                name = base.to_string_lossy().into_owned();
            }
        }
        loop {
            if let Some(&i) = self.owner.get(&name) {
                return Some(i);
            }
            // APFS snapshots (disk3s3s1) may not be registered; drop "sN" and retry.
            let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit());
            if trimmed.len() < name.len() && trimmed.ends_with('s') && trimmed.len() > 1 {
                name = trimmed[..trimmed.len() - 1].to_string();
            } else {
                return None;
            }
        }
    }
}

/// Natural order: disk2 < disk10, sda < sdb.
fn natural_key(s: &str) -> (String, u64) {
    let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    let n = digits
        .chars()
        .rev()
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    (s[..s.len() - digits.len()].to_string(), n)
}

fn finish(mut disks: Vec<Disk>, names: HashMap<String, String>) -> DiskMap {
    disks.sort_by_key(|d| natural_key(&d.id));
    let index: HashMap<String, usize> = disks
        .iter()
        .enumerate()
        .map(|(i, d)| (d.id.clone(), i))
        .collect();
    let owner = names
        .into_iter()
        .filter_map(|(name, root)| index.get(&root).map(|&i| (name, i)))
        .collect();
    DiskMap { disks, owner }
}

// ───────────────────────────── macOS (IOKit) ─────────────────────────────

#[cfg(target_os = "macos")]
mod iokit {
    #![allow(non_upper_case_globals, non_camel_case_types)]
    use std::ffi::{CStr, CString, c_char, c_void};

    pub type CFTypeRef = *const c_void;
    pub type io_object_t = u32;
    pub type kern_return_t = i32;

    pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;
    pub const kCFNumberSInt64Type: isize = 4;
    pub const kIORegistryIterateRecursively: u32 = 1;
    pub const kIORegistryIterateParents: u32 = 2;
    pub const PLANE: &CStr = c"IOService";

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, enc: u32) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, enc: u32) -> u8;
        fn CFNumberGetValue(n: CFTypeRef, ty: isize, out: *mut c_void) -> u8;
        fn CFBooleanGetValue(b: CFTypeRef) -> u8;
        fn CFDictionaryGetValue(d: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        pub fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        pub fn IOServiceGetMatchingServices(
            main: u32,
            matching: *mut c_void,
            it: *mut io_object_t,
        ) -> kern_return_t;
        pub fn IOIteratorNext(it: io_object_t) -> io_object_t;
        pub fn IOObjectRelease(o: io_object_t) -> kern_return_t;
        pub fn IOObjectConformsTo(o: io_object_t, class: *const c_char) -> u32;
        pub fn IORegistryEntryGetParentEntry(
            e: io_object_t,
            plane: *const c_char,
            parent: *mut io_object_t,
        ) -> kern_return_t;
        pub fn IORegistryEntryGetRegistryEntryID(e: io_object_t, id: *mut u64) -> kern_return_t;
        pub fn IORegistryEntryGetName(e: io_object_t, name: *mut c_char) -> kern_return_t;
        fn IORegistryEntryCreateCFProperty(
            e: io_object_t,
            key: CFTypeRef,
            alloc: CFTypeRef,
            opts: u32,
        ) -> CFTypeRef;
        fn IORegistryEntrySearchCFProperty(
            e: io_object_t,
            plane: *const c_char,
            key: CFTypeRef,
            alloc: CFTypeRef,
            opts: u32,
        ) -> CFTypeRef;
    }

    /// Owned CoreFoundation object, released on drop.
    pub struct Cf(pub CFTypeRef);
    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn cfstr(s: &str) -> Cf {
        let c = CString::new(s).unwrap();
        Cf(unsafe {
            CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), kCFStringEncodingUTF8)
        })
    }

    pub fn as_string(v: CFTypeRef) -> Option<String> {
        if v.is_null() || unsafe { CFGetTypeID(v) != CFStringGetTypeID() } {
            return None;
        }
        let mut buf = vec![0 as c_char; 512];
        let ok = unsafe {
            CFStringGetCString(
                v,
                buf.as_mut_ptr(),
                buf.len() as isize,
                kCFStringEncodingUTF8,
            )
        };
        (ok != 0).then(|| {
            unsafe { CStr::from_ptr(buf.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        })
    }

    pub fn as_i64(v: CFTypeRef) -> Option<i64> {
        if v.is_null() || unsafe { CFGetTypeID(v) != CFNumberGetTypeID() } {
            return None;
        }
        let mut out: i64 = 0;
        let ok = unsafe { CFNumberGetValue(v, kCFNumberSInt64Type, (&mut out as *mut i64).cast()) };
        (ok != 0).then_some(out)
    }

    pub fn as_bool(v: CFTypeRef) -> Option<bool> {
        if v.is_null() || unsafe { CFGetTypeID(v) != CFBooleanGetTypeID() } {
            return None;
        }
        Some(unsafe { CFBooleanGetValue(v) } != 0)
    }

    /// Borrowed value from a CFDictionary (not retained).
    pub fn dict_get(d: CFTypeRef, key: &str) -> CFTypeRef {
        if d.is_null() || unsafe { CFGetTypeID(d) != CFDictionaryGetTypeID() } {
            return std::ptr::null();
        }
        let k = cfstr(key);
        unsafe { CFDictionaryGetValue(d, k.0) }
    }

    pub fn prop(e: io_object_t, key: &str) -> Cf {
        let k = cfstr(key);
        Cf(unsafe { IORegistryEntryCreateCFProperty(e, k.0, std::ptr::null(), 0) })
    }

    /// Search this entry and its ancestors for a property.
    pub fn search_up(e: io_object_t, key: &str) -> Cf {
        let k = cfstr(key);
        Cf(unsafe {
            IORegistryEntrySearchCFProperty(
                e,
                PLANE.as_ptr(),
                k.0,
                std::ptr::null(),
                kIORegistryIterateRecursively | kIORegistryIterateParents,
            )
        })
    }

    pub fn entry_id(e: io_object_t) -> u64 {
        let mut id = 0;
        unsafe { IORegistryEntryGetRegistryEntryID(e, &mut id) };
        id
    }

    pub fn entry_name(e: io_object_t) -> String {
        let mut buf = [0 as c_char; 128];
        if unsafe { IORegistryEntryGetName(e, buf.as_mut_ptr()) } != 0 {
            return String::new();
        }
        unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    /// Registry ID of the nearest IOMedia ancestor, if any.
    pub fn parent_media(e: io_object_t) -> Option<u64> {
        let mut cur = e;
        let mut owned = false;
        let mut found = None;
        for _ in 0..64 {
            let mut parent: io_object_t = 0;
            let kr = unsafe { IORegistryEntryGetParentEntry(cur, PLANE.as_ptr(), &mut parent) };
            if owned {
                unsafe { IOObjectRelease(cur) };
                owned = false;
            }
            if kr != 0 || parent == 0 {
                break;
            }
            cur = parent;
            owned = true;
            if unsafe { IOObjectConformsTo(cur, c"IOMedia".as_ptr()) } != 0 {
                found = Some(entry_id(cur));
                break;
            }
        }
        if owned {
            unsafe { IOObjectRelease(cur) };
        }
        found
    }
}

#[cfg(target_os = "macos")]
pub fn discover() -> DiskMap {
    use iokit::*;

    struct Media {
        parent: Option<u64>,
        bsd: String,
        size: u64,
        whole: bool,
        disk: Option<Disk>, // filled in for root media
    }

    let mut media: HashMap<u64, Media> = HashMap::new();
    unsafe {
        let matching = IOServiceMatching(c"IOMedia".as_ptr());
        let mut it: io_object_t = 0;
        if matching.is_null() || IOServiceGetMatchingServices(0, matching, &mut it) != 0 {
            return DiskMap::default();
        }
        loop {
            let e = IOIteratorNext(it);
            if e == 0 {
                break;
            }
            let bsd = as_string(prop(e, "BSD Name").0).unwrap_or_default();
            let size = as_i64(prop(e, "Size").0).unwrap_or(0).max(0) as u64;
            let whole = as_bool(prop(e, "Whole").0).unwrap_or(false);
            let parent = parent_media(e);
            let disk = parent.is_none().then(|| {
                let protocol = search_up(e, "Protocol Characteristics");
                let device = search_up(e, "Device Characteristics");
                let location = as_string(dict_get(protocol.0, "Physical Interconnect Location"))
                    .unwrap_or_default();
                let interconnect =
                    as_string(dict_get(protocol.0, "Physical Interconnect")).unwrap_or_default();
                let medium = as_string(dict_get(device.0, "Medium Type")).unwrap_or_default();
                let name = entry_name(e);
                let model = name
                    .strip_suffix(" Media")
                    .unwrap_or(&name)
                    .trim()
                    .to_string();
                let is_virtual =
                    interconnect.contains("Virtual") || location == "File" || location == "Virtual";
                Disk {
                    id: bsd.clone(),
                    model,
                    size,
                    partitioned: 0,
                    internal: match location.as_str() {
                        "Internal" => Some(true),
                        "External" => Some(false),
                        _ => None,
                    },
                    solid_state: match medium.as_str() {
                        "Solid State" => Some(true),
                        "Rotational" => Some(false),
                        _ => None,
                    },
                    interconnect,
                    is_virtual,
                }
            });
            media.insert(
                entry_id(e),
                Media {
                    parent,
                    bsd,
                    size,
                    whole,
                    disk,
                },
            );
            IOObjectRelease(e);
        }
        IOObjectRelease(it);
    }

    let root_of = |mut id: u64| -> u64 {
        for _ in 0..64 {
            match media.get(&id).and_then(|m| m.parent) {
                Some(p) if media.contains_key(&p) => id = p,
                _ => break,
            }
        }
        id
    };

    let mut partitioned: HashMap<u64, u64> = HashMap::new();
    let mut names = HashMap::new();
    for (&id, m) in &media {
        let root = root_of(id);
        if let Some(r) = media.get(&root) {
            if !m.bsd.is_empty() && !r.bsd.is_empty() {
                names.insert(m.bsd.clone(), r.bsd.clone());
            }
        }
        // Direct partitions of a root disk.
        if m.parent == Some(root) && root != id && !m.whole {
            *partitioned.entry(root).or_default() += m.size;
        }
    }

    let disks = media
        .iter()
        .filter_map(|(id, m)| {
            let mut d = m.disk.clone()?;
            if d.id.is_empty() || d.size == 0 {
                return None;
            }
            d.partitioned = partitioned
                .get(id)
                .copied()
                .filter(|&p| p > 0)
                .unwrap_or(d.size);
            Some(d)
        })
        .collect();
    finish(disks, names)
}

// ───────────────────────────── Linux (sysfs) ─────────────────────────────

#[cfg(target_os = "linux")]
pub fn discover() -> DiskMap {
    discover_sysfs(std::path::Path::new("/sys"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn discover() -> DiskMap {
    DiskMap::default()
}

#[allow(dead_code)]
fn read_trim(p: &std::path::Path) -> String {
    std::fs::read_to_string(p)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Parse a sysfs tree rooted at `sys` (normally `/sys`).
#[allow(dead_code)]
pub fn discover_sysfs(sys: &std::path::Path) -> DiskMap {
    use std::fs;
    const VIRTUAL: &[&str] = &["loop", "ram", "zram", "dm-", "md", "nbd", "fd", "sr", "zd"];

    let block = sys.join("block");
    let mut disks = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();
    let Ok(entries) = fs::read_dir(&block) else {
        return DiskMap::default();
    };
    let mut stacked = Vec::new(); // dm-*/md* resolved after the real disks

    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        let dir = block.join(&id);
        if VIRTUAL.iter().any(|p| id.starts_with(p)) {
            if id.starts_with("dm-") || id.starts_with("md") {
                stacked.push(id);
            }
            continue;
        }
        let size = read_trim(&dir.join("size")).parse::<u64>().unwrap_or(0) * 512;
        if size == 0 {
            continue;
        }
        let model = [
            read_trim(&dir.join("device/vendor")),
            read_trim(&dir.join("device/model")),
        ]
        .into_iter()
        .filter(|s| !s.is_empty() && s != "ATA" && !s.starts_with("0x"))
        .collect::<Vec<_>>()
        .join(" ");
        let removable = read_trim(&dir.join("removable")) == "1";
        let rotational = read_trim(&dir.join("queue/rotational"));
        let real = fs::canonicalize(&dir)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let interconnect = if real.contains("/usb") {
            "USB"
        } else if id.starts_with("nvme") {
            "NVMe"
        } else if id.starts_with("mmcblk") {
            "SD/MMC"
        } else if real.contains("/virtio") || id.starts_with("vd") {
            "virtio"
        } else if real.contains("/ata") {
            "SATA"
        } else {
            ""
        };

        let mut partitioned = 0;
        if let Ok(subs) = fs::read_dir(&dir) {
            for sub in subs.flatten() {
                let part = sub.file_name().to_string_lossy().into_owned();
                if sub.path().join("partition").exists() {
                    partitioned += read_trim(&sub.path().join("size"))
                        .parse::<u64>()
                        .unwrap_or(0)
                        * 512;
                    names.insert(part, id.clone());
                }
            }
        }
        names.insert(id.clone(), id.clone());
        disks.push(Disk {
            id: id.clone(),
            model,
            size,
            partitioned: if partitioned > 0 { partitioned } else { size },
            internal: if removable || interconnect == "USB" {
                Some(false)
            } else {
                None
            },
            solid_state: match rotational.as_str() {
                "0" => Some(true),
                "1" => Some(false),
                _ => None,
            },
            interconnect: interconnect.into(),
            is_virtual: false,
        });
    }

    // device-mapper (LVM, LUKS) and md RAID: follow slaves to a real disk.
    for _ in 0..4 {
        for id in &stacked {
            if names.contains_key(id) {
                continue;
            }
            let slaves = block.join(id).join("slaves");
            if let Ok(mut it) = fs::read_dir(slaves) {
                if let Some(Ok(s)) = it.next() {
                    let slave = s.file_name().to_string_lossy().into_owned();
                    if let Some(root) = names.get(&slave).cloned() {
                        names.insert(id.clone(), root);
                    }
                }
            }
        }
    }
    finish(disks, names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort() {
        let mut v = vec!["disk10", "disk2", "disk0"];
        v.sort_by_key(|s| natural_key(s));
        assert_eq!(v, ["disk0", "disk2", "disk10"]);
    }

    #[test]
    fn unallocated_ignores_slack() {
        let d = Disk {
            size: 1 << 40,
            partitioned: (1 << 40) - (1 << 20),
            ..Default::default()
        };
        assert_eq!(d.unallocated(), 0);
        let d = Disk {
            size: 1 << 40,
            partitioned: 1 << 39,
            ..Default::default()
        };
        assert_eq!(d.unallocated(), 1 << 39);
    }

    #[test]
    fn caption() {
        let d = Disk {
            model: "APPLE SSD".into(),
            internal: Some(true),
            solid_state: Some(true),
            interconnect: "Apple Fabric".into(),
            ..Default::default()
        };
        assert_eq!(d.caption(), "APPLE SSD · internal · SSD · Apple Fabric");
    }

    #[test]
    fn snapshot_names_fall_back_to_volume() {
        let mut owner = HashMap::new();
        owner.insert("disk3s3".to_string(), 0);
        let m = DiskMap {
            disks: vec![Disk::default()],
            owner,
        };
        assert_eq!(m.disk_of("/dev/disk3s3s1"), Some(0));
        assert_eq!(m.disk_of("/dev/disk9"), None);
        assert_eq!(m.disk_of("map auto_home"), None);
    }

    #[test]
    fn sysfs_tree() {
        let root = std::env::temp_dir().join(format!("vdf-sysfs-{}", std::process::id()));
        let w = |p: &str, s: &str| {
            let path = root.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, s).unwrap();
        };
        // 1 TiB disk with a 512 GiB partition, LVM on top of it.
        w("block/sda/size", &((1u64 << 40) / 512).to_string());
        w("block/sda/removable", "0");
        w("block/sda/queue/rotational", "0");
        w("block/sda/device/vendor", "ATA");
        w("block/sda/device/model", "Fast SSD 1TB");
        w("block/sda/sda1/partition", "1");
        w("block/sda/sda1/size", &((1u64 << 39) / 512).to_string());
        w("block/dm-0/size", "100");
        w("block/dm-0/slaves/sda1", "");
        w("block/loop0/size", "100");

        let m = discover_sysfs(&root);
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(m.disks.len(), 1);
        let d = &m.disks[0];
        assert_eq!(d.model, "Fast SSD 1TB");
        assert_eq!(d.solid_state, Some(true));
        assert_eq!(d.unallocated(), 1 << 39);
        assert_eq!(m.owner.get("sda1"), Some(&0));
        assert_eq!(m.owner.get("dm-0"), Some(&0));
        assert!(!m.owner.contains_key("loop0"));
    }

    #[test]
    fn discovers_something() {
        // On a real machine there's at least one disk; inside some CI
        // containers there may be none, so just make sure it doesn't panic.
        let m = discover();
        for d in &m.disks {
            assert!(d.size > 0);
        }
    }
}
