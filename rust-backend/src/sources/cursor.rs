use crate::sources::{cursor_api, cursorpp, LocalSession, SourceError};

/// Unified Cursor source: Cursor++ local logs plus Cursor Dashboard API.
/// Both belong under the single product source `cursor`.
pub fn load_sessions(watermark_ms: Option<i64>) -> Result<Vec<LocalSession>, SourceError> {
    let mut sessions = cursorpp::load_sessions(watermark_ms)?;
    match cursor_api::load_sessions() {
        Ok(api_sessions) => sessions.extend(api_sessions),
        Err(err) => {
            tracing::debug!(error = %err, "cursor dashboard API unavailable; using Cursor++ logs only");
        }
    }
    Ok(sessions)
}
