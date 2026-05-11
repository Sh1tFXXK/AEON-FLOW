use crate::{Blob, DeviceInfo, Identity, CID};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedBlob {
    pub cid: CID,
    pub data: Vec<u8>,
    pub mime: String,
    pub created_by: [u8; 32],
    pub created_at: u64,
    pub device_id: [u8; 16],
    pub signature: Vec<u8>,
}

pub struct CIDStore {
    root: PathBuf,
    memory: HashMap<CID, Blob>,
}

impl CIDStore {
    pub fn new(root: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            memory: HashMap::new(),
        })
    }

    pub fn default_path() -> PathBuf {
        std::env::var_os("AEON_STORE_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".aeon").join("store"))
            })
            .unwrap_or_else(|| PathBuf::from(".aeon").join("store"))
    }

    pub fn put(&mut self, blob: Blob) -> io::Result<CID> {
        let cid = blob.cid;
        let path = self.blob_path(&cid);

        if !path.exists() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, encode_blob(&blob))?;
        }

        self.memory.insert(cid, blob);
        Ok(cid)
    }


    pub fn put_signed(
        &mut self,
        data: Vec<u8>,
        mime: &str,
        identity: &Identity,
        device: &DeviceInfo,
    ) -> io::Result<SignedBlob> {
        let cid = *blake3::hash(&data).as_bytes();
        let signature = identity.sign(&cid).to_bytes().to_vec();

        let blob = Blob {
            cid,
            data: data.clone(),
            mime: mime.to_string(),
        };
        self.put(blob)?;

        Ok(SignedBlob {
            cid,
            data,
            mime: mime.to_string(),
            created_by: identity.id,
            created_at: now_ms(),
            device_id: device.device_id,
            signature,
        })
    }

    pub fn verify_signed_blob(blob: &SignedBlob, identity: &Identity) -> bool {
        if blob.created_by != identity.id {
            return false;
        }
        let Ok(sig_bytes) = <[u8; 64]>::try_from(blob.signature.as_slice()) else {
            return false;
        };
        let sig = Signature::from_bytes(&sig_bytes);
        Identity::verify(&identity.public_key, &blob.cid, &sig)
    }

    pub fn get(&mut self, cid: &CID) -> io::Result<Option<Blob>> {
        if let Some(blob) = self.memory.get(cid) {
            return Ok(Some(blob.clone()));
        }

        let path = self.blob_path(cid);
        if !path.exists() {
            return Ok(None);
        }

        let buf = std::fs::read(path)?;
        let blob = decode_blob(*cid, &buf)?;
        self.memory.insert(*cid, blob.clone());
        Ok(Some(blob))
    }

    pub fn has(&self, cid: &CID) -> bool {
        self.memory.contains_key(cid) || self.blob_path(cid).exists()
    }

    pub fn list(&self) -> io::Result<Vec<CID>> {
        let mut cids = Vec::new();
        if !self.root.exists() {
            return Ok(cids);
        }

        for prefix_dir in std::fs::read_dir(&self.root)? {
            let prefix_dir = prefix_dir?;
            if !prefix_dir.file_type()?.is_dir() {
                continue;
            }

            for file in std::fs::read_dir(prefix_dir.path())? {
                let file = file?;
                if !file.file_type()?.is_file() {
                    continue;
                }

                let name = file.file_name();
                let hex = name.to_string_lossy();
                if let Ok(cid) = parse_cid_hex(&hex) {
                    cids.push(cid);
                }
            }
        }

        cids.sort();
        Ok(cids)
    }

    pub fn total_size_bytes(&self) -> io::Result<u64> {
        let mut total = 0;
        if !self.root.exists() {
            return Ok(total);
        }

        for prefix_dir in std::fs::read_dir(&self.root)? {
            let prefix_dir = prefix_dir?;
            if !prefix_dir.file_type()?.is_dir() {
                continue;
            }

            for file in std::fs::read_dir(prefix_dir.path())? {
                let file = file?;
                if file.file_type()?.is_file() {
                    total += file.metadata()?.len();
                }
            }
        }

        Ok(total)
    }

    fn blob_path(&self, cid: &CID) -> PathBuf {
        let hex = hex_cid(cid);
        self.root.join(&hex[..2]).join(hex)
    }
}

pub fn hex_cid(cid: &CID) -> String {
    let mut out = String::with_capacity(64);
    for byte in cid {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn parse_cid_hex(hex: &str) -> Result<CID, String> {
    if hex.len() != 64 {
        return Err("CID hex must be exactly 64 characters".to_string());
    }

    let mut cid = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
        let part = std::str::from_utf8(chunk).map_err(|e| e.to_string())?;
        cid[i] = u8::from_str_radix(part, 16).map_err(|e| e.to_string())?;
    }
    Ok(cid)
}

fn encode_blob(blob: &Blob) -> Vec<u8> {
    let mime_bytes = blob.mime.as_bytes();
    let mut buf = Vec::with_capacity(8 + mime_bytes.len() + blob.data.len());
    buf.extend_from_slice(&(mime_bytes.len() as u64).to_le_bytes());
    buf.extend_from_slice(mime_bytes);
    buf.extend_from_slice(&blob.data);
    buf
}

fn decode_blob(expected_cid: CID, buf: &[u8]) -> io::Result<Blob> {
    if buf.len() < 8 {
        return Err(invalid_data("blob header is truncated"));
    }

    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&buf[..8]);
    let mime_len = u64::from_le_bytes(len_bytes) as usize;
    let data_start = 8usize
        .checked_add(mime_len)
        .ok_or_else(|| invalid_data("mime length overflow"))?;

    if data_start > buf.len() {
        return Err(invalid_data("mime length exceeds blob size"));
    }

    let mime = String::from_utf8(buf[8..data_start].to_vec())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let data = buf[data_start..].to_vec();
    let actual_cid = *blake3::hash(&data).as_bytes();

    if actual_cid != expected_cid {
        return Err(invalid_data("blob content does not match CID"));
    }

    Ok(Blob {
        cid: expected_cid,
        data,
        mime,
    })
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
