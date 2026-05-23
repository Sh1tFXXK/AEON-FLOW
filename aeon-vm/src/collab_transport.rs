use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};

use crate::session::{MergeReport, SharedContext};

const MAX_CONTEXT_BYTES: usize = 16 * 1024 * 1024;

pub fn exchange_context(addr: &str, local: &mut SharedContext) -> io::Result<MergeReport> {
    let mut stream = TcpStream::connect(addr)?;
    write_context(&mut stream, local)?;
    let remote = read_context(&mut stream)?;
    local.merge_from(remote).map_err(invalid_data)
}

pub fn serve_context_once(
    listener: TcpListener,
    local: &mut SharedContext,
) -> io::Result<MergeReport> {
    let (mut stream, _) = listener.accept()?;
    let remote = read_context(&mut stream)?;
    let report = local.merge_from(remote).map_err(invalid_data)?;
    write_context(&mut stream, local)?;
    Ok(report)
}

fn write_context(stream: &mut TcpStream, context: &SharedContext) -> io::Result<()> {
    let bytes = context.to_bytes();
    if bytes.len() > MAX_CONTEXT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("context is larger than {} bytes", MAX_CONTEXT_BYTES),
        ));
    }

    let len = (bytes.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(&bytes)?;
    stream.flush()
}

fn read_context(stream: &mut TcpStream) -> io::Result<SharedContext> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_CONTEXT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("context frame is larger than {} bytes", MAX_CONTEXT_BYTES),
        ));
    }

    let mut bytes = vec![0u8; len];
    stream.read_exact(&mut bytes)?;
    SharedContext::from_bytes(&bytes).map_err(invalid_data)
}

fn invalid_data(err: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}
