use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VirtualFS {
    files: HashMap<String, Vec<u8>>,
    open_fds: Vec<Option<OpenFile>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenFile {
    pub path: String,
    pub cursor: usize,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    FileNotFound(String),
    BadFd(usize),
    NotWritable(usize),
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::FileNotFound(path) => write!(f, "file not found: {}", path),
            FsError::BadFd(fd) => write!(f, "bad file descriptor: {}", fd),
            FsError::NotWritable(fd) => write!(f, "file descriptor is not writable: {}", fd),
        }
    }
}

impl VirtualFS {
    pub fn open(&mut self, path: &str, writable: bool) -> Result<usize, FsError> {
        if writable {
            self.files.entry(path.to_string()).or_default();
        } else if !self.files.contains_key(path) {
            return Err(FsError::FileNotFound(path.to_string()));
        }

        let file = Some(OpenFile {
            path: path.to_string(),
            cursor: 0,
            writable,
        });
        if let Some((fd, slot)) = self
            .open_fds
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = file;
            Ok(fd)
        } else {
            self.open_fds.push(file);
            Ok(self.open_fds.len() - 1)
        }
    }

    pub fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        let (path, cursor) = {
            let open = self.open_file_mut(fd)?;
            (open.path.clone(), open.cursor)
        };
        let file = self
            .files
            .get(&path)
            .ok_or_else(|| FsError::FileNotFound(path.clone()))?;
        let available = file.len().saturating_sub(cursor);
        let len = available.min(buf.len());
        buf[..len].copy_from_slice(&file[cursor..cursor + len]);
        self.open_file_mut(fd)?.cursor = cursor + len;
        Ok(len)
    }

    pub fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, FsError> {
        let (path, cursor) = {
            let open = self.open_file_mut(fd)?;
            if !open.writable {
                return Err(FsError::NotWritable(fd));
            }
            (open.path.clone(), open.cursor)
        };

        let file = self.files.entry(path).or_default();
        let end = cursor + data.len();
        if end > file.len() {
            file.resize(end, 0);
        }
        file[cursor..end].copy_from_slice(data);
        self.open_file_mut(fd)?.cursor = end;
        Ok(data.len())
    }

    pub fn close(&mut self, fd: usize) -> Result<(), FsError> {
        let slot = self.open_fds.get_mut(fd).ok_or(FsError::BadFd(fd))?;
        if slot.is_none() {
            return Err(FsError::BadFd(fd));
        }
        *slot = None;
        Ok(())
    }

    fn open_file_mut(&mut self, fd: usize) -> Result<&mut OpenFile, FsError> {
        self.open_fds
            .get_mut(fd)
            .and_then(Option::as_mut)
            .ok_or(FsError::BadFd(fd))
    }
}
