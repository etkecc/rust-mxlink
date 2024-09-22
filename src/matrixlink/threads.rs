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

const FETCH_BATCH_SIZE: u32 = 1000;

#[non_exhaustive]
pub struct ThreadGetMessagesParams {
    pub batch_size: u32,
}

impl Default for ThreadGetMessagesParams {
    fn default() -> Self {
        Self {
            batch_size: FETCH_BATCH_SIZE,
        }
    }
}

impl ThreadGetMessagesParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn batch_size(mut self, size: u32) -> Self {
        self.batch_size = size;
        self
    }
}

#[derive(Clone)]
pub struct Threads {
    matrix_link: super::MatrixLink,
}

impl Threads {
    pub(super) fn new(matrix_link: super::MatrixLink) -> Self {
        Self { matrix_link }
    }

    #[tracing::instrument(name="threads_get_messages", skip_all, fields(room_id = room.room_id().as_str(), thread_id = thread_id.as_str()))]
    pub async fn get_messages(
        &self,
        room: &Room,
        thread_id: OwnedEventId,
        params: ThreadGetMessagesParams,
    ) -> Result<Vec<AnyMessageLikeEvent>, matrix_sdk::Error> {
        let mut events: Vec<AnyMessageLikeEvent> = Vec::new();

        tracing::trace!("Fetching thread root event..");
        let thread_event: TimelineEvent = room.event(&thread_id).await?;
        if let AnyTimelineEvent::MessageLike(thread_event) = thread_event.event.deserialize()? {
            events.push(thread_event);
        }

        let mut from: Option<String> = Some(String::new());

        while from.is_some() {
            tracing::trace!(
                ?from,
                batch_size = params.batch_size,
                "Fetching related events batch..",
            );

            let mut request = get_relating_events_with_rel_type::v1::Request::new(
                room.room_id().to_owned(),
                thread_id.clone(),
                RelationType::Thread,
            );

            request.from = from.clone();
            request.limit = Some(params.batch_size.into());

            let http_response = self.matrix_link.client().send(request, None).await?;

            extract_messages_from_http_response(room, http_response.clone(), &mut events).await?;

            from = http_response.next_batch.clone();
        }

        events.sort_by_key(|event| event.origin_server_ts());

        Ok(events)
    }
}

async fn extract_messages_from_http_response(
    room: &Room,
    http_response: get_relating_events_with_rel_type::v1::Response,
    events: &mut Vec<AnyMessageLikeEvent>,
) -> Result<(), matrix_sdk::Error> {
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

    Ok(())
}
