pub mod credentials;

use serde::{Deserialize, Serialize};

use crate::protocols::ProtocolType;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
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
    /// The password or passphrase is kept in the OS credential store (see `credentials`).
    #[serde(default)]
    pub save_password: bool,
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
