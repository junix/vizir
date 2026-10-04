#!/usr/bin/env python3
"""Regenerate tiny renamed OFL font fixtures from locally supplied pinned originals.
Requires fonttools==4.61.1. No network. Ordinary cargo tests do not run this script.
"""
from pathlib import Path
import argparse, hashlib, json
import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.pens.t2CharStringPen import T2CharStringPen

COMMIT = "165c01b46ea533872e002e0785ff17e44f6d97d8"
SOURCES = {
    "Regular": "dc15562470b4f842321894787a0d066879ccff8b",
    "Medium": "00b01dff38ff8d0a345c53d64a71bfcf9ec55522",
    "Bold": "ff4c0450e8a5bf0290fbb6013a72dc61a10e8e56",
}
TEXT = "éÅe\u0301A\u030ax\u0301中文字体测量温度轴标题图服务北京上海深圳广州测试颜色组常规中等粗体延迟请求均值甲乙丙秒城市容量可靠性示例"
UNICODES = sorted(set(range(32, 127)) | {ord(c) for c in TEXT})

def blob(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()

def main():
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("source_directory", type=Path)
    args.add_argument("--output", type=Path, default=Path(__file__).resolve().parents[1] / "crates/vizir-compiler/tests/fixtures/fonts")
    args = args.parse_args()
    if fontTools.__version__ != "4.61.1":
        raise SystemExit("Use exactly fonttools==4.61.1")
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = {"source_commit": COMMIT, "fonttools": fontTools.__version__, "unicode_codepoints": UNICODES, "fonts": []}
    for style, expected_blob in SOURCES.items():
        source = args.source_directory / f"NotoSansCJKsc-{style}.otf"
        data = source.read_bytes()
        if blob(data) != expected_blob:
            raise SystemExit(f"Pinned Git blob mismatch: {source.name}")
        font = TTFont(source, recalcTimestamp=False)
        options = subset.Options()
        options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14, 16, 17]
        options.name_legacy = True
        options.name_languages = [0x409]
        options.layout_features = ["*"]
        options.hinting = False
        options.desubroutinize = True
        subsetter = subset.Subsetter(options=options)
        subsetter.populate(unicodes=UNICODES)
        subsetter.subset(font)
        family = "VizIR Fixture SC"
        ps = f"VizIRFixtureSC-{style}"
        renames = {1:family, 2:style, 3:f"{ps}-1", 4:f"{family} {style}", 6:ps, 16:family, 17:style}
        names = font["name"]
        for record in names.names:
            if record.nameID in renames:
                record.string = renames[record.nameID].encode(record.getEncoding())
        for name_id, value in renames.items():
            names.setName(value, name_id, 3, 1, 0x409)
        if "CFF " in font:
            cff = font["CFF "].cff
            cff.fontNames = [ps]
            top = cff.topDictIndex[0]
            top.FamilyName = family
            top.FullName = f"{family} {style}"
        font["head"].created = font["head"].modified = 2082844800
        dest = args.output / f"VizIRFixtureSC-{style}.otf"
        font.save(dest, reorderTables=True)
        result = dest.read_bytes()
        manifest["fonts"].append({"style":style, "file":dest.name, "bytes":len(result), "sha256":hashlib.sha256(result).hexdigest(), "weight":font["OS/2"].usWeightClass, "face_index":0, "source_file":source.name, "source_git_blob":expected_blob, "source_sha256":hashlib.sha256(data).hexdigest(), "source_url":f"https://raw.githubusercontent.com/notofonts/noto-cjk/{COMMIT}/Sans/OTF/SimplifiedChinese/{source.name}"})
    # Valid fractional-font-unit contour that the pinned CFF extraction cannot
    # preserve. This tiny A/B-only font makes the fail-closed regression offline.
    source_fixture = args.output / "VizIRFixtureSC-Regular.otf"
    font = TTFont(source_fixture, recalcTimestamp=False)
    options = subset.Options()
    options.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14, 16, 17]
    options.name_languages = [0x409]
    options.name_legacy = True
    options.hinting = False
    options.desubroutinize = True
    sub = subset.Subsetter(options=options)
    sub.populate(unicodes=[32, 65, 66])
    sub.subset(font)
    top = font["CFF "].cff.topDictIndex[0]
    glyph = font.getBestCmap()[65]
    old = top.CharStrings[glyph]
    pen = T2CharStringPen(None, None, roundTolerance=0)
    pen.moveTo((100, 0))
    pen.lineTo((100.1, 0))
    pen.lineTo((500.1, 400))
    pen.lineTo((500, 400))
    pen.closePath()
    top.CharStrings[glyph] = pen.getCharString(private=old.private, globalSubrs=old.globalSubrs)
    family = "VizIR Precision Fixture"
    ps = "VizIRPrecisionFixture-Regular"
    for name_id, value in {1:family, 2:"Regular", 3:ps+"-1", 4:family+" Regular", 6:ps, 16:family, 17:"Regular"}.items():
        for record in font["name"].names:
            if record.nameID == name_id:
                record.string = value.encode(record.getEncoding())
        font["name"].setName(value, name_id, 3, 1, 0x409)
    font["CFF "].cff.fontNames = [ps]
    top.FamilyName = family
    top.FullName = family+" Regular"
    font["head"].created = font["head"].modified = 2082844800
    dest = args.output / "VizIRPrecisionFixture-Regular.otf"
    font.save(dest, reorderTables=True)
    data = dest.read_bytes()
    manifest["precision_fixture"] = {"file":dest.name,"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest(),"source_fixture_sha256":manifest["fonts"][0]["sha256"],"weight":400,"face_index":0,"glyph_A_contour":[[100,0],[100.1,0],[500.1,400],[500,400]],"purpose":"reject extraction/serialization ink collapse for A and AB; space remains valid"}
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2)+"\n", encoding="utf-8")
    print(json.dumps({"total_bytes":sum(x["bytes"] for x in manifest["fonts"]), "fonts":manifest["fonts"]}, indent=2))

if __name__ == "__main__":
    main()
