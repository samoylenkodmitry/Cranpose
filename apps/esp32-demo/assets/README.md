# Font

`NotoSans-Latin.ttf` (18 KB) is Noto Sans Regular cut to printable ASCII,
Latin-1, common punctuation and a few symbols, with kerning kept and hinting
dropped: the 1.3 MB fallback font of `cranpose-render-common` would take a
third of the firmware's flash. It is made from that crate's
`assets/NotoSansMerged.ttf` with fontTools 4.60.1:

```sh
pyftsubset crates/cranpose-render/common/assets/NotoSansMerged.ttf \
  --unicodes="U+0020-007E,U+00A0-00FF,U+2013-2014,U+2018-201E,U+2022,U+2026,U+20AC,U+2122" \
  --layout-features='kern' --no-hinting --desubroutinize \
  --output-file=apps/esp32-demo/assets/NotoSans-Latin.ttf
```

Noto Sans is licensed under the SIL Open Font License 1.1: `NotoSans-OFL.txt`.
