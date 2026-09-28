//! Deciding from before/after captures whether a drag really moved its item. Port of
//! `move_verifier.py`'s comparison half (`regions_differ`, and `post_verify`'s "source cleared or
//! destination filled" rule); capturing lives with the executor and [`super::Machine`].

use screen::Frame;

/// Mean absolute difference per colour channel (0-255) above which a probe counts as changed.
/// Python: `MoveVerifier(change_threshold=8.0)`.
pub const CHANGE_THRESHOLD: f64 = 8.0;

/// Mean absolute difference between two same-sized frames, per colour channel, 0-255. `None` when
/// the sizes differ or the frames are empty, which the verifier treats as "can't tell". Python:
/// `np.mean(np.abs(before.astype(int16) - after.astype(int16)))`.
pub fn mean_abs_diff(a: &Frame, b: &Frame) -> Option<f64> {
    if a.width() != b.width() || a.height() != b.height() || a.data().is_empty() {
        return None;
    }
    let total: u64 = a.data().iter().zip(b.data()).map(|(&x, &y)| u64::from(x.abs_diff(y))).sum();
    Some(total as f64 / a.data().len() as f64)
}

/// How much the source and destination probes changed across a drag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveCheck {
    pub source_diff: f64,
    pub dest_diff: f64,
}

impl MoveCheck {
    pub fn source_changed(&self) -> bool {
        self.source_diff > CHANGE_THRESHOLD
    }

    pub fn dest_changed(&self) -> bool {
        self.dest_diff > CHANGE_THRESHOLD
    }

    /// Verified when the source cleared or the destination filled. Python used the same OR: one
    /// confirmed change is strong evidence the drag happened, and requiring both would fail every
    /// move whose item art happens to look alike at both probes.
    pub fn verified(&self) -> bool {
        self.source_changed() || self.dest_changed()
    }
}

/// Compares `(source, destination)` probes taken before a drag with those taken after it. A pair
/// that can't be compared (a capture of the wrong size) counts as unchanged, so it never verifies
/// a move on its own.
pub fn compare_probes(before: (&Frame, &Frame), after: (&Frame, &Frame)) -> MoveCheck {
    MoveCheck {
        source_diff: mean_abs_diff(before.0, after.0).unwrap_or(0.0),
        dest_diff: mean_abs_diff(before.1, after.1).unwrap_or(0.0),
    }
}
