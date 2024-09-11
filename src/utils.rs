use matrix_sdk::ruma::api::client::error::ErrorKind;
use matrix_sdk::{Error, HttpError};

pub fn is_potentially_transient_sdk_error(err: &Error) -> bool {
    if let matrix_sdk::Error::Http(err) = &err {
        return is_potentially_transient_http_error(err);
    }

    true
}

pub fn is_potentially_transient_http_error(err: &HttpError) -> bool {
    if let Some(ErrorKind::UnknownToken { soft_logout: _ }) = err.client_api_error_kind() {
        // This is a permanent error, so we should not retry.
        return false;
    }

    true
}
