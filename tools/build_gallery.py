#!/usr/bin/env python3
"""Build a deterministic, self-contained example gallery from local artifacts."""

from __future__ import annotations

import argparse
import base64
import html
import json
import mimetypes
import subprocess
import sys
from pathlib import Path
from typing import Any, NoReturn


def fail(message: str) -> NoReturn:
    raise SystemExit(message)


def humanize(value: str) -> str:
    words = value.replace("_", "-").split("-")
    if words and words[0].isdigit():
        words = words[1:]
    return " ".join(words).strip().capitalize() or value


def resolve_path(root: Path, value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else root / path


def relative_link(root: Path, output: Path, value: str) -> str:
    path = resolve_path(root, value)
    try:
        return path.relative_to(output.parent).as_posix()
    except ValueError:
        return Path(value).as_posix()


def data_uri(path: Path) -> str:
    mime = mimetypes.guess_type(path.name)[0]
    if path.suffix.lower() == ".svg":
        mime = "image/svg+xml"
    if not mime or not mime.startswith("image/"):
        fail(f"preview is not a supported image: {path}")
    encoded = base64.b64encode(path.read_bytes()).decode("ascii")
    return f"data:{mime};base64,{encoded}"


def expand(template: str, values: dict[str, str]) -> str:
    try:
        return template.format_map(values)
    except KeyError as error:
        fail(f"unknown gallery template field {error.args[0]!r} in {template!r}")


def normalize_item(root: Path, raw: dict[str, Any]) -> dict[str, Any]:
    required = ("id", "title", "category", "source", "preview", "artifact")
    missing = [key for key in required if not raw.get(key)]
    if missing:
        fail(f"gallery item is missing {', '.join(missing)}: {raw}")
    item = dict(raw)
    item["id"] = str(item["id"])
    item["title"] = str(item["title"])
    item["category"] = str(item["category"])
    item["summary"] = str(item.get("summary", "Editable source with a real rendered artifact."))
    item["tags"] = [str(tag) for tag in item.get("tags", [])]
    for key in ("source", "preview", "artifact"):
        item[key] = str(item[key])
        path = resolve_path(root, item[key])
        if not path.exists():
            fail(f"gallery item {item['id']!r} has missing {key}: {path}")
    return item


def discover_items(root: Path, config: dict[str, Any]) -> list[dict[str, Any]]:
    discovery = config.get("discovery", {"mode": "items"})
    mode = discovery.get("mode", "items")
    if mode == "items":
        return [normalize_item(root, item) for item in config.get("items", [])]
    if mode == "glob":
        return discover_glob(root, discovery)
    if mode == "command":
        return discover_command(root, discovery)
    if mode == "manifests":
        return discover_manifests(root, discovery)
    fail(f"unsupported gallery discovery mode: {mode}")


def discover_glob(root: Path, discovery: dict[str, Any]) -> list[dict[str, Any]]:
    base = Path(discovery.get("base", "."))
    sources: set[Path] = set()
    for pattern in discovery.get("patterns", []):
        sources.update(path for path in root.glob(pattern) if path.is_file())
    items: list[dict[str, Any]] = []
    for source in sorted(sources):
        relative = source.relative_to(root / base)
        suffix = discovery.get("remove_suffix")
        relative_text = relative.as_posix()
        if suffix and relative_text.endswith(suffix):
            relative_no_suffix = relative_text[: -len(suffix)]
        else:
            relative_no_suffix = relative.with_suffix("").as_posix()
        stem = Path(relative_no_suffix).name
        parent = Path(relative_no_suffix).parent.as_posix()
        category = discovery.get("category_by_suffix", {}).get(source.suffix)
        if not category:
            category = parent.split("/", 1)[0] if parent != "." else discovery.get("default_category", "overview")
        values = {
            "id": relative_no_suffix.replace("/", "--"),
            "stem": stem,
            "snake": stem.replace("-", "_"),
            "relative": relative_no_suffix,
            "category": category,
        }
        items.append(
            normalize_item(
                root,
                {
                    "id": values["id"],
                    "title": humanize(stem),
                    "category": category,
                    "source": source.relative_to(root).as_posix(),
                    "preview": expand(discovery["preview"], values),
                    "artifact": expand(discovery.get("artifact", discovery["preview"]), values),
                    "summary": expand(discovery.get("summary", "Rendered {category} example."), values),
                    "tags": discovery.get("tags", []),
                },
            )
        )
    return items


def discover_command(root: Path, discovery: dict[str, Any]) -> list[dict[str, Any]]:
    command = [str(part) for part in discovery.get("command", [])]
    if not command:
        fail("command discovery requires a command")
    result = subprocess.run(command, cwd=root, check=True, capture_output=True, text=True)
    items: list[dict[str, Any]] = []
    for line in result.stdout.splitlines():
        columns = line.split(maxsplit=2)
        if len(columns) != 3:
            continue
        item_id, category, title = columns
        values = {
            "id": item_id,
            "stem": item_id,
            "snake": item_id.replace("-", "_"),
            "relative": item_id,
            "category": category,
        }
        source = discovery.get("default_source", ".")
        for candidate in discovery.get("source_candidates", []):
            expanded = expand(candidate, values)
            if resolve_path(root, expanded).exists():
                source = expanded
                break
        items.append(
            normalize_item(
                root,
                {
                    "id": item_id,
                    "title": title,
                    "category": category,
                    "source": source,
                    "preview": expand(discovery["preview"], values),
                    "artifact": expand(discovery.get("artifact", discovery["preview"]), values),
                    "summary": expand(discovery.get("summary", "Rendered {category} example."), values),
                    "tags": [category],
                },
            )
        )
    return items


def discover_manifests(root: Path, discovery: dict[str, Any]) -> list[dict[str, Any]]:
    items: list[dict[str, Any]] = []
    seen: set[str] = set()
    for path in sorted(root.glob(discovery["pattern"])):
        if not path.is_file():
            continue
        try:
            record = json.loads(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            continue
        item_id = record.get(discovery.get("id_key", "id"))
        if not item_id or item_id in seen:
            continue
        seen.add(str(item_id))
        artifact_value = record.get(discovery.get("preview_key", "path"))
        if not artifact_value:
            continue
        values = {
            "id": str(item_id),
            "stem": str(item_id),
            "snake": str(item_id).replace("-", "_"),
            "relative": str(item_id),
            "category": str(record.get(discovery.get("category_key", "category"), "examples")),
        }
        preview_value = expand(discovery["preview"], values) if discovery.get("preview") else str(artifact_value)
        preview_path = resolve_path(root, preview_value)
        if not preview_path.exists():
            continue
        category = str(record.get(discovery.get("category_key", "category"), "examples"))
        tags = record.get(discovery.get("tags_key", "tags"), [])
        items.append(
            normalize_item(
                root,
                {
                    "id": str(item_id),
                    "title": str(record.get(discovery.get("title_key", "title"), humanize(str(item_id)))),
                    "category": category,
                    "source": discovery.get("default_source", "."),
                    "preview": str(preview_path),
                    "artifact": str(artifact_value),
                    "summary": str(record.get(discovery.get("summary_key", "description"), "Rendered example.")),
                    "tags": tags if isinstance(tags, list) else [str(tags)],
                },
            )
        )
    return items


def build_html(root: Path, config: dict[str, Any], items: list[dict[str, Any]]) -> str:
    output = root / config.get("output", "gallery.html")
    categories = sorted({item["category"] for item in items}, key=str.casefold)
    category_options = "".join(
        f'<option value="{html.escape(category, quote=True)}">{html.escape(category)}</option>'
        for category in categories
    )
    cards: list[str] = []
    for item in items:
        source_link = relative_link(root, output, item["source"])
        artifact_link = relative_link(root, output, item["artifact"])
        preview = data_uri(resolve_path(root, item["preview"]))
        tags = " ".join(item["tags"])
        search = " ".join((item["title"], item["category"], item["summary"], tags)).casefold()
        tag_html = "".join(f"<span>{html.escape(tag)}</span>" for tag in item["tags"])
        cards.append(
            f'''<article class="card" data-category="{html.escape(item['category'], quote=True)}" data-search="{html.escape(search, quote=True)}">
  <a class="preview" href="{html.escape(artifact_link, quote=True)}" aria-label="Open artifact for {html.escape(item['title'], quote=True)}">
    <img loading="lazy" decoding="async" src="{preview}" alt="{html.escape(item['title'], quote=True)} rendered example">
  </a>
  <div class="card-body">
    <div class="meta"><span class="category">{html.escape(item['category'])}</span>{tag_html}</div>
    <h2>{html.escape(item['title'])}</h2>
    <p>{html.escape(item['summary'])}</p>
    <nav aria-label="Files for {html.escape(item['title'], quote=True)}"><a href="{html.escape(source_link, quote=True)}">Source</a><a href="{html.escape(artifact_link, quote=True)}">Artifact</a></nav>
  </div>
</article>'''
        )
    title = str(config.get("title", config.get("project", "Example gallery")))
    description = str(config.get("description", "Executable examples with editable source and rendered proof."))
    project = str(config.get("project", title))
    item_label = str(config.get("item_label", "examples"))
    return f'''<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="color-scheme" content="light">
  <title>{html.escape(title)}</title>
  <style>
    :root{{--paper:#f4f1ea;--surface:#fffdfa;--ink:#18212b;--muted:#5e6b78;--line:#d8d4cb;--accent:#0b7768;--accent-soft:#dff3ee;--shadow:0 18px 45px rgba(24,33,43,.09)}}
    *{{box-sizing:border-box}} body{{margin:0;background:var(--paper);color:var(--ink);font:16px/1.55 ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}}
    a{{color:inherit}} .shell{{width:min(1500px,calc(100% - 40px));margin:auto}} header{{padding:64px 0 28px}} .eyebrow{{color:var(--accent);font-size:.76rem;font-weight:800;letter-spacing:.14em;text-transform:uppercase}}
    h1{{max-width:900px;margin:.2em 0 .22em;font-size:clamp(2.5rem,7vw,6.6rem);line-height:.94;letter-spacing:-.055em}} .lede{{max-width:760px;margin:0;color:var(--muted);font-size:clamp(1rem,2vw,1.2rem)}}
    .summary{{display:flex;gap:10px;flex-wrap:wrap;margin-top:24px}} .summary span,.meta span{{border:1px solid var(--line);border-radius:999px;background:var(--surface);padding:5px 10px;font-size:.78rem}}
    .controls{{position:sticky;top:0;z-index:5;display:grid;grid-template-columns:minmax(180px,1fr) minmax(150px,240px) auto;gap:12px;padding:14px 0 18px;background:linear-gradient(var(--paper) 78%,transparent)}}
    label{{position:absolute;inline-size:1px;block-size:1px;overflow:hidden;clip-path:inset(50%)}} input,select{{width:100%;border:1px solid var(--line);border-radius:12px;background:var(--surface);color:var(--ink);padding:12px 14px;font:inherit;box-shadow:0 4px 16px rgba(24,33,43,.04)}}
    input:focus-visible,select:focus-visible,a:focus-visible{{outline:3px solid #5dc9b6;outline-offset:3px}} #visible-count{{align-self:center;color:var(--muted);font-variant-numeric:tabular-nums;white-space:nowrap}}
    main{{display:grid;grid-template-columns:repeat(auto-fill,minmax(290px,1fr));gap:20px;padding:8px 0 80px}} .card{{min-width:0;overflow:hidden;border:1px solid var(--line);border-radius:18px;background:var(--surface);box-shadow:var(--shadow)}}
    .preview{{display:flex;aspect-ratio:16/10;align-items:center;justify-content:center;border-bottom:1px solid var(--line);background:linear-gradient(145deg,#edf4f1,#ece7dd);padding:10px}} .preview img{{display:block;width:100%;height:100%;object-fit:contain}}
    .card-body{{padding:18px}} .meta{{display:flex;gap:6px;flex-wrap:wrap}} .meta .category{{border-color:#9dd3c9;background:var(--accent-soft);color:#075b50;font-weight:750}} h2{{margin:.72rem 0 .35rem;font-size:1.24rem;line-height:1.2;letter-spacing:-.018em}} .card p{{min-height:3em;margin:0;color:var(--muted);font-size:.92rem}}
    .card nav{{display:flex;gap:8px;margin-top:16px}} .card nav a{{border-radius:9px;background:var(--ink);color:white;padding:7px 11px;text-decoration:none;font-size:.82rem;font-weight:700}} .card nav a+ a{{background:transparent;color:var(--ink);box-shadow:inset 0 0 0 1px var(--line)}}
    .card[hidden]{{display:none}} footer{{border-top:1px solid var(--line);padding:24px 0 42px;color:var(--muted);font-size:.86rem}}
    @media(max-width:700px){{.shell{{width:min(100% - 24px,1500px)}}header{{padding-top:42px}}.controls{{grid-template-columns:1fr 1fr}}#visible-count{{grid-column:1/-1}}main{{grid-template-columns:1fr}}}}
    @media print{{body{{background:white}}header{{padding-top:20px}}.controls{{display:none}}main{{grid-template-columns:repeat(2,1fr);gap:10px}}.card{{break-inside:avoid;box-shadow:none}}.preview{{aspect-ratio:16/9}}.card nav{{display:none}}footer{{padding-bottom:0}}}}
  </style>
</head>
<body>
  <header class="shell">
    <div class="eyebrow">{html.escape(project)} / executable gallery</div>
    <h1>{html.escape(title)}</h1>
    <p class="lede">{html.escape(description)}</p>
    <div class="summary"><span>{len(items)} {html.escape(item_label)}</span><span>{len(categories)} categories</span><span>embedded previews</span><span>offline-ready</span></div>
  </header>
  <section class="controls shell" aria-label="Gallery filters">
    <label for="search">Search examples</label><input id="search" type="search" placeholder="Search title, category, or tag…" autocomplete="off">
    <label for="category">Filter category</label><select id="category"><option value="">All categories</option>{category_options}</select>
    <output id="visible-count" aria-live="polite">{len(items)} shown</output>
  </section>
  <main class="shell" id="gallery">{''.join(cards)}</main>
  <footer><div class="shell">Generated from local sources and real render artifacts. Example data may be illustrative; follow each Source link for provenance and editable input.</div></footer>
  <script>
    const search=document.querySelector('#search'), category=document.querySelector('#category'), cards=[...document.querySelectorAll('.card')], count=document.querySelector('#visible-count');
    function filter(){{const q=search.value.trim().toLocaleLowerCase(), c=category.value;let shown=0;for(const card of cards){{const visible=(!q||card.dataset.search.includes(q))&&(!c||card.dataset.category===c);card.hidden=!visible;if(visible)shown++}}count.value=`${{shown}} shown`}}
    search.addEventListener('input',filter);category.addEventListener('change',filter);
  </script>
</body>
</html>
'''


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", default="gallery.config.json", type=Path)
    parser.add_argument("--check", action="store_true", help="fail unless gallery.html is current")
    args = parser.parse_args()
    config_path = args.config.resolve()
    root = config_path.parent
    config = json.loads(config_path.read_text(encoding="utf-8"))
    if config.get("schema_version") != 1:
        fail("gallery config must use schema_version 1")
    items = discover_items(root, config)
    if not items:
        fail("gallery discovery produced no items")
    ids = [item["id"] for item in items]
    if len(ids) != len(set(ids)):
        fail("gallery item ids must be unique")
    rendered = build_html(root, config, items)
    output = root / config.get("output", "gallery.html")
    if args.check:
        if not output.exists() or output.read_text(encoding="utf-8") != rendered:
            fail(f"stale gallery: run {Path(sys.argv[0]).as_posix()} --config {args.config}")
        print(f"verified {output}: {len(items)} examples")
        return
    output.write_text(rendered, encoding="utf-8")
    print(f"wrote {output}: {len(items)} examples")


if __name__ == "__main__":
    main()
