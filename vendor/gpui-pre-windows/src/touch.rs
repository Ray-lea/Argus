//! Per-window contacts. Windows may recycle pointer IDs after a release;
//! GPUI touch IDs must remain unique, including across long-press timers.
use std::collections::HashMap;

use gpui::{Pixels, Point, TouchEvent, TouchId, TouchPhase};

#[derive(Default)]
pub(crate) struct TouchInput {
    next_id: u64,
    contacts: HashMap<u32, TouchEvent>,
}

impl TouchInput {
    pub(crate) fn contains(&self, pointer: u32) -> bool {
        self.contacts.contains_key(&pointer)
    }

    pub(crate) fn update(
        &mut self,
        pointer: u32,
        phase: TouchPhase,
        position: Point<Pixels>,
    ) -> Option<TouchEvent> {
        if phase == TouchPhase::Started {
            if self.contains(pointer) {
                return None;
            }
            self.next_id += 1;
            self.contacts.insert(
                pointer,
                TouchEvent {
                    id: TouchId(self.next_id),
                    ..Default::default()
                },
            );
        }
        let contact = self.contacts.get_mut(&pointer)?;
        contact.phase = phase;
        contact.position = position;
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.contacts.remove(&pointer)
        } else {
            Some(contact.clone())
        }
    }

    pub(crate) fn cancel(&mut self, pointer: u32) -> Option<TouchEvent> {
        let mut event = self.contacts.remove(&pointer)?;
        event.phase = TouchPhase::Cancelled;
        Some(event)
    }

    pub(crate) fn cancel_all(&mut self) -> Vec<TouchEvent> {
        self.contacts
            .drain()
            .map(|(_, mut event)| {
                event.phase = TouchPhase::Cancelled;
                event
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::TouchInput;
    use gpui::{TouchPhase, point, px};

    #[test]
    fn recycled_windows_pointer_gets_a_new_touch_id() {
        let mut input = TouchInput::default();
        let at = point(px(15.), px(30.));
        let first = input.update(7, TouchPhase::Started, at).unwrap();
        let moved = input.update(7, TouchPhase::Moved, at).unwrap();
        let ended = input.update(7, TouchPhase::Ended, at).unwrap();
        let next = input.update(7, TouchPhase::Started, at).unwrap();
        assert_eq!(first.id, moved.id);
        assert_eq!(first.id, ended.id);
        assert_ne!(first.id, next.id);
    }

    #[test]
    fn capture_loss_cancels_at_the_last_position_without_a_release() {
        let mut input = TouchInput::default();
        input.update(3, TouchPhase::Started, point(px(20.), px(40.)));
        let at = point(px(-10.), px(75.));
        input.update(3, TouchPhase::Moved, at);
        let cancelled = input.cancel(3).unwrap();
        assert_eq!(cancelled.phase, TouchPhase::Cancelled);
        assert_eq!(cancelled.position, at);
        assert!(input.update(3, TouchPhase::Ended, at).is_none());
        assert!(input.cancel(3).is_none());
    }

    #[test]
    fn simultaneous_contacts_stay_independent_and_cancel_together() {
        let mut input = TouchInput::default();
        let at = point(px(100.), px(200.));
        let a = input.update(1, TouchPhase::Started, at).unwrap();
        let b = input.update(2, TouchPhase::Started, at).unwrap();
        assert_ne!(a.id, b.id);
        assert!(input.update(1, TouchPhase::Started, at).is_none());
        assert!(input.update(9, TouchPhase::Moved, at).is_none());
        let cancelled = input.cancel_all();
        assert_eq!(cancelled.len(), 2);
        assert!(cancelled.iter().all(|e| e.phase == TouchPhase::Cancelled));
        assert!(input.cancel_all().is_empty());
    }
}
