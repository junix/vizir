# Separate wrapping fixture fonts

These renamed static Regular400, Medium500 and Bold700 OFL derivatives extend the
accepted measured-text fixture coverage with CJK opening/closing punctuation,
full-width punctuation, NBSP/thin spaces and a few additional example characters.
The accepted `../fonts/` bytes are unchanged. These are deliberately incomplete
test fonts, not general-purpose font packs; absent glyph coverage must fail.

`manifest.json` records the exact official Noto CJK source commit, paths, Git blob
hashes, SHA256 hashes, selected face0, derivative names, weights and byte sizes.
Each derivative retains its source copyright and OFL metadata. See `OFL.txt`.
No installation or machine-font lookup is involved.

Regenerate from the three locally supplied full official originals with:

```
python tools/subset_wrapping_fonts.py /path/to/pinned/originals
```

Regeneration requires exactly fonttools4.61.1, verifies both original Git and
SHA256 hashes, sorts codepoints, fixes source timestamps and renames family and
PostScript identities to VizIR Wrapping Fixture SC. Full originals are not
redistributed. Ordinary cargo tests use checked-in derivative bytes and require
neither Python nor network access.
