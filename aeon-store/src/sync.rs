use crate::{Blob, CIDStore, Node, CID};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncMessage {
    Have {
        device_id: [u8; 16],
        bloom: Vec<u8>,
        total_count: u32,
    },
    Want {
        cid: CID,
    },
    Data {
        blob: Blob,
        node: Option<Node>,
    },
    Ping {
        timestamp: u64,
    },
    DeviceJoined {
        device_id: [u8; 16],
        session: String,
    },
    DeviceLeft {
        device_id: [u8; 16],
    },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncReport {
    pub requested: usize,
    pub sent: usize,
    pub received: usize,
}

pub struct SyncEngine {
    store: CIDStore,
    device_id: [u8; 16],
}

impl SyncEngine {
    pub fn new(store: CIDStore, device_id: [u8; 16]) -> Self {
        Self { store, device_id }
    }

    pub fn announce_to(&mut self, cid: CID, peer: &str) -> io::Result<SyncReport> {
        if !self.store.has(&cid) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "cannot announce missing CID",
            ));
        }

        let mut stream = TcpStream::connect(peer)?;
        write_message(
            &mut stream,
            &SyncMessage::Have {
                device_id: self.device_id,
                bloom: pack_cids(&[cid]),
                total_count: 1,
            },
        )?;

        let mut report = SyncReport::default();
        let mut reader = BufReader::new(stream.try_clone()?);
        while let Some(message) = read_message(&mut reader)? {
            match message {
                SyncMessage::Want { cid } => {
                    if let Some(blob) = self.store.get(&cid)? {
                        write_message(&mut stream, &SyncMessage::Data { blob, node: None })?;
                        report.sent += 1;
                    }
                }
                SyncMessage::Data { blob, .. } => {
                    self.store.put(blob)?;
                    report.received += 1;
                }
                _ => {}
            }
        }

        Ok(report)
    }

    pub fn listen_once(&mut self, addr: &str) -> io::Result<SyncReport> {
        let listener = TcpListener::bind(addr)?;
        self.listen_once_on(listener)
    }

    pub fn listen_once_on(&mut self, listener: TcpListener) -> io::Result<SyncReport> {
        let (mut stream, _) = listener.accept()?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let Some(message) = read_message(&mut reader)? else {
            return Ok(SyncReport::default());
        };

        match message {
            SyncMessage::Have { bloom, .. } => self.handle_have(&mut stream, &mut reader, &bloom),
            SyncMessage::Want { cid } => self.handle_want(&mut stream, cid),
            SyncMessage::Data { blob, .. } => {
                self.store.put(blob)?;
                Ok(SyncReport {
                    received: 1,
                    ..SyncReport::default()
                })
            }
            _ => Ok(SyncReport::default()),
        }
    }

    fn handle_have(
        &mut self,
        stream: &mut TcpStream,
        reader: &mut BufReader<TcpStream>,
        bloom: &[u8],
    ) -> io::Result<SyncReport> {
        let mut report = SyncReport::default();
        let offered = unpack_cids(bloom)?;
        let missing: Vec<CID> = offered
            .into_iter()
            .filter(|cid| !self.store.has(cid))
            .collect();

        for cid in &missing {
            write_message(stream, &SyncMessage::Want { cid: *cid })?;
            report.requested += 1;
        }

        while report.received < missing.len() {
            let Some(message) = read_message(reader)? else {
                break;
            };
            if let SyncMessage::Data { blob, .. } = message {
                self.store.put(blob)?;
                report.received += 1;
            }
        }

        Ok(report)
    }

    fn handle_want(&mut self, stream: &mut TcpStream, cid: CID) -> io::Result<SyncReport> {
        let mut report = SyncReport::default();
        if let Some(blob) = self.store.get(&cid)? {
            write_message(stream, &SyncMessage::Data { blob, node: None })?;
            report.sent = 1;
        }
        Ok(report)
    }
}

pub fn pack_cids(cids: &[CID]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(cids.len() * 32);
    for cid in cids {
        bytes.extend_from_slice(cid);
    }
    bytes
}

pub fn unpack_cids(bytes: &[u8]) -> io::Result<Vec<CID>> {
    if bytes.len() % 32 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packed CID list length is not a multiple of 32",
        ));
    }

    let mut cids = Vec::with_capacity(bytes.len() / 32);
    for chunk in bytes.chunks_exact(32) {
        let mut cid = [0u8; 32];
        cid.copy_from_slice(chunk);
        cids.push(cid);
    }
    Ok(cids)
}

fn write_message(stream: &mut TcpStream, message: &SyncMessage) -> io::Result<()> {
    serde_json::to_writer(&mut *stream, message).map_err(json_error)?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn read_message(reader: &mut BufReader<TcpStream>) -> io::Result<Option<SyncMessage>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let message = serde_json::from_str(line.trim_end()).map_err(json_error)?;
    Ok(Some(message))
}

fn json_error(err: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}
