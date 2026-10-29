// Password protection: AES-256 (PDF 2.0, /V 5 /R 6) through lopdf's standard
// security handler. Removal needs a password the user knows; there is no
// bypass for files whose password is unknown.
use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::{dictionary, Document, EncryptionState, EncryptionVersion, Object};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

type R<T> = Result<T, String>;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    pub print: bool,
    pub copy: bool,
    pub modify: bool,
    pub annotate: bool,
}

// Owner-chosen anti-guessing policy, stored unencrypted in the catalog as
// /WumpaPolicy so this app can honour it before the file is opened. Other
// viewers ignore it; it only ever acts on the local copy.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub max_attempts: u32,
    // Overwrite and delete the local file once the attempts run out.
    pub wipe: bool,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SecurityInfo {
    pub encrypted: bool,
    // The empty user password does not open the file.
    pub needs_password: bool,
    // "AES-256", "AES-128", "RC4", or "" when not encrypted.
    pub method: String,
    pub permissions: Permissions,
    pub policy: Option<Policy>,
}

#[derive(Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AttemptStatus {
    pub failures: u32,
    // Attempts left before the policy acts; None when the file has no policy.
    pub remaining: Option<u32>,
    // Seconds until the next attempt is accepted.
    pub locked_secs: u64,
    pub wipe: bool,
    // The file was destroyed by this call.
    pub wiped: bool,
}

const P_PRINT: u64 = 1 << 2;
const P_MODIFY: u64 = 1 << 3;
const P_COPY: u64 = 1 << 4;
const P_ANNOTATE: u64 = 1 << 5;
const P_FILL: u64 = 1 << 8;
const P_ACCESS: u64 = 1 << 9;
const P_ASSEMBLE: u64 = 1 << 10;
const P_PRINT_HQ: u64 = 1 << 11;

fn to_lopdf(p: Permissions) -> lopdf::Permissions {
    let mut bits = P_ACCESS;
    if p.print {
        bits |= P_PRINT | P_PRINT_HQ;
    }
    if p.copy {
        bits |= P_COPY;
    }
    if p.modify {
        bits |= P_MODIFY | P_ASSEMBLE;
    }
    if p.annotate {
        bits |= P_ANNOTATE | P_FILL;
    }
    lopdf::Permissions::from_bits_retain(bits)
}

fn from_p(p: i64) -> Permissions {
    let bits = p as u64;
    Permissions {
        print: bits & P_PRINT != 0,
        copy: bits & P_COPY != 0,
        modify: bits & P_MODIFY != 0,
        annotate: bits & P_ANNOTATE != 0,
    }
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn encrypt_file(
    src: &str,
    out: &str,
    user_password: &str,
    owner_password: &str,
    perms: Permissions,
    policy: Option<Policy>,
) -> R<()> {
    if user_password.is_empty() && owner_password.is_empty() {
        return Err("a password is required".into());
    }
    let bytes = std::fs::read(src).map_err(err)?;
    let mut doc = Document::load_mem(&bytes).map_err(err)?;
    if doc.is_encrypted() {
        return Err("file is already protected; remove its password first".into());
    }
    // Acrobat's behaviour: an empty permissions password falls back to the open password.
    let owner = if owner_password.is_empty() {
        user_password
    } else {
        owner_password
    };
    if let Some(p) = policy {
        let catalog_id = crate::common::catalog_id(&doc)?;
        let catalog = doc
            .get_object_mut(catalog_id)
            .and_then(|o| o.as_dict_mut())
            .map_err(err)?;
        catalog.set(
            "WumpaPolicy",
            dictionary! { "MaxAttempts" => p.max_attempts.max(1) as i64, "Wipe" => p.wipe },
        );
    }

    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(err)?;
    let mut filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>> = BTreeMap::new();
    filters.insert(b"StdCF".to_vec(), Arc::new(Aes256CryptFilter));
    let state = EncryptionState::try_from(EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: filters,
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: owner,
        user_password,
        permissions: to_lopdf(perms),
    })
    .map_err(err)?;
    doc.encrypt(&state).map_err(err)?;
    doc.save(out).map_err(err)?;
    Ok(())
}

pub fn info(path: &str) -> R<SecurityInfo> {
    let bytes = std::fs::read(path).map_err(err)?;
    let doc = Document::load_mem(&bytes).map_err(err)?;
    let policy = read_policy(&doc).or_else(|| read_policy_raw(&bytes));
    let enc = match doc.get_encrypted() {
        Ok(d) => d.clone(),
        Err(_) => {
            if let Some(state) = doc.encryption_state.as_ref() {
                // Loaded and decrypted with the empty user password.
                let v = state.version();
                return Ok(SecurityInfo {
                    encrypted: true,
                    needs_password: false,
                    method: method_name(v, state.key_length(), None),
                    permissions: from_p(state.permissions().bits() as i64),
                    policy,
                });
            }
            return Ok(SecurityInfo {
                encrypted: false,
                needs_password: false,
                method: String::new(),
                permissions: Permissions {
                    print: true,
                    copy: true,
                    modify: true,
                    annotate: true,
                },
                policy: None,
            });
        }
    };
    let v = enc.get(b"V").and_then(Object::as_i64).unwrap_or(0);
    let length = enc
        .get(b"Length")
        .and_then(Object::as_i64)
        .ok()
        .map(|l| l as usize);
    let cfm = enc
        .get(b"CF")
        .and_then(Object::as_dict)
        .and_then(|cf| cf.get(b"StdCF"))
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"CFM"))
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n).to_string())
        .ok();
    let p = enc.get(b"P").and_then(Object::as_i64).unwrap_or(-1);
    Ok(SecurityInfo {
        encrypted: true,
        needs_password: doc.authenticate_user_password("").is_err(),
        method: method_name(v, length, cfm.as_deref()),
        permissions: from_p(p),
        policy,
    })
}

// Without the password lopdf only yields the /Encrypt dictionary, so the
// (unencrypted) policy is also recovered from the raw bytes.
fn read_policy_raw(bytes: &[u8]) -> Option<Policy> {
    let at = bytes.windows(12).position(|w| w == b"/WumpaPolicy")?;
    let window = &bytes[at..(at + 120).min(bytes.len())];
    let text = String::from_utf8_lossy(window);
    let end = text.find(">>")?;
    let body = &text[..end];
    let num = |key: &str| -> Option<i64> {
        let i = body.find(key)? + key.len();
        let digits: String = body[i..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        digits.parse().ok()
    };
    let wipe = body
        .find("/Wipe")
        .map(|i| body[i + 5..].trim_start().starts_with("true"))
        .unwrap_or(false);
    Some(Policy {
        max_attempts: num("/MaxAttempts").unwrap_or(10).max(1) as u32,
        wipe,
    })
}

fn read_policy(doc: &Document) -> Option<Policy> {
    let root = crate::common::catalog_id(doc).ok()?;
    let d = doc
        .get_object(root)
        .ok()?
        .as_dict()
        .ok()?
        .get(b"WumpaPolicy")
        .ok()?
        .as_dict()
        .ok()?;
    Some(Policy {
        max_attempts: d
            .get(b"MaxAttempts")
            .and_then(Object::as_i64)
            .unwrap_or(10)
            .max(1) as u32,
        wipe: d.get(b"Wipe").and_then(Object::as_bool).unwrap_or(false),
    })
}

// ---------- password attempt ledger ----------
//
// Guessing is slowed down inside this app with growing lockouts, keyed by the
// file's content hash so renaming or copying does not reset the count. The
// ledger lives in the app data directory, not in the file.

#[derive(Serialize, Deserialize, Default, Clone)]
struct Entry {
    failures: u32,
    last_fail: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_key(path: &str) -> R<String> {
    let bytes = std::fs::read(path).map_err(err)?;
    Ok(format!("{:x}", sha2::Sha256::digest(&bytes)))
}

fn ledger_path(dir: &Path) -> PathBuf {
    dir.join("password-attempts.json")
}

fn load_ledger(dir: &Path) -> BTreeMap<String, Entry> {
    std::fs::read(ledger_path(dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_ledger(dir: &Path, ledger: &BTreeMap<String, Entry>) -> R<()> {
    std::fs::create_dir_all(dir).map_err(err)?;
    let json = serde_json::to_vec(ledger).map_err(err)?;
    std::fs::write(ledger_path(dir), json).map_err(err)
}

// Lockout after `failures` wrong passwords, in seconds.
fn lock_for(failures: u32) -> u64 {
    match failures {
        0..=2 => 0,
        3..=4 => 10,
        5..=6 => 60,
        7..=8 => 5 * 60,
        9 => 15 * 60,
        _ => 60 * 60,
    }
}

fn status_of(entry: &Entry, policy: Option<Policy>) -> AttemptStatus {
    let elapsed = now_unix().saturating_sub(entry.last_fail);
    let lock = lock_for(entry.failures);
    AttemptStatus {
        failures: entry.failures,
        remaining: policy.map(|p| p.max_attempts.saturating_sub(entry.failures)),
        locked_secs: lock.saturating_sub(elapsed),
        wipe: policy.map(|p| p.wipe).unwrap_or(false),
        wiped: false,
    }
}

pub fn attempt_status(dir: &Path, path: &str) -> R<AttemptStatus> {
    let key = file_key(path)?;
    let policy = info(path).ok().and_then(|i| i.policy);
    let ledger = load_ledger(dir);
    Ok(status_of(
        ledger
            .get(&key)
            .cloned()
            .as_ref()
            .unwrap_or(&Entry::default()),
        policy,
    ))
}

// Overwrite the file with random bytes, flush, then delete it.
fn wipe_file(path: &str) -> R<()> {
    use std::io::Write;
    let len = std::fs::metadata(path).map_err(err)?.len();
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(err)?;
        let mut left = len;
        let mut buf = vec![0u8; 64 * 1024];
        while left > 0 {
            let n = left.min(buf.len() as u64) as usize;
            getrandom::fill(&mut buf[..n]).map_err(err)?;
            f.write_all(&buf[..n]).map_err(err)?;
            left -= n as u64;
        }
        f.sync_all().map_err(err)?;
    }
    std::fs::remove_file(path).map_err(err)
}

pub fn record_attempt(dir: &Path, path: &str, ok: bool) -> R<AttemptStatus> {
    let key = file_key(path)?;
    let policy = info(path).ok().and_then(|i| i.policy);
    let mut ledger = load_ledger(dir);
    if ok {
        ledger.remove(&key);
        save_ledger(dir, &ledger)?;
        return Ok(AttemptStatus {
            remaining: policy.map(|p| p.max_attempts),
            wipe: policy.map(|p| p.wipe).unwrap_or(false),
            ..Default::default()
        });
    }
    let entry = ledger.entry(key.clone()).or_default();
    entry.failures += 1;
    entry.last_fail = now_unix();
    let mut st = status_of(entry, policy);
    save_ledger(dir, &ledger)?;
    if let Some(p) = policy {
        if p.wipe && st.failures >= p.max_attempts {
            wipe_file(path)?;
            ledger.remove(&key);
            save_ledger(dir, &ledger)?;
            st.wiped = true;
            st.remaining = Some(0);
        }
    }
    Ok(st)
}

fn method_name(v: i64, length: Option<usize>, cfm: Option<&str>) -> String {
    match v {
        5 => "AES-256".into(),
        4 => {
            if cfm == Some("AESV2") {
                "AES-128".into()
            } else {
                "RC4".into()
            }
        }
        _ => format!("RC4-{}", length.unwrap_or(40)),
    }
}

pub fn remove_file(src: &str, out: &str, password: &str) -> R<()> {
    let bytes = std::fs::read(src).map_err(err)?;
    let plain = Document::load_mem(&bytes).map_err(err)?;
    if !plain.is_encrypted() && plain.encryption_state.is_none() {
        return Err("file is not protected".into());
    }
    // The owner password is what authorises dropping the security handler;
    // the user password alone only opens the file for reading.
    if plain.is_encrypted() && plain.authenticate_owner_password(password).is_err() {
        return Err(if plain.authenticate_user_password(password).is_ok() {
            "the permissions password is required to remove protection".into()
        } else {
            "wrong password".into()
        });
    }
    if !plain.is_encrypted() && !owner_password_matches(&plain, password) {
        return Err("wrong password".into());
    }
    let mut doc =
        Document::load_mem_with_options(&bytes, lopdf::LoadOptions::with_password(password))
            .map_err(|_| "wrong password".to_string())?;
    if doc.is_encrypted() {
        return Err("wrong password".into());
    }
    doc.trailer.remove(b"Encrypt");
    doc.encryption_state = None;
    doc.save(out).map_err(err)?;
    Ok(())
}

// lopdf auto-decrypts files whose user password is empty and drops their
// /Encrypt dictionary, so the owner password is checked against a shell
// document rebuilt from the recorded encryption state.
fn owner_password_matches(plain: &Document, password: &str) -> bool {
    let Some(state) = plain.encryption_state.as_ref() else {
        return false;
    };
    let Ok(enc) = state.encode() else {
        return false;
    };
    let mut shell = Document::new();
    let id = shell.add_object(Object::Dictionary(enc));
    shell.trailer.set("Encrypt", Object::Reference(id));
    if let Ok(ids) = plain.trailer.get(b"ID") {
        shell.trailer.set("ID", ids.clone());
    }
    shell.authenticate_owner_password(password).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::blank_document;

    fn tmp(name: &str) -> String {
        let dir = std::env::temp_dir().join("wumpa-security-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name).to_string_lossy().to_string()
    }

    #[test]
    fn encrypt_then_info_then_remove() {
        let src = tmp("plain.pdf");
        let enc = tmp("enc.pdf");
        let dec = tmp("dec.pdf");
        blank_document(200.0, 200.0, 1).save(&src).unwrap();

        let perms = Permissions {
            print: true,
            copy: false,
            modify: false,
            annotate: true,
        };
        encrypt_file(&src, &enc, "open", "owner", perms, None).unwrap();

        let i = info(&enc).unwrap();
        assert!(i.encrypted);
        assert!(i.needs_password);
        assert_eq!(i.method, "AES-256");

        assert_eq!(
            remove_file(&enc, &dec, "nope").unwrap_err(),
            "wrong password"
        );
        assert!(remove_file(&enc, &dec, "open").is_err());
        remove_file(&enc, &dec, "owner").unwrap();
        let after = info(&dec).unwrap();
        assert!(!after.encrypted);
        let doc = Document::load(&dec).unwrap();
        assert_eq!(doc.get_pages().len(), 1);
    }

    #[test]
    fn attempts_lock_and_policy_wipes() {
        let src = tmp("plain3.pdf");
        let enc = tmp("enc3.pdf");
        let dir = std::env::temp_dir()
            .join("wumpa-security-tests")
            .join("ledger");
        let _ = std::fs::remove_dir_all(&dir);
        blank_document(200.0, 200.0, 1).save(&src).unwrap();
        let policy = Policy {
            max_attempts: 3,
            wipe: true,
        };
        encrypt_file(&src, &enc, "open", "", Permissions::default(), Some(policy)).unwrap();
        let i = info(&enc).unwrap();
        assert_eq!(i.policy, Some(policy));

        let st = attempt_status(&dir, &enc).unwrap();
        assert_eq!(st.remaining, Some(3));
        assert_eq!(st.locked_secs, 0);
        let st = record_attempt(&dir, &enc, false).unwrap();
        assert_eq!((st.failures, st.remaining, st.wiped), (1, Some(2), false));
        // A correct password resets the count.
        let st = record_attempt(&dir, &enc, true).unwrap();
        assert_eq!(st.failures, 0);
        record_attempt(&dir, &enc, false).unwrap();
        record_attempt(&dir, &enc, false).unwrap();
        let st = record_attempt(&dir, &enc, false).unwrap();
        assert!(st.wiped);
        assert!(!Path::new(&enc).exists(), "file should have been destroyed");

        // Without a policy the count keeps growing and locks out, but never wipes.
        let enc2 = tmp("enc4.pdf");
        encrypt_file(&src, &enc2, "open", "", Permissions::default(), None).unwrap();
        for _ in 0..3 {
            record_attempt(&dir, &enc2, false).unwrap();
        }
        let st = attempt_status(&dir, &enc2).unwrap();
        assert_eq!(st.remaining, None);
        assert!(st.locked_secs > 0 && st.locked_secs <= 10);
        assert!(Path::new(&enc2).exists());
    }

    #[test]
    fn owner_only_password_keeps_file_openable() {
        let src = tmp("plain2.pdf");
        let enc = tmp("enc2.pdf");
        blank_document(200.0, 200.0, 1).save(&src).unwrap();
        let perms = Permissions {
            print: false,
            copy: false,
            modify: false,
            annotate: false,
        };
        encrypt_file(&src, &enc, "", "owner", perms, None).unwrap();
        let i = info(&enc).unwrap();
        assert!(i.encrypted);
        assert!(!i.needs_password);
        assert!(!i.permissions.print);
        assert!(!i.permissions.copy);

        let dec = tmp("dec2.pdf");
        assert_eq!(
            remove_file(&enc, &dec, "nope").unwrap_err(),
            "wrong password"
        );
        remove_file(&enc, &dec, "owner").unwrap();
        assert!(!info(&dec).unwrap().encrypted);
        assert!(
            crate::common::load_plain(&enc).is_err(),
            "rewrites must refuse protected files"
        );
    }
}
