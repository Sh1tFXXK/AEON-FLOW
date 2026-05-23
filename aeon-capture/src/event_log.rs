use crate::event::{AeonEvent, EventId};
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::path::PathBuf;

const DEFAULT_EVENT_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventQuery {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: usize,
}

impl Default for EventQuery {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            limit: DEFAULT_EVENT_LIMIT,
        }
    }
}

pub struct EventLog {
    path: PathBuf,
}

impl EventLog {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn append(&self, event: &AeonEvent) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(event)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()
    }

    pub fn list(&self, query: EventQuery) -> io::Result<Vec<AeonEvent>> {
        let mut events = self.read_all()?;
        events.retain(|event| {
            query.from.is_none_or(|from| event.ts >= from)
                && query.to.is_none_or(|to| event.ts <= to)
        });
        events.sort_by(|a, b| b.ts.cmp(&a.ts).then(a.id.0.cmp(&b.id.0)));
        events.truncate(query.limit);
        Ok(events)
    }

    pub fn get(&self, id: &EventId) -> io::Result<Option<AeonEvent>> {
        Ok(self.read_all()?.into_iter().find(|event| &event.id == id))
    }

    fn read_all(&self) -> io::Result<Vec<AeonEvent>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&self.path)?;
        let reader = std::io::BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let Ok(line) = line else {
                continue;
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<AeonEvent>(&line) {
                events.push(event);
            }
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
    use crate::event::{EventKind, EventSource};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-event-log-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn event_with_ts(ts: u64, text: &str) -> AeonEvent {
        let mut entry = CaptureEntry::new(
            text.as_bytes().to_vec(),
            CaptureKind::Text,
            CaptureSource::Manual,
        );
        entry.captured_at = ts;
        entry.by = [3u8; 32];
        entry.device = [4u8; 16];
        AeonEvent::from_capture(&entry)
    }

    #[test]
    fn append_writes_one_json_line_per_event_and_lists_newest_first() {
        let dir = temp_dir();
        let log = EventLog::new(dir.join("events.jsonl"));
        let older = event_with_ts(100, "older");
        let newer = event_with_ts(200, "newer");

        log.append(&older).unwrap();
        log.append(&newer).unwrap();

        let raw = std::fs::read_to_string(dir.join("events.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 2);

        let events = log
            .list(EventQuery {
                from: None,
                to: None,
                limit: 10,
            })
            .unwrap();
        assert_eq!(
            events.iter().map(|event| event.ts).collect::<Vec<_>>(),
            vec![200, 100]
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn range_query_filters_by_timestamp_and_limit() {
        let dir = temp_dir();
        let log = EventLog::new(dir.join("events.jsonl"));
        log.append(&event_with_ts(100, "a")).unwrap();
        log.append(&event_with_ts(200, "b")).unwrap();
        log.append(&event_with_ts(300, "c")).unwrap();

        let events = log
            .list(EventQuery {
                from: Some(150),
                to: Some(350),
                limit: 1,
            })
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].ts, 300);
        let EventKind::CaptureAdded(capture) = &events[0].kind;
        assert_eq!(capture.mime, "text/plain");
        assert_eq!(
            events[0].source,
            EventSource::LocalCapture(CaptureSource::Manual)
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_lines_are_skipped_during_reads() {
        let dir = temp_dir();
        let path = dir.join("events.jsonl");
        let valid = event_with_ts(100, "valid");
        std::fs::write(
            &path,
            format!("not-json\n{}\n", serde_json::to_string(&valid).unwrap()),
        )
        .unwrap();

        let log = EventLog::new(&path);
        let events = log.list(EventQuery::default()).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, valid.id);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn get_finds_event_by_id() {
        let dir = temp_dir();
        let log = EventLog::new(dir.join("events.jsonl"));
        let event = event_with_ts(100, "find me");
        let id = event.id;
        log.append(&event).unwrap();

        assert_eq!(log.get(&id).unwrap().unwrap().id, id);
        assert!(log.get(&EventId([0u8; 32])).unwrap().is_none());

        let _ = std::fs::remove_dir_all(dir);
    }
}
