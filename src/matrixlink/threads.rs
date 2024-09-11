use matrix_sdk::{
    deserialized_responses::TimelineEvent,
    ruma::{
        api::client::relations::get_relating_events_with_rel_type,
        events::{
            relation::RelationType, AnyMessageLikeEvent, AnySyncMessageLikeEvent,
            AnySyncTimelineEvent, AnyTimelineEvent, SyncMessageLikeEvent,
        },
        OwnedEventId,
    },
    Room,
};

#[derive(Clone)]
pub struct Threads {
    matrix_link: super::MatrixLink,
}

impl Threads {
    pub fn new(matrix_link: super::MatrixLink) -> Self {
        Self { matrix_link }
    }

    pub async fn get_messages(
        &self,
        room: &Room,
        thread_id: OwnedEventId,
    ) -> Result<Vec<AnyMessageLikeEvent>, matrix_sdk::Error> {
        let thread_event: TimelineEvent = room.event(&thread_id).await?;

        let request = get_relating_events_with_rel_type::v1::Request::new(
            room.room_id().to_owned(),
            thread_id,
            RelationType::Thread,
        );

        let http_response = self.matrix_link.client().send(request, None).await?;

        let mut events: Vec<AnyMessageLikeEvent> = Vec::with_capacity(http_response.chunk.len());

        if let AnyTimelineEvent::MessageLike(thread_event) = thread_event.event.deserialize()? {
            events.push(thread_event);
        }

        for event in http_response.chunk.iter().rev() {
            if let Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomEncrypted(
                SyncMessageLikeEvent::Original(_),
            ))) = event.deserialize_as::<AnySyncTimelineEvent>()
            {
                if let Ok(event) = room.decrypt_event(event.cast_ref()).await {
                    if let AnyTimelineEvent::MessageLike(ev) = event.event.deserialize()? {
                        events.push(ev);
                    }
                } else {
                    tracing::error!("failed-to-decrypt?: {:?}", event);
                }
            } else {
                events.push(event.deserialize()?);
            };
        }

        Ok(events)
    }
}
