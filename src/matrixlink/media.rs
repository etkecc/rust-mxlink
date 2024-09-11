use matrix_sdk::{
    ruma::events::room::{
        message::{
            AudioInfo, AudioMessageEventContent, FileInfo, FileMessageEventContent,
            ImageMessageEventContent, MessageType, RoomMessageEventContent,
            UnstableVoiceContentBlock, VideoInfo, VideoMessageEventContent,
        },
        ImageInfo,
    },
    Room,
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum MediaAttachmentUploadPrepError {
    #[error("Error getting encryption status: {0}")]
    EncryptionStatusUnknown(matrix_sdk::Error),

    #[error("Error during unencrypted upload: {0}")]
    UnencryptedUpload(matrix_sdk::HttpError),

    #[error("Error during encrypted upload: {0}")]
    EncryptedUpload(matrix_sdk::Error),
}

#[derive(Clone)]
pub struct Media {}

impl Default for Media {
    fn default() -> Self {
        Self::new()
    }
}

impl Media {
    pub fn new() -> Self {
        Self {}
    }

    /// This is similar to `Room::send_attachment()`, but only does the upload and preparation part, without automatically sending the attachment.
    pub async fn upload_and_prepare_event_content(
        &self,
        room: &Room,
        content_type: &mime::Mime,
        data: Vec<u8>,
        attachment_body_text: &str,
    ) -> Result<RoomMessageEventContent, MediaAttachmentUploadPrepError> {
        let bytes = data.clone();

        let message_type = upload_and_prepare_attachment_message(
            room,
            content_type,
            bytes,
            attachment_body_text.to_owned(),
        )
        .await?;

        Ok(RoomMessageEventContent::new(message_type))
    }
}

/// Uploads the given file (encrypted or unencrypted, depending on the room) and prepares the message payload for it.
pub async fn upload_and_prepare_attachment_message(
    room: &matrix_sdk::Room,
    content_type: &mime::Mime,
    data: Vec<u8>,
    attachment_body: String,
) -> Result<MessageType, MediaAttachmentUploadPrepError> {
    let is_encrypted = room
        .is_encrypted()
        .await
        .map_err(MediaAttachmentUploadPrepError::EncryptionStatusUnknown)?;

    if is_encrypted {
        upload_and_prepare_attachment_message_encrypted(
            room.client(),
            content_type,
            data,
            attachment_body,
        )
        .await
    } else {
        upload_and_prepare_attachment_message_unencrypted(
            room.client(),
            content_type,
            data,
            attachment_body,
        )
        .await
    }
}

/// Uploads the given file as unencrypted media and prepares the message payload for it.
/// This is like `Media::prepare_attachment_message()`
async fn upload_and_prepare_attachment_message_unencrypted(
    client: matrix_sdk::Client,
    content_type: &mime::Mime,
    data: Vec<u8>,
    attachment_body: String,
) -> Result<MessageType, MediaAttachmentUploadPrepError> {
    let data_size = data.len();

    let response = client
        .media()
        .upload(content_type, data)
        .await
        .map_err(MediaAttachmentUploadPrepError::UnencryptedUpload)?;

    let url = response.content_uri;

    Ok(match content_type.type_() {
        mime::IMAGE => {
            let mut image_event_content = ImageMessageEventContent::plain(attachment_body, url);

            image_event_content =
                inject_info_into_image_content(image_event_content, content_type, data_size);

            MessageType::Image(image_event_content)
        }
        mime::AUDIO => {
            let mut audio_message_event_content =
                AudioMessageEventContent::plain(attachment_body, url);

            audio_message_event_content = inject_info_into_audio_content(
                audio_message_event_content,
                content_type,
                data_size,
            );

            MessageType::Audio(audio_message_event_content)
        }
        mime::VIDEO => {
            let mut video_message_event_content =
                VideoMessageEventContent::plain(attachment_body, url);

            video_message_event_content = inject_info_into_video_content(
                video_message_event_content,
                content_type,
                data_size,
            );

            MessageType::Video(video_message_event_content)
        }
        _ => {
            let mut file_message_event_content =
                FileMessageEventContent::plain(attachment_body, url);

            file_message_event_content =
                inject_info_into_file_content(file_message_event_content, content_type, data_size);

            MessageType::File(file_message_event_content)
        }
    })
}

/// Uploads the given file as encrypted media and prepares the message payload for it.
/// This is like `Client::prepare_encrypted_attachment_message()`
async fn upload_and_prepare_attachment_message_encrypted(
    client: matrix_sdk::Client,
    content_type: &mime::Mime,
    data: Vec<u8>,
    attachment_body: String,
) -> Result<MessageType, MediaAttachmentUploadPrepError> {
    let data_size = data.len();

    let mut cursor = std::io::Cursor::new(data);

    let file = client
        .prepare_encrypted_file(content_type, &mut cursor)
        .await
        .map_err(MediaAttachmentUploadPrepError::EncryptedUpload)?;

    Ok(match content_type.type_() {
        mime::IMAGE => {
            let mut image_event_content =
                ImageMessageEventContent::encrypted(attachment_body, file);

            image_event_content =
                inject_info_into_image_content(image_event_content, content_type, data_size);

            MessageType::Image(image_event_content)
        }
        mime::AUDIO => {
            let mut audio_message_event_content =
                AudioMessageEventContent::encrypted(attachment_body, file);

            audio_message_event_content = inject_info_into_audio_content(
                audio_message_event_content,
                content_type,
                data_size,
            );

            MessageType::Audio(audio_message_event_content)
        }
        mime::VIDEO => {
            let mut video_message_event_content =
                VideoMessageEventContent::encrypted(attachment_body, file);

            video_message_event_content = inject_info_into_video_content(
                video_message_event_content,
                content_type,
                data_size,
            );

            MessageType::Video(video_message_event_content)
        }
        _ => {
            let mut file_message_event_content =
                FileMessageEventContent::encrypted(attachment_body, file);

            file_message_event_content =
                inject_info_into_file_content(file_message_event_content, content_type, data_size);

            MessageType::File(file_message_event_content)
        }
    })
}

fn inject_info_into_image_content(
    content: ImageMessageEventContent,
    content_type: &mime::Mime,
    size: usize,
) -> ImageMessageEventContent {
    let mut info = ImageInfo::new();

    info.mimetype = Some(content_type.as_ref().to_owned());
    info.size = js_int::UInt::new(size as u64);

    content.info(Box::new(info))
}

fn inject_info_into_audio_content(
    content: AudioMessageEventContent,
    content_type: &mime::Mime,
    size: usize,
) -> AudioMessageEventContent {
    let mut content = content.clone();

    if content_type.as_ref() == "audio/ogg" {
        // Audio messages backed by OGG files are eligible for being treated as voice messages,
        // as per MSC3245: https://github.com/matrix-org/matrix-spec-proposals/blob/83f6c5b469c1d78f714e335dcaa25354b255ffa5/proposals/3245-voice-messages.md
        //
        // We can't be sure that our caller wishes to add a "voice message marker" to this specific audio message,
        // but it seems like a reasonable assumption that allows us to magically do it, without complicating our API.
        //
        // Without this "voice message marker", the audio message would be treated as a regular attachment by Element
        // (especially Element X iOS, as of 2024-09-06),
        // which is not what we want.
        content.voice = Some(UnstableVoiceContentBlock::new());
    }

    let mut info = AudioInfo::new();

    info.mimetype = Some(content_type.as_ref().to_owned());
    info.size = js_int::UInt::new(size as u64);

    content.info(Box::new(info))
}

fn inject_info_into_video_content(
    content: VideoMessageEventContent,
    content_type: &mime::Mime,
    size: usize,
) -> VideoMessageEventContent {
    let mut info = VideoInfo::new();

    info.mimetype = Some(content_type.as_ref().to_owned());
    info.size = js_int::UInt::new(size as u64);

    content.info(Box::new(info))
}

fn inject_info_into_file_content(
    content: FileMessageEventContent,
    content_type: &mime::Mime,
    size: usize,
) -> FileMessageEventContent {
    let mut info = FileInfo::new();

    info.mimetype = Some(content_type.as_ref().to_owned());
    info.size = js_int::UInt::new(size as u64);

    content.info(Box::new(info))
}
