# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-11-13

### Added
- First release.
- Output grouped by disk. Each physical disk has an "entire disk" bar that shows
  used, free and unallocated space, with the volumes mounted from it listed in a
  tree underneath.
- `--disks` shows physical disks only. `--flat` shows the classic `df`-style
  volume list.
- Horizontal colour-gradient bars drawn to 1/8-character precision. Six themes:
  heat, ocean, sunset, forest, rainbow and mono.
- 24-bit colour with a 256-colour fallback. Honours `NO_COLOR`. `--ascii` mode
  for plain terminals.
- On macOS, APFS-aware numbers that match `df`: per-volume used space and
  volume names, with shared containers counted once in totals. Disk details
  come from IOKit.
- On Linux, disk details come from `/sys/block`, including partitions,
  device-mapper/LVM/LUKS and md.
- Inode view (`-i`), path filtering (`vdf ~`), type filters, sorting, totals
  and `--json` output.
- Distribution: a universal macOS binary, a macOS `.pkg` installer, static
  Linux binaries (x86_64, aarch64), a man page and a Homebrew tap.

[1.0.0]: https://github.com/jggonz/vdf/releases/tag/v1.0.0
