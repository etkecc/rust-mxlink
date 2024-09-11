use mxlink::helpers::encryption::EncryptionKey;
use mxlink::{InvitationDecision, MessageResponseType};
use mxlink::{InitConfig, LoginConfig, LoginCredentials, LoginEncryption, MatrixLink, PersistenceConfig};

// You can run this example either by modifying the configuration below,
// or against the Synapse server provided by baibot (https://github.com/etkecc/baibot/) - see its docs/development.md guide.

const HOMESERVER_URL: &str = "http://synapse.127.0.0.1.nip.io:42020";

const LOGIN_USERNAME: &str = "baibot";
const LOGIN_PASSWORD: &str = "baibot";
const LOGIN_ENCRYPTION_RECOVERY_PASSPHRASE: &str = "long-and-secure-passphrase-here";
const LOGIN_ENCRYPTION_RESET_ALLOWED: bool = false;

const DEVICE_DISPLAY_NAME: &str = LOGIN_USERNAME;

const PERSISTENCE_SESSION_FILE_PATH: &str = "/tmp/mxlink-session.json";
const PERSISTENCE_SESSION_ENCRYPTION_KEY: &str =
    "ef4d037845e5591c6627122112b7f30b2154e7354f928ff75e4dda206ba84338";
const PERSISTENCE_DB_DIR_PATH: &str = "/tmp/mxlink-db";

#[tokio::main]
async fn main() {
    let matrix_link = create_matrix_link().await;

    // matrix_link can be cloned freely
    register_event_handlers(matrix_link.clone()).await;

    matrix_link
        .start()
        .await
        .expect("Failed to start MatrixLink");

    println!("Done");
}

async fn create_matrix_link() -> MatrixLink {
    let login_creds =
        LoginCredentials::UserPassword(LOGIN_USERNAME.to_owned(), LOGIN_PASSWORD.to_owned());

    let login_encryption = LoginEncryption::new(
        Some(LOGIN_ENCRYPTION_RECOVERY_PASSPHRASE.to_owned()),
        LOGIN_ENCRYPTION_RESET_ALLOWED,
    );

    let login_config = LoginConfig::new(
        HOMESERVER_URL.to_owned(),
        login_creds,
        Some(login_encryption),
        DEVICE_DISPLAY_NAME.to_owned(),
    );

    let session_file_path = std::path::PathBuf::from(PERSISTENCE_SESSION_FILE_PATH);
    let session_encryption_key = EncryptionKey::from_hex_str(PERSISTENCE_SESSION_ENCRYPTION_KEY)
        .expect("Invalid encryption key hex string");
    let db_dir_path = std::path::PathBuf::from(PERSISTENCE_DB_DIR_PATH);

    let persistence_config =
        PersistenceConfig::new(session_file_path, Some(session_encryption_key), db_dir_path);

    let init_config = InitConfig::new(login_config, persistence_config);

    mxlink::init(&init_config)
        .await
        .expect("Failed to initialize MatrixLink")
}

async fn register_event_handlers(matrix_link: MatrixLink) {
    let rooms = matrix_link.rooms();

    // We auto-accept all invitations
    rooms.on_invitation(|_event, _room| async move { Ok(InvitationDecision::Join) });

    // We send an introduction to all rooms we join
    let messaging = matrix_link.messaging();
    rooms.on_joined(|_event, room| async move {
        let _ = messaging
            .send_text_markdown(&room, "Hello!".to_owned(), MessageResponseType::InRoom)
            .await
            .expect("Failed to send message");

        Ok(())
    });

    // We listen for reactions to messages and send a reply to the original message that received the reaction
    let messaging = matrix_link.messaging();
    let reacting = matrix_link.reacting();
    reacting.on_actionable_reaction(|event, room, reaction_event_content| async move {
        let response_text = if reaction_event_content.relates_to.key == "👍️" {
            format!("{} reacted to this message with thumbs up!", event.sender())
        } else {
            format!(
                "{} reacted to this message with an unknown-to-me reaction ({}).",
                event.sender(),
                reaction_event_content.relates_to.key,
            )
        };

        let response_type =
            MessageResponseType::Reply(reaction_event_content.relates_to.event_id.clone());

        let _ = messaging
            .send_notice_markdown(&room, response_text, response_type)
            .await
            .expect("Failed to send message");

        Ok(())
    });
}
