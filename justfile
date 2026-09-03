set shell := ["zsh", "-cu"]

os_name := if os() == "macos" { "macos" } else { "linux" }
arch_name := if arch() == "aarch64" { "arm64" } else { "x86" }
default_install_bin := home_directory() / "sync" / (os_name + "-" + arch_name + "-bin")
install_bin := env("SYNC_BIN_DIR", default_install_bin)
target_dir := env("CARGO_TARGET_DIR", justfile_directory() / "target")

build:
    cargo build --release -p vizir-cli

test:
    cargo test --workspace

check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

schemas:
    cargo run -q -p vizir-cli -- schema mir --output schemas/viz-mir.schema.json
    cargo run -q -p vizir-cli -- schema scene-patch --output schemas/scene-patch.schema.json
    cargo run -q -p vizir-cli -- schema capability --output schemas/capability.schema.json

gallery:
    mkdir -p gallery
    for file in examples/*/*.viz.yaml; do name="$(basename "$file" .viz.yaml)"; cargo run -q -p vizir-cli -- render "$file" --format png --background transparent --output "gallery/${name}.png"; done
    python3 tools/build_gallery.py

gallery-check: gallery
    python3 tools/build_gallery.py --check

inspect:
    identify -format '%f %wx%h %[channels]\n' gallery/*.png

install: build
    mkdir -p "{{ install_bin }}"
    cp "{{ target_dir }}/release/vizir" "{{ install_bin }}/vizir"
    chmod +x "{{ install_bin }}/vizir"
    @echo "installed {{ install_bin }}/vizir"
