use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

pub struct Identity {
    signing_key: SigningKey,
    pub public_key: VerifyingKey,
    pub id: [u8; 32],
}

impl Identity {
    pub fn generate() -> Self {
        let mut rng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let public_key = signing_key.verifying_key();
        let id = *blake3::hash(public_key.as_bytes()).as_bytes();
        Self {
            signing_key,
            public_key,
            id,
        }
    }

    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        if path.exists() {
            let bytes = std::fs::read(path)?;
            if bytes.len() < 32 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "identity file too short"));
            }
            let key_bytes: [u8; 32] = bytes[..32]
                .try_into()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid identity key"))?;
            let signing_key = SigningKey::from_bytes(&key_bytes);
            let public_key = signing_key.verifying_key();
            let id = *blake3::hash(public_key.as_bytes()).as_bytes();
            Ok(Self { signing_key, public_key, id })
        } else {
            let identity = Self::generate();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, identity.signing_key.to_bytes())?;
            Ok(identity)
        }
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }

    pub fn verify(public_key: &VerifyingKey, data: &[u8], sig: &Signature) -> bool {
        public_key.verify(data, sig).is_ok()
    }

    pub fn id_short(&self) -> String {
        hex::encode(&self.id[..4])
    }

    pub fn id_hex(&self) -> String {
        hex::encode(self.id)
    }

    pub fn private_key_hex(&self) -> String {
        hex::encode(self.signing_key.to_bytes())
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key.as_bytes())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: [u8; 16],
    pub identity_id: [u8; 32],
    pub name: String,
    pub platform: Platform,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Platform {
    Windows,
    Linux,
    MacOS,
    Android,
    IOS,
    Web,
}

impl Platform {
    pub fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            return Platform::Windows;
        }
        #[cfg(target_os = "linux")]
        {
            return Platform::Linux;
        }
        #[cfg(target_os = "macos")]
        {
            return Platform::MacOS;
        }
        #[cfg(target_os = "android")]
        {
            return Platform::Android;
        }
        #[cfg(target_os = "ios")]
        {
            return Platform::IOS;
        }

        Platform::Web
    }
}
