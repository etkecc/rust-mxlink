use std::borrow::Borrow;

use tracing::Instrument;

use matrix_sdk::{
    ruma::{
        events::{
            relation::{InReplyTo, Thread},
            room::message::{
                MessageType, OriginalSyncRoomMessageEvent, Relation, RoomMessageEventContent, Relation::Replacement
            },
        },
        OwnedEventId,
    },
    Room, RoomState,
};

use matrix_sdk::ruma::api::client::message::send_message_event;

use crate::{CallbackError, MessageResponseType};

#[derive(Clone)]
pub struct Messaging {
    matrix_link: super::MatrixLink,
}

impl Messaging {
    pub fn new(matrix_link: super::MatrixLink) -> Self {
        Self { matrix_link }
    }

    pub async fn send_text_markdown(
        &self,
        room: &Room,
        message: String,
        response_type: MessageResponseType,
    ) -> Result<send_message_event::v3::Response, matrix_sdk::Error> {
        let mut content = RoomMessageEventContent::text_markdown(message);

        self.send_event(room, &mut content, response_type).await
    }

    pub async fn send_notice_markdown(
        &self,
        room: &Room,
        message: String,
        response_type: MessageResponseType,
    ) -> Result<send_message_event::v3::Response, matrix_sdk::Error> {
        let mut content: RoomMessageEventContent =
            RoomMessageEventContent::notice_markdown(message);

        self.send_event(room, &mut content, response_type).await
    }

    #[tracing::instrument(name="send_event", skip_all, fields(room_id = room.room_id().as_str(), response_type = response_type.as_str()))]
    pub async fn send_event(
        &self,
        room: &Room,
        content: &mut RoomMessageEventContent,
        response_type: MessageResponseType,
    ) -> Result<send_message_event::v3::Response, matrix_sdk::Error> {
        let start_time = std::time::Instant::now();

        tracing::debug!("Sending event..",);

        match response_type {
            MessageResponseType::InRoom => {}
            MessageResponseType::Reply(event_id) => {
                content.relates_to = Some(Relation::Reply {
                    in_reply_to: InReplyTo::new(event_id),
                })
            }
            MessageResponseType::InThread(thread_info) => {
                content.relates_to = Some(Relation::Thread(Thread::plain(
                    thread_info.root_event_id,
                    thread_info.last_event_id,
                )))
            }
        };

        let result = room.send(content.clone()).await;

        let duration = start_time.elapsed();

        tracing::debug!(?duration, "Event sent",);

        result
    }

    pub async fn redact_event(
        &self,
        room: &Room,
        target_event_id: OwnedEventId,
        reason: Option<String>,
    ) -> matrix_sdk::HttpResult<matrix_sdk::ruma::api::client::redact::redact_event::v3::Response>
    {
        room.redact(target_event_id.borrow(), reason.as_deref(), None)
            .await
    }

    /// Register a callback to be called when a message is received in any room and it seems like one that we should handle.
    /// Messages by our own user are ignored.
    /// Messages of type `MessageType::Notice` are ignored.
    /// Messages that represent edits are ignored.
    pub fn on_actionable_room_message<F, Fut>(&self, callback: F)
    where
        F: FnOnce(OriginalSyncRoomMessageEvent, Room) -> Fut + Send + 'static + Clone + Sync,
        Fut: std::future::Future<Output = Result<(), CallbackError>> + Send + 'static,
    {
        let own_user_id = self.matrix_link.user_id().to_owned();

        self.matrix_link.client().add_event_handler(
            move |ev: OriginalSyncRoomMessageEvent, room: Room| async move {
                let event_span = tracing::error_span!(
                    "on_actionable_room_message",
                    event_id = ev.event_id.as_str(),
                    room_id = room.room_id().as_str(),
                    sender_id = ev.sender.as_str()
                );

                {
                    let _enter = event_span.enter();

                    tracing::trace!(
                        "Sync room message event handler (on_actionable_room_message) for event: {:?}",
                        ev,
                    );

                    if room.state() != RoomState::Joined {
                        return;
                    }

                    if let MessageType::Notice(_) = &ev.content.msgtype {
                        // Reason:
                        // > m.notice messages must never be automatically responded to. This helps to prevent infinite-loop situations where two automated clients continuously exchange messages.
                        // See: https://spec.matrix.org/v1.11/client-server-api/#mnotice
                        tracing::trace!("Ignoring notice message type");
                        return;
                    }

                    if let Some(Replacement(_)) = &ev.content.relates_to {
                        tracing::trace!("Ignoring message edit");
                        return;
                    }

                    if ev.sender == own_user_id {
                        tracing::debug!("Ignoring own message");
                        return;
                    }
                }

                tokio::spawn(async move {
                    if let Err(err) = callback(ev, room).await {
                        tracing::error!(?err, "Error in callback");
                    }
                }.instrument(event_span));
            },
        );
    }
}
