// Scaffolding for the tamper-evident digital signature feature; the command
// surface and types are fixed here, the crypto is filled in next.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NewIdRequest {
    pub name: String,
    pub email: String,
    pub org: String,
    pub password: String,
    pub out_path: String,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IdInfo {
    pub name: String,
    pub email: String,
    pub org: String,
    pub issuer: String,
    /// RFC 3339
    pub not_after: String,
    pub self_signed: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SignRequest {
    pub src_path: String,
    pub out_path: String,
    pub p12_path: String,
    pub password: String,
    /// 0-based page index; with `rect` and `appearance_png_b64` makes the signature visible.
    pub page: Option<u32>,
    /// x1, y1, x2, y2 in that page's PDF user space.
    pub rect: Option<[f64; 4]>,
    pub appearance_png_b64: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SigInfo {
    pub field: String,
    pub signer: String,
    pub email: Option<String>,
    pub issuer: String,
    /// RFC 3339
    pub signed_at: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    /// Signature value and message digest over its ByteRange both check out.
    pub intact: bool,
    /// The signed byte range reaches the end of the file: nothing was appended after it.
    pub covers_whole: bool,
    pub self_signed: bool,
    pub page: Option<u32>,
    pub error: Option<String>,
}

pub fn create_id(req: &NewIdRequest) -> Result<IdInfo, String> {
    let _ = req;
    Err("not implemented".into())
}

pub fn id_info(p12_path: &str, password: &str) -> Result<IdInfo, String> {
    let _ = (p12_path, password);
    Err("not implemented".into())
}

pub fn sign_file(req: &SignRequest) -> Result<(), String> {
    let _ = req;
    Err("not implemented".into())
}

pub fn verify_file(path: &str) -> Result<Vec<SigInfo>, String> {
    let _ = path;
    Ok(Vec::new())
}
