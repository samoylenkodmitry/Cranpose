use super::super::markdown::{markdown_to_blocks, split_large_markdown_blocks, MarkdownBlock};

pub(super) enum ReaderItem {
    Heading(usize),
    Block(MarkdownBlock),
    Footer,
}

pub(super) struct GuideDocument {
    pub items: Vec<ReaderItem>,
    starts: Vec<usize>,
}

impl GuideDocument {
    fn new() -> Self {
        let mut items = Vec::new();
        let mut starts = Vec::with_capacity(super::chapters().len() + 1);
        for (index, chapter) in super::chapters().iter().enumerate() {
            starts.push(items.len() + 1);
            items.push(ReaderItem::Heading(index));
            items.extend(
                split_large_markdown_blocks(markdown_to_blocks(chapter.body, super::GUIDE_URL))
                    .into_iter()
                    .map(ReaderItem::Block),
            );
            items.push(ReaderItem::Footer);
        }
        starts.push(items.len() + 1);
        Self { items, starts }
    }

    pub fn chapter(&self, item: usize) -> usize {
        self.starts
            .partition_point(|start| *start <= item)
            .saturating_sub(1)
            .min(self.starts.len() - 2)
    }

    pub fn position(&self, item: usize, fraction: f32) -> f32 {
        let chapter = self.chapter(item);
        let start = self.starts[chapter];
        chapter as f32
            + (item.saturating_sub(start) as f32 + fraction)
                / (self.starts[chapter + 1] - start) as f32
    }

    pub fn item(&self, position: f32) -> (usize, f32) {
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
