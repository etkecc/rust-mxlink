use matrix_sdk::ruma::{events::receipt::ReceiptThread, OwnedEventId};

#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub root_event_id: OwnedEventId,
    pub last_event_id: OwnedEventId,
}

impl Info {
    pub fn new(root_event_id: OwnedEventId, last_event_id: OwnedEventId) -> Self {
        Self {
            root_event_id,
            last_event_id,
        }
    }

    pub fn is_thread_root_only(&self) -> bool {
        self.root_event_id == self.last_event_id
    }
}

impl From<Info> for ReceiptThread {
    fn from(val: Info) -> Self {
        if val.is_thread_root_only() {
            ReceiptThread::Main
        } else {
            ReceiptThread::Thread(val.root_event_id)
        }
    }
}
