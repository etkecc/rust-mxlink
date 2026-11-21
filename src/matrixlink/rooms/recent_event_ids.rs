use std::collections::VecDeque;
use std::sync::Mutex;

use matrix_sdk::ruma::{EventId, OwnedEventId};

/// Remembers the IDs of recently handled events, so that an event delivered more than once is only handled once.
///
/// A sync response may deliver the same state event twice: in the `state_after` (or `state`) section
/// and in the `timeline` section. matrix-sdk calls state event handlers for both.
pub(super) struct RecentEventIds {
    capacity: usize,
    event_ids: Mutex<VecDeque<OwnedEventId>>,
}

impl RecentEventIds {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            event_ids: Mutex::new(VecDeque::with_capacity(capacity)),
        }
    }

    /// Records the given event ID and tells if it's new (not seen among the recent ones).
    pub(super) fn insert(&self, event_id: &EventId) -> bool {
        let mut event_ids = self
            .event_ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if event_ids.iter().any(|id| id == event_id) {
            return false;
        }

        if event_ids.len() >= self.capacity {
            event_ids.pop_front();
        }

        event_ids.push_back(event_id.to_owned());

        true
    }
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::OwnedEventId;

    use super::RecentEventIds;

    fn event_id(id: &str) -> OwnedEventId {
        OwnedEventId::try_from(format!("${id}:example.com")).unwrap()
    }

    #[test]
    fn insert_reports_repeats() {
        let recent = RecentEventIds::new(10);

        assert!(recent.insert(&event_id("a")));
        assert!(!recent.insert(&event_id("a")));
        assert!(recent.insert(&event_id("b")));
        assert!(!recent.insert(&event_id("b")));
        assert!(!recent.insert(&event_id("a")));
    }

    #[test]
    fn insert_forgets_the_oldest_beyond_capacity() {
        let recent = RecentEventIds::new(2);

        assert!(recent.insert(&event_id("a")));
        assert!(recent.insert(&event_id("b")));
        assert!(recent.insert(&event_id("c")));

        // "a" was pushed out, "c" is still remembered.
        assert!(!recent.insert(&event_id("c")));
        assert!(recent.insert(&event_id("a")));
    }
}
