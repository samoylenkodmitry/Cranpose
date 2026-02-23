/// How overflowing text should be handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
    Visible,
    StartEllipsis,
    MiddleEllipsis,
}

/// Text layout behavior options matching Compose `BasicText` controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextLayoutOptions {
    pub overflow: TextOverflow,
    pub soft_wrap: bool,
    pub max_lines: usize,
    pub min_lines: usize,
}

impl Default for TextLayoutOptions {
    fn default() -> Self {
        Self {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        }
    }
}

impl TextLayoutOptions {
    pub fn normalized(self) -> Self {
        let min_lines = self.min_lines.max(1);
        let max_lines = self.max_lines.max(min_lines);
        Self {
            overflow: self.overflow,
            soft_wrap: self.soft_wrap,
            max_lines,
            min_lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_enforces_minimum_one_line() {
        let options = TextLayoutOptions {
            min_lines: 0,
            max_lines: 0,
            ..Default::default()
        }
        .normalized();

        assert_eq!(options.min_lines, 1);
        assert_eq!(options.max_lines, 1);
    }

    #[test]
    fn normalized_ensures_max_not_smaller_than_min() {
        let options = TextLayoutOptions {
            min_lines: 3,
            max_lines: 1,
            ..Default::default()
        }
        .normalized();

        assert_eq!(options.min_lines, 3);
        assert_eq!(options.max_lines, 3);
    }
}
