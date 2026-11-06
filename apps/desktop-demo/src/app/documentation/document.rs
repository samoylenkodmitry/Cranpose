use cranpose_core::collections::map::HashMap;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::super::markdown::{markdown_to_blocks, split_large_markdown_blocks, MarkdownBlock};

pub(super) enum ReaderItem {
    Heading(usize),
    Block(MarkdownBlock),
    Footer,
}

pub(super) struct GuideDocument {
    pub items: Vec<ReaderItem>,
    starts: Vec<usize>,
    // The reader item of each heading, by the anchor GitHub gives it.
    headings: Vec<(String, usize)>,
}

// The anchors of a document's headings: as GitHub makes them, a heading
// whose anchor came before takes the next number after it.
#[derive(Default)]
struct Anchors(HashMap<String, usize>);

impl Anchors {
    fn next(&mut self, title: &str) -> String {
        let anchor: String = title
            .trim()
            .chars()
            .filter_map(|c| match c {
                ' ' => Some('-'),
                c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
                _ => None,
            })
            .flat_map(char::to_lowercase)
            .collect();
        let seen = self.0.entry(anchor.clone()).or_insert(0);
        let numbered = match *seen {
            0 => anchor,
            count => format!("{anchor}-{count}"),
        };
        *seen += 1;
        numbered
    }
}

// The text of each heading in `markdown`, as it shows.
fn heading_texts(markdown: &str) -> Vec<String> {
    let mut texts = Vec::new();
    let mut heading = None::<String>;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = heading.as_mut() {
                    heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => texts.extend(heading.take()),
            _ => {}
        }
    }
    texts
}

impl GuideDocument {
    fn new() -> Self {
        let mut items = Vec::new();
        let mut starts = Vec::with_capacity(super::chapters().len() + 1);
        let mut anchors = Anchors::default();
        let mut headings = Vec::new();
        // The reader's list starts with its header, so a document item is
        // the list item after it.
        for (index, chapter) in super::chapters().iter().enumerate() {
            starts.push(items.len() + 1);
            headings.push((anchors.next(chapter.title), items.len() + 1));
            items.push(ReaderItem::Heading(index));
            let mut wanted = heading_texts(chapter.body).into_iter().peekable();
            for block in
                split_large_markdown_blocks(markdown_to_blocks(chapter.body, super::GUIDE_URL))
            {
                if let MarkdownBlock::Text(text) = &block {
                    if wanted.next_if(|heading| *heading == text.text).is_some() {
                        headings.push((anchors.next(&text.text), items.len() + 1));
                    }
                }
                items.push(ReaderItem::Block(block));
            }
            items.push(ReaderItem::Footer);
        }
        starts.push(items.len() + 1);
        Self {
            items,
            starts,
            headings,
        }
    }

    // The list item of the heading `anchor` names.
    pub(super) fn heading(&self, anchor: &str) -> Option<usize> {
        self.headings
            .iter()
            .find(|(name, _)| name == anchor)
            .map(|(_, item)| *item)
    }

    pub(super) fn chapter(&self, item: usize) -> usize {
        self.starts
            .partition_point(|start| *start <= item)
            .saturating_sub(1)
            .min(self.starts.len() - 2)
    }

    pub(super) fn position(&self, item: usize, fraction: f32) -> f32 {
        let chapter = self.chapter(item);
        let start = self.starts[chapter];
        chapter as f32
            + (item.saturating_sub(start) as f32 + fraction)
                / (self.starts[chapter + 1] - start) as f32
    }

    pub(super) fn item(&self, position: f32) -> (usize, f32) {
        let chapter = (position as usize).min(self.starts.len() - 2);
        let offset = position.fract() * (self.starts[chapter + 1] - self.starts[chapter]) as f32;
        (
            self.starts[chapter] + offset.floor() as usize,
            offset.fract(),
        )
    }
}

thread_local! {
    pub(super) static DOCUMENT: GuideDocument = GuideDocument::new();
}
