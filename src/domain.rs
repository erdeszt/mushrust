#[derive(Debug, sqlx::FromRow)]
pub struct Measurement {
    pub at: String,
    pub temperature: f32,
    pub humidity: f32,
}
