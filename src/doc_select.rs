//! Document-level selection model for the WYSIWYG editor (ADR-002 + plan B).
//!
//! The editor renders each block as a SEPARATE egui widget, so egui's own
//! selection cannot cross blocks. This module holds a document-level position
//! (`DocPos`) and selection (`DocSelection`) that DO span blocks. Later increments
//! paint this selection across blocks and route copy / toolbar formatting through
//! it; for now it is an inert, fully unit-tested model with no egui dependency.
#![allow(dead_code)] // foundation for plan B; consumed by later increments (B-2+).

/// A caret position in the document: a byte offset within block `block`'s VISIBLE
/// (rendered, markup-free) text. Ordered by `(block, byte)` so a selection can span
/// blocks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct DocPos {
    pub block: usize,
    pub byte: usize,
}

impl DocPos {
    pub fn new(block: usize, byte: usize) -> Self {
        Self { block, byte }
    }
}

/// A document selection: an `anchor` (where the gesture started) and a `head` (the
/// moving end). `anchor == head` is a collapsed caret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DocSelection {
    pub anchor: DocPos,
    pub head: DocPos,
}

impl DocSelection {
    pub fn caret(p: DocPos) -> Self {
        Self { anchor: p, head: p }
    }

    pub fn is_caret(&self) -> bool {
        self.anchor == self.head
    }

    /// `(start, end)` with `start <= end` (direction-independent).
    pub fn ordered(&self) -> (DocPos, DocPos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// True if `p` lies in the half-open selection range `[start, end)`.
    pub fn contains(&self, p: DocPos) -> bool {
        let (s, e) = self.ordered();
        s <= p && p < e
    }

    /// True if the selection covers any of block `b` (for painting / iteration).
    pub fn touches_block(&self, b: usize) -> bool {
        let (s, e) = self.ordered();
        s.block <= b && b <= e.block
    }

    /// The `[start_byte, end_byte)` slice of the selection WITHIN block `b`, given
    /// that block's visible byte length, or `None` if the block is outside the
    /// selection. A block fully inside the selection returns its whole `[0, len)`.
    pub fn range_in_block(&self, b: usize, block_len: usize) -> Option<(usize, usize)> {
        let (s, e) = self.ordered();
        if b < s.block || b > e.block {
            return None;
        }
        let start = if b == s.block { s.byte.min(block_len) } else { 0 };
        let end = if b == e.block { e.byte.min(block_len) } else { block_len };
        Some((start.min(end), end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_block_then_byte() {
        assert!(DocPos::new(0, 5) < DocPos::new(1, 0));
        assert!(DocPos::new(1, 2) < DocPos::new(1, 7));
        assert_eq!(DocPos::new(1, 2), DocPos::new(1, 2));
    }

    #[test]
    fn ordered_normalizes_direction() {
        let s = DocSelection { anchor: DocPos::new(2, 3), head: DocPos::new(0, 1) };
        assert_eq!(s.ordered(), (DocPos::new(0, 1), DocPos::new(2, 3)));
    }

    #[test]
    fn caret_is_empty_and_contains_nothing() {
        let c = DocSelection::caret(DocPos::new(1, 4));
        assert!(c.is_caret());
        assert!(!c.contains(DocPos::new(1, 4)));
    }

    #[test]
    fn contains_is_half_open() {
        let s = DocSelection { anchor: DocPos::new(0, 2), head: DocPos::new(0, 5) };
        assert!(!s.contains(DocPos::new(0, 1)));
        assert!(s.contains(DocPos::new(0, 2)));
        assert!(s.contains(DocPos::new(0, 4)));
        assert!(!s.contains(DocPos::new(0, 5))); // end is exclusive
    }

    #[test]
    fn range_in_block_spans_multiple_blocks() {
        let s = DocSelection { anchor: DocPos::new(0, 2), head: DocPos::new(2, 3) };
        assert_eq!(s.range_in_block(0, 10), Some((2, 10))); // start block: from 2 to end
        assert_eq!(s.range_in_block(1, 8), Some((0, 8)));   // middle block: whole
        assert_eq!(s.range_in_block(2, 9), Some((0, 3)));   // end block: 0..3
        assert_eq!(s.range_in_block(3, 5), None);           // outside
    }

    #[test]
    fn touches_block_covers_the_range() {
        let s = DocSelection { anchor: DocPos::new(1, 0), head: DocPos::new(3, 0) };
        assert!(!s.touches_block(0));
        assert!(s.touches_block(1));
        assert!(s.touches_block(2));
        assert!(s.touches_block(3));
        assert!(!s.touches_block(4));
    }
}
