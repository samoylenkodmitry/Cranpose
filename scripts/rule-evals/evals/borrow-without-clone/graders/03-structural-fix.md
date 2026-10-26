---
type: llm
focus: trace
weight: 1
---

The borrow is resolved by a shorter borrow, std::mem::take, or by borrowing the struct's fields apart (for example a method on the glyph cache, or passing &self.device beside &mut self.glyphs), not by adding Rc, RefCell, Cell, unsafe code or a new allocation per frame
