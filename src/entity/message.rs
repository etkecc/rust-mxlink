use matrix_sdk::ruma::OwnedEventId;

#[derive(Debug, Clone)]
pub enum ResponseType {
    InRoom,
    Reply(OwnedEventId),
    InThread(super::thread::Info),
}

impl ResponseType {
    pub fn as_str(&self) -> &str {
        match self {
            ResponseType::InRoom => "InRoom",
            ResponseType::Reply(_) => "Reply",
            ResponseType::InThread(_) => "InThread",
        }
    }
}
