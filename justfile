set shell := ["zsh", "-cu"]

os_name := if os() == "macos" { "macos" } else { "linux" }
arch_name := if arch() == "aarch64" { "arm64" } else { "x86" }
default_install_bin := home_directory() / "sync" / (os_name + "-" + arch_name + "-bin")
install_bin := env("SYNC_BIN_DIR", default_install_bin)
target_dir := env("CARGO_TARGET_DIR", justfile_directory() / "target")
stamp := `git rev-parse --short HEAD` + `(git diff --quiet && git diff --cached --quiet) >/dev/null 2>&1 || printf .dirty`

build:
    PM_BUILD_SHA="g{{stamp}}" cargo build --release -p vizir-cli

test:
    cargo test --workspace

check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

schemas:
    cargo run -q -p vizir-cli -- schema composition --output schemas/composition.schema.json
    cargo run -q -p vizir-cli -- schema mir --output schemas/viz-mir.schema.json
    cargo run -q -p vizir-cli -- schema themed-mir --output schemas/themed-mir.schema.json
    cargo run -q -p vizir-cli -- schema compiled-mir --output schemas/compiled-mir.schema.json
    cargo run -q -p vizir-cli -- schema scene-patch --output schemas/scene-patch.schema.json
    cargo run -q -p vizir-cli -- schema capability --output schemas/capability.schema.json

gallery:
    mkdir -p gallery
    for file in examples/*/*.viz.yaml; do name="$(basename "$file" .viz.yaml)"; cargo run -q -p vizir-cli -- render "$file" --format png --background transparent --output "gallery/${name}.png"; done
    python3 tools/build_gallery.py

gallery-check:
    python3 tools/build_gallery.py --check

inspect:
    identify -format '%f %wx%h %[channels]\n' gallery/*.png

install: build
    mkdir -p "{{ install_bin }}"
    @set -eu; dest="{{ install_bin }}/vizir"; mkdir -p "$(dirname "$dest")"; tmp="$(mktemp "{{ install_bin }}/.vizir.XXXXXX")"; trap 'rm -f "$tmp"' EXIT; cp "{{ target_dir }}/release/vizir" "$tmp"; chmod 755 "$tmp"; if [ "$(uname -s)" = "Darwin" ]; then xattr -c "$tmp" 2>/dev/null || true; codesign --force --sign - "$tmp"; fi; mv -f "$tmp" "$dest"
    @echo "installed {{ install_bin }}/vizir"

# Composition is an independent source contract; emit HIR before rendering it.
composition-demo:
    mkdir -p "{{ target_dir }}/composition-demo"
    cargo run -q -p vizir-cli -- compose examples/composition/service-grid.compose.yaml --output "{{ target_dir }}/composition-demo/service-grid.viz.json"
    cargo run -q -p vizir-cli -- validate "{{ target_dir }}/composition-demo/service-grid.viz.json"
    cargo run -q -p vizir-cli -- render "{{ target_dir }}/composition-demo/service-grid.viz.json" --format svg --output "{{ target_dir }}/composition-demo/service-grid.svg"
    cargo run -q -p vizir-cli -- render "{{ target_dir }}/composition-demo/service-grid.viz.json" --format png --background transparent --output "{{ target_dir }}/composition-demo/service-grid.png"
