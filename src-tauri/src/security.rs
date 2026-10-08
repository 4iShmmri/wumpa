// Scaffolding for password protection and permissions; wired next.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    pub print: bool,
    pub copy: bool,
    pub modify: bool,
    pub annotate: bool,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SecurityInfo {
    pub encrypted: bool,
    /// The empty user password does not open the file.
    pub needs_password: bool,
    /// "AES-256", "AES-128", "RC4", or "" when not encrypted.
    pub method: String,
    pub permissions: Permissions,
}

pub fn encrypt_file(
    src: &str,
    out: &str,
    user_password: &str,
    owner_password: &str,
    perms: Permissions,
) -> Result<(), String> {
    let _ = (src, out, user_password, owner_password, perms);
    Err("not implemented".into())
}

pub fn info(path: &str) -> Result<SecurityInfo, String> {
    let _ = path;
    Err("not implemented".into())
}

pub fn remove_file(src: &str, out: &str, password: &str) -> Result<(), String> {
    let _ = (src, out, password);
    Err("not implemented".into())
}
