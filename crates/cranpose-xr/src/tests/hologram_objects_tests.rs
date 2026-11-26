use super::*;

const FORMS: [ChoiceForm; 4] = [
    ChoiceForm::Cubes,
    ChoiceForm::Gems,
    ChoiceForm::Petals,
    ChoiceForm::Stepper,
];

// Cells of a row of 568 dp shared by 3 to 5 options, a stepper's middle, and rows from 84 to 130
// dp high
fn cells() -> impl Iterator<Item = [f32; 2]> {
    [568.0 / 5.0, 568.0 / 4.0, 568.0 / 3.0, 405.0]
        .into_iter()
        .flat_map(|width| [84.0, 100.0, 112.0, 130.0].map(|height| [width, height]))
}

#[test]
fn every_object_in_full_bloom_stays_in_its_cell_above_the_words() {
    for form in FORMS {
        let reach = Reach::of(form);
        for [width, height] in cells() {
            let fit = fit(reach, [width, height]);
            // The cell spans y from -height / 2 to height / 2, and the words take its bottom
            let top = fit.lift - reach.up * fit.scale;
            let bottom = fit.lift + reach.down * fit.scale;
            let words = height / 2.0 - WORDS_UNDER;
            assert!(
                top >= -height / 2.0 + 1.0,
                "{form:?} in {width}x{height}: top {top}"
            );
            assert!(
                bottom <= words - 1.0,
                "{form:?} in {width}x{height}: bottom {bottom} over the words at {words}"
            );
            assert!(
                reach.half * fit.scale <= width / 2.0 - 1.0,
                "{form:?} in {width}x{height}: half width {}",
                reach.half * fit.scale
            );
        }
    }
}

#[test]
fn the_largest_object_fills_most_of_the_room_one_way() {
    for form in FORMS {
        let reach = Reach::of(form);
        for [width, height] in cells() {
            let fit = fit(reach, [width, height]);
            let room = height - WORDS_UNDER;
            let across = 2.0 * reach.half * fit.scale / width;
            let down = (reach.up + reach.down) * fit.scale / room;
            assert!(
                across.max(down) > 0.8,
                "{form:?} in {width}x{height} takes {across:.2} across and {down:.2} down"
            );
        }
    }
}
