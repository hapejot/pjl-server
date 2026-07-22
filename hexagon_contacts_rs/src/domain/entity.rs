use serde::{Deserialize, Serialize};

/// Domain entity - no dependency on axum, tokio-postgres, or any framework.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub id: Option<i32>,
    pub customer_id: String,
    pub name: String,
    pub email: String,
    pub phone: String,
}
