use super::font_scale_from_css;

#[test]
fn a_larger_root_font_reads_as_a_scale_and_junk_reads_as_one() {
    assert_eq!(font_scale_from_css("16px"), 1.0);
    assert_eq!(font_scale_from_css("20px"), 1.25);
    assert_eq!(font_scale_from_css(" 24px "), 1.5);
    assert_eq!(font_scale_from_css(""), 1.0);
    assert_eq!(font_scale_from_css("large"), 1.0);
    assert_eq!(font_scale_from_css("0px"), 1.0);
}
