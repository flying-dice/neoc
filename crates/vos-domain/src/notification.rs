use crate::AppId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub app: AppId,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
}
