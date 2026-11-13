VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DIST    := dist
PREFIX  ?= /usr/local

.PHONY: all build test lint universal pkg dist install uninstall clean

all: build

build:
	cargo build --release

test:
	cargo test

lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

# Universal (arm64 + x86_64) macOS binary. Needs rustup with both targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
universal:
	MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --target aarch64-apple-darwin
	MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --target x86_64-apple-darwin
	mkdir -p target/universal
	lipo -create -output target/universal/vdf \
		target/aarch64-apple-darwin/release/vdf \
		target/x86_64-apple-darwin/release/vdf
	codesign --force --sign - target/universal/vdf

# macOS installer package from a binary (defaults to the native build).
BIN ?= target/release/vdf
pkg:
	packaging/macos/build-pkg.sh $(VERSION) $(BIN) $(DIST)

# Release tarball for the current platform.
dist: build
	packaging/make-tarball.sh $(VERSION) target/release/vdf $(DIST)

install: build
	install -d $(DESTDIR)$(PREFIX)/bin $(DESTDIR)$(PREFIX)/share/man/man1
	install -m 755 target/release/vdf $(DESTDIR)$(PREFIX)/bin/vdf
	install -m 644 man/vdf.1 $(DESTDIR)$(PREFIX)/share/man/man1/vdf.1

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/vdf $(DESTDIR)$(PREFIX)/share/man/man1/vdf.1

clean:
	cargo clean
	rm -rf $(DIST)
