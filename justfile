set shell := ["zsh", "-cu"]

check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

gallery:
    mkdir -p gallery
    for file in examples/*/*.viz.yaml; do name="$(basename "$file" .viz.yaml)"; cargo run -q -p vizir-cli -- render "$file" --format png --background transparent --output "gallery/${name}.png"; done

inspect:
    identify -format '%f %wx%h %[channels]\n' gallery/*.png

install:
    cargo install --path crates/vizir-cli --locked --force
