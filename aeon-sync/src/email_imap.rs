use crate::email_sync::FetchedEmailMessage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

const IMAP_MAX_LIMIT: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapMailboxConfig {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub mailbox: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImapFetchError {
    Network,
    Protocol,
    TlsConfig,
}

pub async fn fetch_imap_messages(
    mailbox: &ImapMailboxConfig,
    credentials: &ImapCredentials,
    limit: usize,
) -> Result<Vec<FetchedEmailMessage>, ImapFetchError> {
    let stream = TcpStream::connect((mailbox.host.as_str(), mailbox.port))
        .await
        .map_err(|_| ImapFetchError::Network)?;
    if mailbox.tls {
        let connector = build_tls_connector(&mailbox.host)?;
        let server_name =
            ServerName::try_from(mailbox.host.clone()).map_err(|_| ImapFetchError::TlsConfig)?;
        let stream = connector
            .connect(server_name, stream)
            .await
            .map_err(|_| ImapFetchError::Network)?;
        fetch_imap_messages_over_io(stream, mailbox, credentials, limit).await
    } else {
        fetch_imap_messages_over_io(stream, mailbox, credentials, limit).await
    }
}

fn build_tls_connector(host: &str) -> Result<TlsConnector, ImapFetchError> {
    let _ = ServerName::try_from(host.to_string()).map_err(|_| ImapFetchError::TlsConfig)?;
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder_with_provider(
        tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .map_err(|_| ImapFetchError::TlsConfig)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

async fn fetch_imap_messages_over_io<S>(
    stream: S,
    mailbox: &ImapMailboxConfig,
    credentials: &ImapCredentials,
    limit: usize,
) -> Result<Vec<FetchedEmailMessage>, ImapFetchError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let _ = read_response_line(&mut reader).await?;

    send_command(
        &mut writer,
        "A0001",
        &format!(
            "LOGIN {} {}",
            quote_imap(&credentials.username),
            quote_imap(&credentials.password)
        ),
    )
    .await?;
    expect_ok(&read_response_until_tag(&mut reader, "A0001").await?)?;

    send_command(
        &mut writer,
        "A0002",
        &format!("SELECT {}", quote_imap(&mailbox.mailbox)),
    )
    .await?;
    expect_ok(&read_response_until_tag(&mut reader, "A0002").await?)?;

    send_command(&mut writer, "A0003", "UID SEARCH ALL").await?;
    let search = read_response_until_tag(&mut reader, "A0003").await?;
    expect_ok(&search)?;
    let uids = latest_uids(parse_search_uids(&search), limit);

    let mut messages = Vec::new();
    for (index, uid) in uids.into_iter().enumerate() {
        let tag = format!("A{:04}", index + 4);
        send_command(&mut writer, &tag, &format!("UID FETCH {uid} (BODY.PEEK[])")).await?;
        let fetch = read_response_until_tag(&mut reader, &tag).await?;
        expect_ok(&fetch)?;
        messages.extend(parse_fetch_messages(&fetch, &mailbox.mailbox)?);
    }

    let logout_tag = format!("A{:04}", messages.len() + 4);
    send_command(&mut writer, &logout_tag, "LOGOUT").await?;
    let _ = read_response_until_tag(&mut reader, &logout_tag).await?;

    Ok(messages)
}

async fn send_command<W: AsyncWrite + Unpin>(
    writer: &mut W,
    tag: &str,
    command: &str,
) -> Result<(), ImapFetchError> {
    writer
        .write_all(format!("{tag} {command}\r\n").as_bytes())
        .await
        .map_err(|_| ImapFetchError::Network)?;
    writer.flush().await.map_err(|_| ImapFetchError::Network)
}

async fn read_response_line<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<String, ImapFetchError> {
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .await
        .map_err(|_| ImapFetchError::Network)?;
    if read == 0 {
        return Err(ImapFetchError::Protocol);
    }
    Ok(line)
}

async fn read_response_until_tag<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    tag: &str,
) -> Result<String, ImapFetchError> {
    let mut response = String::new();
    loop {
        let line = read_response_line(reader).await?;
        let literal_len = imap_literal_len(&line);
        let tagged = line.starts_with(tag);
        response.push_str(&line);
        if let Some(len) = literal_len {
            let mut literal = vec![0u8; len];
            reader
                .read_exact(&mut literal)
                .await
                .map_err(|_| ImapFetchError::Network)?;
            response.push_str(&String::from_utf8_lossy(&literal));
        }
        if tagged {
            return Ok(response);
        }
    }
}

fn imap_literal_len(line: &str) -> Option<usize> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let start = trimmed.rfind('{')?;
    let end = trimmed[start + 1..].find('}')? + start + 1;
    trimmed[start + 1..end].parse().ok()
}

fn expect_ok(response: &str) -> Result<(), ImapFetchError> {
    response
        .lines()
        .last()
        .filter(|line| line.contains(" OK"))
        .map(|_| ())
        .ok_or(ImapFetchError::Protocol)
}

fn quote_imap(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn parse_search_uids(response: &str) -> Vec<u64> {
    response
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("* SEARCH"))
        .map(|line| {
            line.split_whitespace()
                .filter_map(|part| part.parse::<u64>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn latest_uids(mut uids: Vec<u64>, limit: usize) -> Vec<u64> {
    let limit = limit.clamp(1, IMAP_MAX_LIMIT);
    if uids.len() > limit {
        uids.drain(0..uids.len() - limit);
    }
    uids
}

fn parse_fetch_messages(
    response: &str,
    mailbox: &str,
) -> Result<Vec<FetchedEmailMessage>, ImapFetchError> {
    let bytes = response.as_bytes();
    let mut cursor = 0;
    let mut messages = Vec::new();

    while let Some(open) = find_byte(bytes, cursor, b'{') {
        let Some(close) = find_byte(bytes, open, b'}') else {
            return Err(ImapFetchError::Protocol);
        };
        let len = std::str::from_utf8(&bytes[open + 1..close])
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(ImapFetchError::Protocol)?;
        let literal_start = close + 3;
        let literal_end = literal_start.saturating_add(len);
        if literal_end > bytes.len() {
            return Err(ImapFetchError::Protocol);
        }

        let prefix_start = bytes[..open]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let prefix = String::from_utf8_lossy(&bytes[prefix_start..open]);
        let uid = parse_fetch_uid(&prefix).ok_or(ImapFetchError::Protocol)?;
        let raw = String::from_utf8_lossy(&bytes[literal_start..literal_end]);
        messages.push(parse_rfc822_message(uid, &raw, mailbox));
        cursor = literal_end;
    }

    Ok(messages)
}

fn find_byte(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    bytes
        .get(start..)?
        .iter()
        .position(|byte| *byte == needle)
        .map(|pos| pos + start)
}

fn parse_fetch_uid(prefix: &str) -> Option<u64> {
    let after_uid = prefix.split("UID ").nth(1)?;
    after_uid
        .split(|ch: char| !ch.is_ascii_digit())
        .next()
        .and_then(|value| value.parse().ok())
}

fn parse_rfc822_message(uid: u64, raw: &str, mailbox: &str) -> FetchedEmailMessage {
    let (headers, body) = split_headers_body(raw);
    let headers = parse_headers(headers);
    let message_id = headers
        .get("message-id")
        .cloned()
        .unwrap_or_else(|| uid.to_string());
    let body_preview = body.split_whitespace().collect::<Vec<_>>().join(" ");

    FetchedEmailMessage {
        uid,
        message_id: format!("imap:{message_id}"),
        from: headers.get("from").cloned().unwrap_or_default(),
        to: headers
            .get("to")
            .map(|to| split_address_list(to))
            .unwrap_or_default(),
        subject: headers.get("subject").cloned().unwrap_or_default(),
        body_preview: body_preview.chars().take(200).collect(),
        received_at: headers
            .get("date")
            .and_then(|date| parse_rfc2822_utc_ms(date))
            .unwrap_or_default(),
        labels: vec![mailbox.to_string()],
    }
}

fn split_headers_body(raw: &str) -> (&str, &str) {
    raw.split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .unwrap_or((raw, ""))
}

fn parse_headers(raw: &str) -> HashMap<String, String> {
    let mut unfolded: Vec<String> = Vec::new();
    for line in raw.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = unfolded.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
            continue;
        }
        unfolded.push(line.trim_end_matches('\r').to_string());
    }

    let mut headers = HashMap::new();
    for line in unfolded {
        if let Some((name, value)) = line.split_once(':') {
            headers
                .entry(name.trim().to_ascii_lowercase())
                .or_insert_with(|| value.trim().to_string());
        }
    }
    headers
}

fn split_address_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_rfc2822_utc_ms(value: &str) -> Option<u64> {
    let normalized = value.replace(',', " ");
    let parts = normalized.split_whitespace().collect::<Vec<_>>();
    let day_index = parts
        .windows(3)
        .position(|window| window[0].parse::<u32>().is_ok() && month_number(window[1]).is_some())?;
    let day = parts.get(day_index)?.parse::<u32>().ok()?;
    let month = month_number(parts.get(day_index + 1)?)?;
    let year = parts.get(day_index + 2)?.parse::<i32>().ok()?;
    let (hour, minute, second) = parse_hms(parts.get(day_index + 3)?)?;
    let offset = parts
        .get(day_index + 4)
        .and_then(|value| parse_tz_offset_seconds(value))
        .unwrap_or(0);
    let days = days_from_civil(year, month, day)?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))?
        .checked_sub(i64::from(offset))?;
    (seconds >= 0).then_some(seconds as u64 * 1_000)
}

fn month_number(value: &str) -> Option<u32> {
    match value.to_ascii_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn parse_hms(value: &str) -> Option<(u32, u32, u32)> {
    let mut parts = value.split(':');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

fn parse_tz_offset_seconds(value: &str) -> Option<i32> {
    if value.len() != 5 {
        return None;
    }
    let sign = match &value[..1] {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let hours = value[1..3].parse::<i32>().ok()?;
    let minutes = value[3..5].parse::<i32>().ok()?;
    Some(sign * (hours * 3_600 + minutes * 60))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = year - i32::from(month <= 2);
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let year_of_era = year - era * 400;
    let month = month as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(i64::from(era * 146_097 + day_of_era - 719_468))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    async fn read_command(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> String {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        line.trim_end_matches(['\r', '\n']).to_string()
    }

    #[tokio::test]
    async fn plain_imap_fetch_converts_rfc822_message_to_fetched_email() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);

            writer.write_all(b"* OK AEON test IMAP\r\n").await.unwrap();
            assert_eq!(
                read_command(&mut reader).await,
                "A0001 LOGIN \"wc@example.test\" \"secret\""
            );
            writer
                .write_all(b"A0001 OK LOGIN completed\r\n")
                .await
                .unwrap();

            assert_eq!(read_command(&mut reader).await, "A0002 SELECT \"INBOX\"");
            writer
                .write_all(b"* 1 EXISTS\r\nA0002 OK SELECT completed\r\n")
                .await
                .unwrap();

            assert_eq!(read_command(&mut reader).await, "A0003 UID SEARCH ALL");
            writer
                .write_all(b"* SEARCH 42\r\nA0003 OK SEARCH completed\r\n")
                .await
                .unwrap();

            assert_eq!(
                read_command(&mut reader).await,
                "A0004 UID FETCH 42 (BODY.PEEK[])"
            );
            let raw_message = concat!(
                "Message-ID: <m-42@example.test>\r\n",
                "From: Sender <sender@example.test>\r\n",
                "To: wc@example.test, ops@example.test\r\n",
                "Subject: Build finished\r\n",
                "Date: Thu, 01 Jan 1970 00:00:01 +0000\r\n",
                "\r\n",
                "AEON build completed.\r\nSecond line.\r\n"
            );
            writer
                .write_all(
                    format!(
                        "* 1 FETCH (UID 42 BODY[] {{{}}}\r\n{} )\r\nA0004 OK FETCH completed\r\n",
                        raw_message.len(),
                        raw_message
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();

            assert_eq!(read_command(&mut reader).await, "A0005 LOGOUT");
            writer
                .write_all(b"* BYE\r\nA0005 OK LOGOUT completed\r\n")
                .await
                .unwrap();
        });

        let mailbox = ImapMailboxConfig {
            host: "127.0.0.1".to_string(),
            port: addr.port(),
            tls: false,
            mailbox: "INBOX".to_string(),
        };
        let credentials = ImapCredentials {
            username: "wc@example.test".to_string(),
            password: "secret".to_string(),
        };

        let messages = fetch_imap_messages(&mailbox, &credentials, 5)
            .await
            .unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].uid, 42);
        assert_eq!(messages[0].message_id, "imap:<m-42@example.test>");
        assert_eq!(messages[0].from, "Sender <sender@example.test>");
        assert_eq!(
            messages[0].to,
            vec![
                "wc@example.test".to_string(),
                "ops@example.test".to_string()
            ]
        );
        assert_eq!(messages[0].subject, "Build finished");
        assert_eq!(messages[0].received_at, 1_000);
        assert_eq!(
            messages[0].body_preview,
            "AEON build completed. Second line."
        );
        assert_eq!(messages[0].labels, vec!["INBOX".to_string()]);
        server.await.unwrap();
    }

    #[test]
    fn tls_connector_accepts_dns_server_name() {
        let _connector = build_tls_connector("imap.example.test").unwrap();
    }
}
