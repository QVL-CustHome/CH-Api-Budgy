use crate::api::error::ApiError;
use crate::domain::ports::lecture::LectureError;

impl From<LectureError> for ApiError {
    fn from(error: LectureError) -> Self {
        match error {
            LectureError::Acces(message) => ApiError::internal(message),
        }
    }
}
