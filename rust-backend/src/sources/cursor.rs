use crate::sources::{cursor_api, cursorpp, LocalSession, SourceError};

/// Unified Cursor source: Cursor++ log tokens plus Cursor dashboard session API billing.
pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let mut sessions = cursorpp::load_sessions()?;
    match cursor_api::load_sessions() {
        Ok(api_sessions) => sessions.extend(api_sessions),
        Err(err) => {
            tracing::debug!(error = %err, "cursor dashboard API unavailable; using Cursor++ logs only");
        }
    }
    Ok(sessions)
}