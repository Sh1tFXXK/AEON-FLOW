use std::io::{self, Read, Write};

use crate::ProgramId;

pub const SNAPSHOT: u8 = 0x01;
pub const PATCHSET: u8 = 0x02;
pub const NEED_PROGRAM: u8 = 0x03;
pub const PROGRAM: u8 = 0x04;
pub const OK: u8 = 0x05;
pub const ERROR: u8 = 0x06;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub msg_type: u8,
    pub payload: Vec<u8>,
}

pub fn write_msg(stream: &mut impl Write, msg_type: u8, payload: &[u8]) -> io::Result<()> {
    stream.write_all(&[msg_type])?;
    stream.write_all(&(payload.len() as u64).to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

pub fn read_msg(stream: &mut impl Read) -> io::Result<Message> {
    let mut msg_type = [0u8; 1];
    stream.read_exact(&mut msg_type)?;

    let mut len = [0u8; 8];
    stream.read_exact(&mut len)?;
    let payload_len = u64::from_be_bytes(len) as usize;

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;

    Ok(Message {
        msg_type: msg_type[0],
        payload,
    })
}

pub fn write_error(stream: &mut impl Write, message: impl AsRef<str>) -> io::Result<()> {
    write_msg(stream, ERROR, message.as_ref().as_bytes())
}

pub fn parse_program_id(payload: &[u8]) -> io::Result<ProgramId> {
    if payload.len() != 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected 32-byte ProgramId, got {}", payload.len()),
        ));
    }

    let mut id = [0u8; 32];
    id.copy_from_slice(payload);
    Ok(id)
}
