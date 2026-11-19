use cranpose_core::collections::pass_aged::IDLE_PASSES;
use cranpose_text::PositionedGlyph;
use cranpose_ui::{
    AppContext, MemoryHeadroom, TextLayoutOptions, TextStyle, prepare_text_layout,
    set_memory_headroom, text::AnnotatedString,
};

// More texts than the shared cache keeps, 1,024 of them.
const PAST_THE_CACHE: usize = 1_100;
// A width the paragraph wraps at, and one it fits on a line at.
const NARROW: f32 = 60.0;
const WIDE: f32 = 2_000.0;

// What a renderer draws of a layout: its size and each glyph at its place.
#[derive(Debug, PartialEq)]
struct Picture {
    width: f32,
    height: f32,
    glyphs: Vec<PositionedGlyph>,
}

fn picture(text: &AnnotatedString, max_width: f32) -> Picture {
    let prepared = prepare_text_layout(
        text,
        &TextStyle::default(),
        TextLayoutOptions::default(),
        Some(max_width),
    );
    let layout = &prepared.layout;
    let glyphs = (0..layout.line_count())
        .flat_map(|line| layout.glyph_runs(line))
        .flat_map(|run| run.positions().collect::<Vec<_>>())
        .collect();
    Picture {
        width: layout.width(),
        height: layout.height(),
        glyphs,
    }
}

// How the shared cache lets a text go.
#[derive(Clone, Copy, Debug)]
enum LetGo {
    // Texts laid out once each fill it past its room.
    FullCache,
    // Passes that ask for other texts go by until it ages the text out.
    Idle,
}

// The pictures of a paragraph laid out at two widths, before and after the
// shared cache lets it go as `let_go` says, on a host with `headroom`. The
// other texts lay out again, in a pass of their own, into what the cache
// let go.
fn pictures_around(let_go: LetGo, headroom: MemoryHeadroom) -> [(Picture, Picture); 2] {
    let paragraph = AnnotatedString::from("bring the layout back as it was drawn");
    let others: Vec<AnnotatedString> = (0..PAST_THE_CACHE)
        .map(|index| AnnotatedString::from(format!("other text {index}")))
        .collect();
    let idle_passes = match headroom {
        MemoryHeadroom::Ample => IDLE_PASSES,
        MemoryHeadroom::Tight => 1,
    };
    let context = AppContext::new();
    context.enter(|| {
        set_memory_headroom(headroom);
        let before = [picture(&paragraph, NARROW), picture(&paragraph, WIDE)];
        match let_go {
            LetGo::FullCache => {
                context.start_text_frame();
                for other in &others {
                    picture(other, WIDE);
                }
            }
            LetGo::Idle => {
                for other in others
                    .iter()
                    .take(1 + usize::try_from(idle_passes).unwrap_or(0))
                {
                    context.start_text_frame();
                    picture(other, WIDE);
                }
            }
        }
        context.start_text_frame();
        for other in &others {
            picture(other, NARROW);
        }
        context.start_text_frame();
        let [narrow, wide] = before;
        [
            (narrow, picture(&paragraph, NARROW)),
            (wide, picture(&paragraph, WIDE)),
        ]
    })
}

#[test]
fn a_text_at_two_widths_draws_as_before_once_the_cache_lets_it_go() {
    for let_go in [LetGo::FullCache, LetGo::Idle] {
        for headroom in [MemoryHeadroom::Ample, MemoryHeadroom::Tight] {
            let pictures = pictures_around(let_go, headroom);
            let [(narrow, _), (wide, _)] = &pictures;
            assert!(
                narrow.height > wide.height,
                "the paragraph wraps at the narrow width"
            );
            for (before, after) in &pictures {
                assert_eq!(before, after, "{let_go:?} on a {headroom:?} heap");
            }
        }
    }
}
