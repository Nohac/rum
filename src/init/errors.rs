use crate::error::RumError;

pub(super) fn map_inquire_err(e: inquire::InquireError) -> RumError {
    match e {
        inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
            RumError::InitCancelled
        }
        other => RumError::Validation {
            message: format!("prompt error: {other}"),
        },
    }
}
