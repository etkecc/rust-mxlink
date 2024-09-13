use matrix_sdk::{
    ruma::{
        events::{reaction::ReactionEventContent, AnySyncMessageLikeEvent, AnySyncTimelineEvent},
        OwnedEventId,
    },
    Room,
};

use tracing::Instrument;

use crate::CallbackError;

#[derive(Clone)]
pub struct Reacting {
    matrix_link: super::MatrixLink,
}

impl Reacting {
    pub(super) fn new(matrix_link: super::MatrixLink) -> Self {
        Self { matrix_link }
    }

    /// Reacts to the given event with a reaction.
    /// reaction_key could be an emoji or a custom string (text).
    pub async fn react(
        &self,
        room: &Room,
        target_event_id: OwnedEventId,
        reaction_key: String,
    ) -> Result<
        matrix_sdk::ruma::api::client::message::send_message_event::v3::Response,
        matrix_sdk::Error,
    > {
        let content =
            ReactionEventContent::new(matrix_sdk::ruma::events::relation::Annotation::new(
                target_event_id,
                reaction_key.to_owned(),
            ));

        room.send(content.clone()).await
    }

    /// Register a callback to be called when a reaction is received in any room and it seems like one that we should handle.
    /// Reactions by our own user are ignored.
    pub fn on_actionable_reaction<F, Fut>(&self, callback: F)
    where
        F: FnOnce(AnySyncTimelineEvent, Room, ReactionEventContent) -> Fut
            + Send
            + 'static
            + Clone
            + Sync,
        Fut: std::future::Future<Output = Result<(), CallbackError>> + Send + 'static,
    {
        let own_user_id = self.matrix_link.user_id().to_owned();

        self.matrix_link.client().add_event_handler(
            move |ev: AnySyncTimelineEvent, room: Room| async move {
                let event_span = tracing::error_span!(
                    "on_actionable_reaction",
                    event_id = ev.event_id().as_str(),
                    room_id = room.room_id().as_str(),
                    sender_id = ev.sender().as_str(),
                    reaction = tracing::field::Empty,
                    reacted_to_event_id = tracing::field::Empty,
                );

                let reaction_content;

                {
                    let _enter = event_span.enter();

                    tracing::trace!(
                        "Sync room message event handler (on_actionable_reaction) for event: {:?}",
                        ev
                    );

                    let AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::Reaction(
                        reaction,
                    )) = ev.clone()
                    else {
                        tracing::trace!("Ignoring non-reaction event");
                        return;
                    };

                    if ev.sender() == own_user_id {
                        tracing::debug!("Ignoring own reaction");
                        return;
                    }

                    let Some(original) = reaction.as_original() else {
                        tracing::debug!("Ignoring redacted reaction");
                        return;
                    };

                    reaction_content = original.content.clone();

                    event_span.record("reaction", reaction_content.relates_to.key.clone());

                    event_span.record(
                        "reacted_to_event_id",
                        reaction_content.relates_to.event_id.as_str(),
                    );
                }

                tokio::spawn(
                    async move {
                        if let Err(err) = callback(ev, room, reaction_content).await {
                            tracing::error!(?err, "Error in callback");
                        }
                    }
                    .instrument(event_span),
                );
            },
        );
    }
}
