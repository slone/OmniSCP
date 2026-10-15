use serde::{Deserialize, Serialize};

use crate::protocols::ProtocolType;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthMethod {
    Password,
    KeyFile { path: String },
    KeyWithPassphrase { path: String },
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub protocol: ProtocolType,
    pub username: String,
    pub auth_method: AuthMethod,
    pub remote_directory: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub language: String,
    pub theme: String,
    pub concurrent_transfers: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: "fr".to_string(),
            theme: "system".to_string(),
            concurrent_transfers: 2,
        }
    }
}
