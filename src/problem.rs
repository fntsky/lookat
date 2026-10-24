use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub struct Problem {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub field: Option<String>,
}
impl Problem {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            field: None,
        }
    }
    pub fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        let mut error = Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_payload", message);
        error.field = Some(field.into());
        error
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }
    pub fn internal(error: impl std::fmt::Display) -> Self {
        eprintln!("lookat storage: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "无法读写统计数据",
        )
    }
}
impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Problem {}
impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        (self.status,Json(serde_json::json!({"error":{"code":self.code,"message":self.message,"field":self.field}}))).into_response()
    }
}
impl From<rusqlite::Error> for Problem {
    fn from(error: rusqlite::Error) -> Self {
        Self::internal(error)
    }
}
pub type Result<T> = std::result::Result<T, Problem>;
