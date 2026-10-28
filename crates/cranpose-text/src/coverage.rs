// The characters a font maps to glyphs, as a two-level bitmap.

// One bit per code point in pages of 256. `pages[0]` is the empty page, which
// every page of code points the font lacks shares; `index` holds a page number
// for each 256 code points up to the font's highest one, so a lookup is two
// loads and a bit test.
pub(crate) struct Coverage {
    index: Box<[u16]>,
    pages: Box<[[u64; 4]]>,
}

impl Coverage {
    pub(crate) fn from_codepoints(codepoints: impl Iterator<Item = u32>) -> Self {
        let mut index: Vec<u16> = Vec::new();
        let mut pages: Vec<[u64; 4]> = vec![[0; 4]];
        for codepoint in codepoints {
            let Ok(codepoint) = usize::try_from(codepoint) else {
                continue;
            };
            let page = codepoint >> 8;
            if index.len() <= page {
                index.resize(page + 1, 0);
            }
            if index[page] == 0 {
                let Ok(number) = u16::try_from(pages.len()) else {
                    continue;
                };
                index[page] = number;
                pages.push([0; 4]);
            }
            pages[usize::from(index[page])][(codepoint >> 6) & 3] |= 1 << (codepoint & 63);
        }
        Self {
            index: index.into_boxed_slice(),
            pages: pages.into_boxed_slice(),
        }
    }

    // The ASCII characters the font maps, bit `n` for code point `n`.
    pub(crate) fn ascii(&self) -> u128 {
        self.index.first().map_or(0, |&page| {
            let [low, high, ..] = self.pages[usize::from(page)];
            u128::from(low) | u128::from(high) << 64
        })
    }

    pub(crate) fn contains(&self, ch: char) -> bool {
        let Ok(codepoint) = usize::try_from(u32::from(ch)) else {
            return false;
        };
        self.index.get(codepoint >> 8).is_some_and(|&page| {
            self.pages[usize::from(page)][(codepoint >> 6) & 3] >> (codepoint & 63) & 1 != 0
        })
    }
}
