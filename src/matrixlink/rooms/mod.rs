mod typing_notice;

use matrix_sdk::{
    ruma::events::{
        room::member::{MembershipState, StrippedRoomMemberEvent},
        AnySyncStateEvent, AnySyncTimelineEvent,
    },
    Room, RoomMemberships, RoomState,
};

use thiserror::Error;

use tracing::Instrument;

use crate::{CallbackError, InvitationDecision};

pub use typing_notice::TypingNoticeGuard;

const MAX_JOIN_DELAY_SECONDS: u64 = 3600;

#[derive(Error, Debug)]
pub enum JoinError {
    #[error(
        "Refusing to retry joining room due to expontential backoff delay being too large: {0}"
    )]
    BackOffTooLarge(u64),
}

#[derive(Clone)]
pub struct Rooms {
    matrix_link: super::MatrixLink,
}

impl Rooms {
    pub(super) fn new(matrix_link: super::MatrixLink) -> Self {
        Self { matrix_link }
    }

    #[tracing::instrument(skip_all, name="own_display_name_in_room", fields(room_id = room.room_id().as_str()))]
    pub async fn own_display_name_in_room(
        &self,
        room: &Room,
    ) -> matrix_sdk::Result<Option<String>> {
        let members = room.members(RoomMemberships::JOIN).await?;

        for member in members {
            if !member.is_account_user() {
                // Another user, not us.
                continue;
            }

            return Ok(member.display_name().map(|s| s.to_owned()));
        }

        Ok(None)
    }

    /// Starts sending typing notices for the given room and returns a guard object.
    ///
    /// If multiple callers invoke this method for the same room, only the first caller will start
    /// the typing notice sending loop and it will remain active until all callers have released their guards.
    ///
    /// When all guard objects for a given room have gone out of scope, the typing notice will be turned off.
    #[tracing::instrument(skip_all, name="start_typing_notice", fields(room_id = room.room_id().as_str()))]
    pub async fn start_typing_notice(&self, room: &Room) -> TypingNoticeGuard {
        typing_notice::start_typing_notice(self.matrix_link.clone(), room).await
    }

    #[tracing::instrument(skip_all, name="join_with_retries", fields(room_id = room.room_id().as_str(), max_delay_seconds = ?max_delay_seconds))]
    async fn join_with_retries(
        &self,
        room: &Room,
        max_delay_seconds: Option<u64>,
    ) -> Result<(), JoinError> {
        tracing::debug!("Joining room");

        let mut delay = 2;

        while let Err(err) = room.join().await {
            // retry autojoin due to synapse sending invites, before the
            // invited user can join for more information see
            // https://github.com/matrix-org/synapse/issues/4345
            tracing::warn!(?err, ?delay, "Failed to join. Retrying..",);

            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            delay *= 2;

            if let Some(max_delay_seconds) = max_delay_seconds {
                if delay > max_delay_seconds {
                    return Err(JoinError::BackOffTooLarge(delay));
                }
            }
        }

        tracing::info!("Successfully joined room");

        Ok(())
    }

    /// Register a callback to be called when an invitation for the room arrives.
    /// The callback is expected to return a decision as to whether the room should be joined or not.
    pub fn on_invitation<F, Fut>(&self, callback: F)
    where
        F: FnOnce(StrippedRoomMemberEvent, Room) -> Fut + Send + 'static + Clone + Sync,
        Fut: std::future::Future<Output = Result<InvitationDecision, CallbackError>>
            + Send
            + 'static,
    {
        let self_ref = self.clone();
        let own_user_id = self.matrix_link.user_id().to_owned();

        self.matrix_link.client().add_event_handler(
            |room_member: StrippedRoomMemberEvent, room: Room| async move {
                let event_span = tracing::error_span!(
                    "on_invitation",
                    room_id = room.room_id().as_str(),
                    sender_id = room_member.sender.as_str(),
                    decision = tracing::field::Empty,
                );

                {
                    let _enter = event_span.enter();

                    if room_member.state_key != own_user_id {
                        // Invite for someone else. Ignore.
                        return;
                    }

                    if room.state() != RoomState::Invited {
                        return;
                    }

                    tracing::debug!(
                        "Deciding how to respond to room invitation",
                    );
                }

                let decision = callback(room_member.clone(), room.clone()).instrument(event_span.clone()).await;

                match decision {
                    Err(err) => {
                        let _enter = event_span.enter();

                        tracing::error!(
                            ?err,
                            "Error while determining decision for joining. The invitation will be ignored",
                        );
                    }
                    Ok(status) => {
                        event_span.record("decision", format!("{:?}", status));

                        tracing::info!(
                            "Decision for joining {} (due to invitation from {}) is {:?}",
                            room.room_id(),
                            room_member.sender.clone().as_str(),
                            status,
                        );

                        match status {
                            InvitationDecision::Join => {
                                tokio::spawn(async move {
                                    if let Err(err) = self_ref.join_with_retries(&room, Some(MAX_JOIN_DELAY_SECONDS)).await {
                                        tracing::error!(?err, "Failed to join room");
                                    } else {
                                        tracing::info!("Accepted invitation and joined");
                                    }
                                }.instrument(event_span));
                            }
                            InvitationDecision::Reject => {
                                tokio::spawn(async move {
                                    let result = room.leave().await;
                                    if let Err(err) = result {
                                        tracing::error!(?err, "Failed to reject invitation");
                                    } else {
                                        tracing::info!("Rejected invitation and left");
                                    }
                                }.instrument(event_span));
                            }
                        }
                    }
                }
            },
        );
    }

    /// Register a callback to be called when a room has been joined.
    pub fn on_joined<F, Fut>(&self, callback: F)
    where
        F: FnOnce(AnySyncTimelineEvent, Room) -> Fut + Send + 'static + Clone + Sync,
        Fut: std::future::Future<Output = Result<(), CallbackError>> + Send + 'static,
    {
        let own_user_id = self.matrix_link.user_id().to_owned();

        self.matrix_link.client().add_event_handler(
            move |ev: AnySyncTimelineEvent, room: Room| async move {
                let event_span = tracing::error_span!(
                    "on_joined",
                    event_id = ev.event_id().as_str(),
                    room_id = room.room_id().as_str(),
                    sender_id = ev.sender().as_str()
                );

                {
                    let _enter = event_span.enter();

                    tracing::trace!(
                        "Sync timeline event handler (on_joined_room) for event: {:?}",
                        ev
                    );

                    let membership = if let AnySyncTimelineEvent::State(
                        AnySyncStateEvent::RoomMember(membership),
                    ) = ev.clone()
                    {
                        membership
                    } else {
                        tracing::trace!("Ignoring non-state/non-membership event");
                        return;
                    };

                    match membership.membership() {
                        MembershipState::Join => {}
                        event_type => {
                            tracing::debug!(?event_type, "Ignoring non-join membership event");
                            return;
                        }
                    }

                    if membership.state_key() != own_user_id.as_str() {
                        tracing::debug!(
                            state_key = membership.state_key().as_str(),
                            "Ignoring join for another user"
                        );
                        return;
                    }


                    // We wish to ignore events that are a result of the bot's display name changing.
                    // When that happens, the event's content still looks like a join event:
                    //  > "content": {"displayname": "some_display_name", "membership": "join"}
                    //
                    // The difference is that join events that are a result to an invitation have a `prev_content` field like this:
                    // > "prev_content": {"displayname": "some_display_name", "membership": "invite"}
                    //
                    // Join events that are a result of a display name change have a `prev_content` field like this:
                    // > "prev_content": {"displayname": "some_display_name", "membership": "join"}
                    //
                    // That is.. it's only an actual join event if the `membership` field in `prev_content` was not "join" already.

                    let Some(original) = membership.as_original() else {
                        tracing::debug!("Ignoring redacted join event");
                        return;
                    };

                    let Some(unsigned) = original.prev_content() else {
                        tracing::debug!("Ignoring join event without prev_content");
                        return;
                    };

                    if let MembershipState::Join = unsigned.membership {
                        tracing::debug!("Ignoring join event that supersedes another join event (likely a displayname/avatar change, etc.)");
                        return;
                    };
                }

                if let Err(err) = callback(ev, room).instrument(event_span).await {
                    tracing::error!(?err, "Error in callback");
                }
            },
        );
    }

    /// Register a callback to be called when we've determined to be the last member in the room.
    /// When this happens, you usually may wish to clean up and leave the room.
    pub fn on_being_last_member<F, Fut>(&self, callback: F)
    where
        F: FnOnce(AnySyncTimelineEvent, Room) -> Fut + Send + 'static + Clone + Sync,
        Fut: std::future::Future<Output = Result<(), CallbackError>> + Send + 'static,
    {
        let own_user_id = self.matrix_link.user_id().to_owned();

        self.matrix_link.client().add_event_handler(
            move |ev: AnySyncTimelineEvent, room: Room| async move {
                let event_span = tracing::error_span!(
                    "on_being_last_member",
                    room_id = room.room_id().as_str(),
                    sender_id = ev.sender().as_str(),
                );

                {
                    let _enter = event_span.enter();

                    tracing::trace!(
                        "Sync timeline event handler (on_being_last_member_in_room) for event: {:?}",
                        ev
                    );

                    let membership =
                        if let AnySyncTimelineEvent::State(AnySyncStateEvent::RoomMember(membership)) =
                            ev.clone()
                        {
                            membership
                        } else {
                            tracing::trace!("Ignoring non-state/non-membership event");
                            return;
                        };

                    match membership.membership() {
                        MembershipState::Leave | MembershipState::Ban => {}
                        _ => {
                            tracing::debug!("Ignoring non-leave/ban membership event");
                            return;
                        }
                    }

                    if membership.sender() == own_user_id {
                        tracing::debug!("Ignoring leave/ban initiated by us");
                        return;
                    }

                    if membership.state_key() == own_user_id.as_str() {
                        tracing::debug!("Ignoring leave/ban targeting us");
                        return;
                    }
                }

                // RoomMemberships::ACTIVE is another possibility (which includes invited members),
                // but we don't care if someone is invited and may possibly join later (or not).
                // If we're the only actually-active member right now, it sounds like it's time to leave.
                match room.members(RoomMemberships::JOIN).instrument(event_span.clone()).await {
                    Ok(members) => {
                        {
                            let _enter = event_span.enter();

                            tracing::info!(
                                count = members.len(),
                                "Determined room members count",
                            );

                            if members.len() == 1 {
                                return;
                            }
                        }

                        tokio::spawn(async move {
                            if let Err(err) = callback(ev, room).await {
                                tracing::error!(?err, "Error in callback");
                            }
                        });
                    }
                    Err(err) => {
                        let _enter = event_span.enter();
                        tracing::error!(?err, "Failed to get members");
                    }
                }
            },
        );
    }
}
