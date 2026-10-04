#!/usr/bin/env python3
"""Deterministically regenerate separate OFL wrapping fixtures from local official originals.
Requires fonttools==4.61.1; no network, installation, or ordinary cargo-test dependency.
The accepted measured-text fixtures are never touched.
"""
import argparse
import hashlib
import json
from pathlib import Path
import fontTools
from fontTools import subset
from fontTools.ttLib import TTFont
import subset_test_fonts as accepted

TEXT = accepted.TEXT + "、。，：！？（）「」『』【】《》“”‘’\u00a0\u2009\u0308\u0327支持明确换行保留空白标点混合文本窄列布局第一第二行结束"
CODEPOINTS = sorted(set(accepted.UNICODES) | {ord(c) for c in TEXT})

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source_directory", type=Path)
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parents[1]/"crates/vizir-compiler/tests/fixtures/wrapping-fonts")
    args = parser.parse_args()
    if fontTools.__version__ != "4.61.1":
        raise SystemExit("Use exactly fonttools==4.61.1")
    original_manifest = json.loads((Path(__file__).resolve().parents[1]/"crates/vizir-compiler/tests/fixtures/fonts/manifest.json").read_text())
    original_hashes = {f["style"]: f["source_sha256"] for f in original_manifest["fonts"]}
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = {"source_commit":accepted.COMMIT,"fonttools":fontTools.__version__,"unicode_codepoints":CODEPOINTS,"fonts":[]}
    for style, git_hash in accepted.SOURCES.items():
        source = args.source_directory/f"NotoSansCJKsc-{style}.otf"
        data = source.read_bytes()
        if accepted.blob(data) != git_hash or hashlib.sha256(data).hexdigest() != original_hashes[style]:
            raise SystemExit(f"Pinned source identity mismatch: {source.name}")
        font = TTFont(source, recalcTimestamp=False)
        options = subset.Options()
        options.name_IDs = [0,1,2,3,4,5,6,13,14,16,17]
        options.name_legacy = True
        options.name_languages = [0x409]
        options.layout_features = ["*"]
        options.hinting = False
        options.desubroutinize = True
        sub = subset.Subsetter(options=options)
        sub.populate(unicodes=CODEPOINTS)
        sub.subset(font)
        family = "VizIR Wrapping Fixture SC"
        ps = f"VizIRWrappingFixtureSC-{style}"
        for name_id,value in {1:family,2:style,3:ps+"-1",4:family+" "+style,6:ps,16:family,17:style}.items():
            for record in font["name"].names:
                if record.nameID == name_id:
                    record.string = value.encode(record.getEncoding())
            font["name"].setName(value,name_id,3,1,0x409)
        top = font["CFF "].cff.topDictIndex[0]
        font["CFF "].cff.fontNames = [ps]
        top.FamilyName = family
        top.FullName = family+" "+style
        font["head"].created = font["head"].modified = 2082844800
        dest = args.output/f"VizIRWrappingFixtureSC-{style}.otf"
        font.save(dest,reorderTables=True)
        output = dest.read_bytes()
        manifest["fonts"].append({"style":style,"file":dest.name,"bytes":len(output),"sha256":hashlib.sha256(output).hexdigest(),"weight":font["OS/2"].usWeightClass,"face_index":0,"source_file":source.name,"source_git_blob":git_hash,"source_sha256":original_hashes[style],"source_url":f"https://raw.githubusercontent.com/notofonts/noto-cjk/{accepted.COMMIT}/Sans/OTF/SimplifiedChinese/{source.name}"})
    (args.output/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n",encoding="utf-8")
    print(json.dumps({"total_bytes":sum(f["bytes"] for f in manifest["fonts"]),"fonts":manifest["fonts"]},indent=2))

if __name__ == "__main__":
    main()
