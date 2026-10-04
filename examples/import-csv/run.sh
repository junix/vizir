#!/usr/bin/env bash
# From any directory: VIZIR=/path/to/vizir examples/import-csv/run.sh [OUTPUT_DIR]
# Requires Python 3 and a native PNG rasterizer supported by vizir render.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VIZIR="${VIZIR:-vizir}"
OUT="${1:-$ROOT/out/import-csv}"
mkdir -p "$OUT"
EXAMPLE="$ROOT/examples/import-csv"

"$VIZIR" import-csv "$EXAMPLE/area.csv" \
  --template "$EXAMPLE/area.template.yaml" --template-kind hir \
  --dataset signals --spec "$EXAMPLE/area.spec.json" \
  --output "$OUT/area.viz.json" --provenance "$OUT/area.provenance.json"
"$VIZIR" import-csv "$EXAMPLE/checks.csv" \
  --template "$EXAMPLE/dashboard.template.yaml" --template-kind composition \
  --dataset checks --spec "$EXAMPLE/checks.spec.json" \
  --output "$OUT/dashboard.compose.json" --provenance "$OUT/dashboard.provenance.json"
"$VIZIR" compose "$OUT/dashboard.compose.json" --output "$OUT/dashboard.viz.json"

FONTS=()
while IFS= read -r font; do FONTS+=(--font "$font"); done < <(
  python3 - "$ROOT" <<'PY'
import json
import pathlib
import sys
root = pathlib.Path(sys.argv[1])
fixtures = root / "crates/vizir-compiler/tests/fixtures/wrapping-fonts"
for font in json.loads((fixtures / "manifest.json").read_text())["fonts"]:
    print(f'{font["sha256"]}={fixtures / font["file"]}')
PY
)
for name in area dashboard; do
  HIR="$OUT/$name.viz.json"
  "$VIZIR" validate "$HIR"
  "$VIZIR" normalize "$HIR" --theme azure --output "$OUT/$name.mir.json"
  "$VIZIR" lower "$HIR" --theme azure --output "$OUT/$name.direct.scene.json"
  "$VIZIR" lower "$OUT/$name.mir.json" --output "$OUT/$name.replay.scene.json"
  cmp "$OUT/$name.direct.scene.json" "$OUT/$name.replay.scene.json"
  for theme in azure azure-dark; do
    STEM="$OUT/$name.$theme"
    "$VIZIR" normalize "$HIR" --theme "$theme" \
      --text-profile "$ROOT/examples/text/wrapping-font-profile.json" \
      --text-layout "$EXAMPLE/$name-title-layout.json" "${FONTS[@]}" \
      --output "$STEM.compiled.json"
    "$VIZIR" lower "$STEM.compiled.json" "${FONTS[@]}" --output "$STEM.scene.json"
    "$VIZIR" render "$STEM.compiled.json" "${FONTS[@]}" \
      --format svg --output "$STEM.svg" --manifest "$STEM.svg.manifest.json"
    "$VIZIR" render "$STEM.compiled.json" "${FONTS[@]}" \
      --format png --background transparent --output "$STEM.png" \
      --manifest "$STEM.png.manifest.json"
  done
done
"$VIZIR" explain "$OUT/area.mir.json" --node signals/area/Alpha > "$OUT/area.explain.txt"
"$VIZIR" explain "$OUT/dashboard.mir.json" --node coverage/cell/jobs-mon > "$OUT/dashboard.explain.txt"
printf 'CSV examples emitted and replay checked: %s\n' "$OUT"
