set shell := ["zsh", "-cu"]

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

inspect:
    identify -format '%f %wx%h %[channels]\n' gallery/*.png

install:
    cargo install --path crates/vizir-cli --locked --force
