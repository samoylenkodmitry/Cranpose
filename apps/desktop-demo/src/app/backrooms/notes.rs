// What the entity says: on the walls, through the television and in the
// camcorder's captions. Some of it counts what the eye has done, read from
// the journey, so a composable that shows it follows the count.

use super::{journey::Journey, tab::FRAMES_PER_SECOND};

// The words the entity wrote on a wall: one of `plan::WORDS`. The first
// greet the eye in the room it starts in.
pub(super) fn wall(words: usize, journey: &Journey) -> String {
    let rooms = journey.rooms.get();
    let text = match words {
        0 => "YOU NOCLIPPED\nOUT OF REALITY.\nWELCOME HOME",
        1 => "I CAN HEAR\nYOU WALKING",
        2 => return format!("{rooms} ROOMS\nAND STILL\nNO EXIT"),
        3 => return format!("YOU PRESSED W\n{} TIMES.\nI COUNTED", journey.presses.get()),
        4 => "DON'T LOOK\nBEHIND YOU",
        5 => "THE HUM\nIS ME",
        6 => return format!("I AM WAITING\nIN ROOM {}", rooms + 2),
        7 => "COUNT THE LAMPS.\nONE OF THEM\nIS AN EYE",
        8 if journey.pulls.get() == 0 => "DO NOT\nTOUCH\nTHE LEVER",
        8 => return format!("YOU PULLED IT\n{} TIMES.\nWHY", journey.pulls.get()),
        9 => "THERE WAS\nNEVER\nAN EXIT",
        10 => "THE WALLS\nARE NOT YELLOW.\nYOU ARE",
        11 => "LEFT.\nLEFT.\nLEFT.",
        12 => "STOP WALKING\nAND LISTEN",
        13 => "I MADE\nTHIS ROOM\nFOR YOU",
        14 => "YOU LEFT\nA DOOR OPEN\nIN ROOM 1",
        _ => "WE ARE\nALMOST\nTHERE",
    };
    text.to_string()
}

// The line the television types after `answers` answers.
pub(super) fn screen(answers: u32, journey: &Journey) -> String {
    let seconds = journey.frames.get() / FRAMES_PER_SECOND;
    match answers % 6 {
        0 => "> CAN YOU HEAR ME".to_string(),
        1 => "> I AM BEHIND THE WALLPAPER".to_string(),
        2 => format!("> YOU HAVE BEEN HERE {} MINUTES", seconds / 60),
        3 => format!("> {} ROOMS. KEEP WALKING", journey.rooms.get()),
        4 => "> WHY DO YOU KEEP ANSWERING".to_string(),
        _ => "> TURN AROUND".to_string(),
    }
}

// What the camcorder's captions say as the eye walks into its `rooms`-th
// room.
pub(super) fn caption(rooms: u32) -> &'static str {
    const CAPTIONS: [&str; 8] = [
        "[HUMMING]",
        "[SOMETHING WHISPERS] closer",
        "[FOOTSTEPS THAT ARE NOT YOURS]",
        "[THE LIGHTS BUZZ LOUDER]",
        "[A VOICE] you came back",
        "[SILENCE]",
        "[A VOICE] not this door",
        "[BREATHING]",
    ];
    let index = usize::try_from(rooms).unwrap_or(0) % CAPTIONS.len();
    CAPTIONS[index]
}
