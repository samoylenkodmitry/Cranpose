use super::*;

#[test]
fn a_click_cycles_the_mood_and_the_fourth_comes_back_around() {
    assert_eq!(Mood::after_clicks(0), Mood::Prompt);
    assert_eq!(Mood::after_clicks(1), Mood::Happy);
    assert_eq!(Mood::after_clicks(2), Mood::Sleepy);
    assert_eq!(Mood::after_clicks(3), Mood::Prompt);
}

#[test]
fn a_click_changes_the_face_and_the_fire_it_wears() {
    for clicks in 0..24u32 {
        let before = (Mood::after_clicks(clicks), style_after_clicks(clicks).label);
        let after = (
            Mood::after_clicks(clicks + 1),
            style_after_clicks(clicks + 1).label,
        );
        assert_ne!(before, after, "a click has to change something at {clicks}");
    }
    assert_ne!(
        style_after_clicks(0).label,
        style_after_clicks(1).label,
        "the fire walks with the face, not behind it"
    );
}

#[test]
fn every_mood_has_its_own_caption_and_shader_code() {
    let moods = [Mood::Prompt, Mood::Happy, Mood::Sleepy];
    for (index, mood) in moods.iter().enumerate() {
        assert_eq!(mood.shader_code(), index as f32);
        assert!(
            moods
                .iter()
                .filter(|other| other.caption() == mood.caption())
                .count()
                == 1
        );
    }
}
