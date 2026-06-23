//! Soft function-key labels (`curs_slk`) -- the label-line model.
//!
//! Reconstructs the deterministic, courtable core: the label *registry*
//! (`slk_set` / `slk_label`). The label-line rendering at refresh, the label
//! attributes/colour, and the pre-`initscr` format selection are not byte- or
//! state-reconstructed here (the refresh/`doupdate` of the label line is a seed,
//! and the attribute state has no immediate terminal output).
//!
//! Pinned by court NCURSES.SLK, which compares `slk_label` read-back to ncurses.
//! Non-claim: the label width is `COLS`-derived; this models the standard 8-label
//! layout at 80 columns (width 8), the admitted geometry.

/// The standard soft-label set: 8 labels, each up to [`SoftLabels::WIDTH`] bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftLabels {
    labels: [String; 8],
    width: usize,
}

impl Default for SoftLabels {
    fn default() -> SoftLabels {
        SoftLabels::new()
    }
}

impl SoftLabels {
    /// The label width for the standard 8-label layout at 80 columns.
    pub const WIDTH: usize = 8;

    /// A fresh, empty soft-label set (as after `slk_init` + `initscr`).
    pub fn new() -> SoftLabels {
        SoftLabels { labels: Default::default(), width: Self::WIDTH }
    }

    /// `slk_set(labnum, label, fmt)` -- store `label` for `labnum` (1..=8). The
    /// justification `fmt` is accepted but does not affect `slk_label`, which
    /// returns the label text (truncated to the label width).
    pub fn set(&mut self, labnum: i32, label: &str) -> bool {
        if !(1..=8).contains(&labnum) {
            return false;
        }
        self.labels[(labnum - 1) as usize] = label.to_string();
        true
    }

    /// `slk_label(labnum)` -- the stored label, truncated to the label width;
    /// `None` for an out-of-range `labnum` (ncurses returns `NULL`).
    pub fn label(&self, labnum: i32) -> Option<String> {
        if !(1..=8).contains(&labnum) {
            return None;
        }
        let s = &self.labels[(labnum - 1) as usize];
        Some(s.chars().take(self.width).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_label() {
        let mut s = SoftLabels::new();
        assert!(s.set(1, "File"));
        assert!(s.set(4, "VeryLongLabel"));
        assert_eq!(s.label(1).as_deref(), Some("File"));
        assert_eq!(s.label(4).as_deref(), Some("VeryLong")); // truncated to 8
        assert_eq!(s.label(5).as_deref(), Some("")); // unset
        assert_eq!(s.label(0), None); // out of range
        assert_eq!(s.label(9), None);
        assert!(!s.set(0, "x"));
    }
}
