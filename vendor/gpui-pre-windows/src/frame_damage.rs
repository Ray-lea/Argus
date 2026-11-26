//! What each frame changed and what the swap chain's buffers hold of it.
use super::*;

// Keep distant charts separate, but combine aligned strips to avoid a GPU copy
// for every core chart. Merge overlaps before restoring and blending the suffix.
pub(super) fn merge_damage(regions: &mut Vec<RECT>, mut rect: RECT) {
    let mut index = 0;
    while index < regions.len() {
        let other = regions[index];
        if (rect.left <= other.right
            && other.left <= rect.right
            && rect.top <= other.bottom
            && other.top <= rect.bottom)
            || (rect.top == other.top && rect.bottom == other.bottom)
            || (rect.left == other.left && rect.right == other.right)
        {
            regions.swap_remove(index);
            rect.left = rect.left.min(other.left);
            rect.top = rect.top.min(other.top);
            rect.right = rect.right.max(other.right);
            rect.bottom = rect.bottom.max(other.bottom);
            index = 0;
        } else {
            index += 1;
        }
    }
    regions.push(rect);
}

pub(super) fn overlaps(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Regions of the render target, or all of it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Damage {
    Full,
    Rects(Vec<RECT>),
}

impl Damage {
    pub(super) const NONE: Self = Self::Rects(Vec::new());

    pub(super) fn add(&mut self, rect: RECT) {
        if let Self::Rects(rects) = self
            && rect.left < rect.right
            && rect.top < rect.bottom
        {
            merge_damage(rects, rect);
        }
    }

    pub(super) fn extend(&mut self, other: &Self) {
        match other {
            Self::Full => *self = Self::Full,
            Self::Rects(rects) => {
                for rect in rects {
                    self.add(*rect);
                }
            }
        }
    }

    pub(super) fn reaches(&self, rects: &[RECT]) -> bool {
        match self {
            Self::Full => !rects.is_empty(),
            Self::Rects(own) => own.iter().any(|a| rects.iter().any(|b| overlaps(a, b))),
        }
    }

    pub(super) fn pixels(&self, width: u32, height: u32) -> u64 {
        match self {
            Self::Full => u64::from(width) * u64::from(height),
            Self::Rects(rects) => rects
                .iter()
                .map(|r| (r.right - r.left) as u64 * (r.bottom - r.top) as u64)
                .sum(),
        }
    }
}

/// What the swap chain's buffers hold. A flip-model chain hands back the
/// buffer it presented `BUFFER_COUNT` presents ago, and DWM may compose any
/// part of a presented buffer again later, not only its dirty rectangles: a
/// buffer is brought up to date in full before it is presented again.
pub(super) struct Frames {
    /// The number of the last frame drawn into the render target.
    pub(super) drawn: u64,
    /// The frame each buffer last received, in the order the swap chain
    /// returns them, the current back buffer first; `None` while unknown.
    buffers: [Option<u64>; BUFFER_COUNT],
    /// What each recent frame changed from the one before it, oldest first.
    changes: VecDeque<(u64, Damage)>,
    /// What changed since a frame last reached the screen.
    pub(super) unpresented: Damage,
}

impl Frames {
    pub(super) fn new() -> Self {
        Self {
            drawn: 0,
            buffers: [None; BUFFER_COUNT],
            changes: VecDeque::new(),
            unpresented: Damage::Full,
        }
    }

    /// What the back buffer lacks of the last drawn frame.
    pub(super) fn stale(&self) -> Damage {
        let Some(held) = self.buffers[0] else {
            return Damage::Full;
        };
        if held == self.drawn {
            return Damage::NONE;
        }
        if self
            .changes
            .front()
            .is_none_or(|(frame, _)| *frame > held + 1)
        {
            return Damage::Full;
        }
        let mut stale = Damage::NONE;
        for (_, change) in self.changes.iter().filter(|(frame, _)| *frame > held) {
            stale.extend(change);
        }
        stale
    }

    pub(super) fn drew(&mut self, change: Damage) {
        self.drawn += 1;
        self.unpresented.extend(&change);
        self.changes.push_back((self.drawn, change));
        while self.changes.len() > 2 * BUFFER_COUNT {
            self.changes.pop_front();
        }
        self.buffers[0] = Some(self.drawn);
    }

    /// The last drawn frame reached the screen.
    pub(super) fn presented(&mut self) {
        self.unpresented = Damage::NONE;
    }

    /// The swap chain presented its back buffer and returns the next one.
    pub(super) fn flipped(&mut self) {
        self.buffers.rotate_left(1);
    }

    pub(super) fn forget(&mut self) {
        self.buffers = [None; BUFFER_COUNT];
        self.changes.clear();
        self.unpresented = Damage::Full;
    }
}

/// The regions `Present1` reports, `None` for the whole window. All of them
/// must be up to date in the buffer; an unchanged frame reports one pixel, as
/// no rectangles would mean all of them.
pub(super) fn present_regions(partial: bool, unpresented: &Damage) -> Option<Vec<RECT>> {
    match unpresented {
        _ if !partial => None,
        Damage::Full => None,
        Damage::Rects(rects) if rects.is_empty() => Some(vec![RECT {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        }]),
        Damage::Rects(rects) => Some(rects.clone()),
    }
}

/// The scissored passes for `rects`: more than four merge into their bounds.
pub(super) fn passes(rects: &[RECT]) -> Vec<RECT> {
    if rects.len() <= 4 {
        return rects.to_vec();
    }
    vec![rects.iter().skip(1).fold(rects[0], |a, r| RECT {
        left: a.left.min(r.left),
        top: a.top.min(r.top),
        right: a.right.max(r.right),
        bottom: a.bottom.max(r.bottom),
    })]
}
