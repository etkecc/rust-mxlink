use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

use tracing::Instrument;

use matrix_sdk::Room;

use crate::MatrixLink;

use super::Rooms;

// This needs to be smaller than the typing notice duration that `room.typing_notice()` uses (4 seconds).
// `room.typing_notice()` keeps track of the last time it was called and will not send a new typing notice request if called too often.
const TYPING_NOTICE_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

pub struct TypingNoticeGuard {
    rooms: Rooms,
    room: Room,
}

impl Drop for TypingNoticeGuard {
    fn drop(&mut self) {
        let room = self.room.clone();
        let rooms = self.rooms.clone();

        let span = tracing::trace_span!("drop_typing_notice_guard", room_id = %room.room_id());

        tokio::spawn(
            async move {
                tracing::trace!("Doing stop-typing-notice work");

                let room_id = room.room_id().to_owned();

                let mut typing_notifications = rooms.matrix_link.inner.typing_notices.lock().await;

                let is_last_one = if let Some(counter) = typing_notifications.get(&room_id) {
                    let mut count = counter.lock().await;
                    *count = count.saturating_sub(1);

                    tracing::trace!(count = *count, "Remaining subscribers count");

                    *count == 0
                } else {
                    tracing::trace!(
                        "Not aware of typing notification loop for room.. Nothing to do."
                    );
                    false
                };

                if is_last_one {
                    tracing::trace!("Last one out, turning off typing notice..");

                    if let Err(err) = room.typing_notice(false).await {
                        tracing::error!(?err, "Failed to turn off typing notice");
                    }

                    typing_notifications.remove(&room_id);
                }
            }
            .instrument(span),
        );
    }
}

pub(super) async fn start_typing_notice(matrix_link: MatrixLink, room: &Room) -> TypingNoticeGuard {
    let mut typing_notifications = matrix_link.inner.typing_notices.lock().await;

    let room_subscribers_counter = typing_notifications
        .entry(room.room_id().to_owned())
        .or_insert_with(|| Arc::new(Mutex::new(0)));

    let mut room_subscribers_count = room_subscribers_counter.lock().await;
    *room_subscribers_count += 1;

    // Only the first subscriber will trigger the typing notice task.
    // The task will run as long as there's at least one subscriber.
    if *room_subscribers_count == 1 {
        let span = tracing::trace_span!("typing_notice", room_id = %room.room_id());

        let room_clone = room.clone();
        let room_subscribers_count_clone = room_subscribers_counter.clone();

        tokio::spawn(
            async move {
                let mut interval = interval(TYPING_NOTICE_REFRESH_INTERVAL);

                loop {
                    tracing::trace!("Sending typing notice..");

                    if let Err(err) = room_clone.typing_notice(true).await {
                        tracing::warn!(?err, "Failed to send typing notice");
                    }

                    interval.tick().await;

                    let count = room_subscribers_count_clone.lock().await;
                    if *count == 0 {
                        tracing::trace!("0 subscribers remain, stopping typing notice loop");
                        break;
                    }
                }
            }
            .instrument(span),
        );
    } else {
        tracing::trace!("Not starting typing notice loop as it's already started");
    }

    TypingNoticeGuard {
        rooms: matrix_link.rooms(),
        room: room.clone(),
    }
}
