use serde::Serialize;

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct Measurement {
    pub at: String,
    pub temperature: f32,
    pub humidity: f32,
}
