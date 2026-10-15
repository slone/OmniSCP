use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolType {
    Sftp,
    Scp,
    Ftp,
    Ftps,
    WebDav,
    WebDavs,
}

impl ProtocolType {
    pub fn default_port(self) -> u16 {
        match self {
            ProtocolType::Sftp | ProtocolType::Scp => 22,
            ProtocolType::Ftp => 21,
            ProtocolType::Ftps => 990,
            ProtocolType::WebDav => 80,
            ProtocolType::WebDavs => 443,
        }
    }

    pub fn is_encrypted(self) -> bool {
        matches!(
            self,
            ProtocolType::Sftp
                | ProtocolType::Scp
                | ProtocolType::Ftps
                | ProtocolType::WebDavs
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<i64>,
    pub permissions: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
}

pub trait Protocol: Send + Sync {
    fn connect(&mut self) -> Result<()>;
    fn disconnect(&mut self) -> Result<()>;
    fn is_connected(&self) -> bool;
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>>;
    fn upload(&self, local: &str, remote: &str) -> Result<()>;
    fn download(&self, remote: &str, local: &str) -> Result<()>;
    fn mkdir(&self, path: &str) -> Result<()>;
    fn rename(&self, from: &str, to: &str) -> Result<()>;
    fn delete(&self, path: &str) -> Result<()>;
    fn stat(&self, path: &str) -> Result<FileEntry>;
}
