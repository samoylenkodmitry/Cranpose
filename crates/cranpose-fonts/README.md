# Cranpose font packs

This crate provides optional variable font data for applications that need
Arabic, Devanagari, or Simplified Chinese glyph coverage. Enable only the
matching feature on `cranpose`: `localized-arabic`, `localized-devanagari`, or
`localized-cjk`. Each pack keeps the original character map and registers the
400, 500, 600, 700, and 800 weight instances from one embedded font file.

The Arabic and Devanagari fonts are Noto Sans builds from the Google Fonts
repository. The CJK font is Noto Sans CJK SC from the Noto CJK repository.
Each font's license is included with its data. Font data is included only when
its feature is enabled. The CJK font is split into three published data crates
to fit the crates.io compressed package-size limit; the `cjk` build feature
joins the original contiguous bytes into one build artifact, with no runtime
decoding or copying.
