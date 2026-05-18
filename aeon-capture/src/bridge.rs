use crate::{CaptureEntry, CaptureKind, CaptureSource};
use serde::{Deserialize, Serialize};

pub const BRIDGE_KIND_KEY: &str = "bridge.kind";
pub const SMS_BRIDGE_APP: &str = "bridge.sms";
pub const EMAIL_BRIDGE_APP: &str = "bridge.email";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmsBridgePayload {
    pub message_id: String,
    pub address: String,
    pub body: String,
    pub received_at: u64,
    pub direction: SmsDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SmsDirection {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailBridgePayload {
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body_preview: String,
    pub received_at: u64,
    pub labels: Vec<String>,
}

pub fn extract_verification_code(text: &str) -> Option<String> {
    let runs = digit_runs(text);
    if runs.is_empty() {
        return None;
    }

    let lower = text.to_ascii_lowercase();
    let has_code_cue = text.contains("验证码")
        || lower.contains("verification code")
        || lower.contains("verify code")
        || lower.contains("use code")
        || lower.contains(" code ");

    if has_code_cue {
        return runs.into_iter().find(|run| (4..=8).contains(&run.len()));
    }

    let mut six_digit_runs = runs
        .into_iter()
        .filter(|run| run.len() == 6)
        .collect::<Vec<_>>();
    if six_digit_runs.len() == 1 {
        six_digit_runs.pop()
    } else {
        None
    }
}

impl SmsBridgePayload {
    pub fn into_capture_entry(self) -> CaptureEntry {
        let mut entry = CaptureEntry::new(
            self.body.clone().into_bytes(),
            CaptureKind::Text,
            CaptureSource::AppApi {
                app: SMS_BRIDGE_APP.to_string(),
            },
        )
        .with_title(&format!("SMS from {}", self.address))
        .with_summary(&self.body)
        .with_app("SMS");
        entry.captured_at = self.received_at;
        entry
            .meta
            .extra
            .insert(BRIDGE_KIND_KEY.to_string(), "sms".to_string());
        entry
            .meta
            .extra
            .insert("message_id".to_string(), self.message_id);
        entry.meta.extra.insert("address".to_string(), self.address);
        entry.meta.extra.insert(
            "direction".to_string(),
            match self.direction {
                SmsDirection::Incoming => "incoming",
                SmsDirection::Outgoing => "outgoing",
            }
            .to_string(),
        );
        if let Some(code) = extract_verification_code(&self.body) {
            entry
                .meta
                .extra
                .insert("verification_code".to_string(), code);
        }
        entry
    }
}

impl EmailBridgePayload {
    pub fn into_capture_entry(self) -> CaptureEntry {
        let mut entry = CaptureEntry::new(
            self.body_preview.clone().into_bytes(),
            CaptureKind::Text,
            CaptureSource::AppApi {
                app: EMAIL_BRIDGE_APP.to_string(),
            },
        )
        .with_title(&self.subject)
        .with_summary(&self.body_preview)
        .with_app("Email");
        entry.captured_at = self.received_at;
        entry
            .meta
            .extra
            .insert(BRIDGE_KIND_KEY.to_string(), "email".to_string());
        entry
            .meta
            .extra
            .insert("message_id".to_string(), self.message_id);
        entry.meta.extra.insert("from".to_string(), self.from);
        entry.meta.extra.insert("to".to_string(), self.to.join(","));
        entry
            .meta
            .extra
            .insert("labels".to_string(), self.labels.join(","));
        entry
    }
}

fn digit_runs(text: &str) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        runs.push(current);
    }

    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CaptureKind, CaptureSource};

    #[test]
    fn extracts_verification_codes_from_chinese_and_english_messages() {
        assert_eq!(
            extract_verification_code("您的验证码是 476291，5分钟内有效").as_deref(),
            Some("476291")
        );
        assert_eq!(
            extract_verification_code("verification code: 123456").as_deref(),
            Some("123456")
        );
        assert_eq!(
            extract_verification_code("Use code 9384 to finish login").as_deref(),
            Some("9384")
        );
    }

    #[test]
    fn ignores_text_without_a_plausible_verification_code() {
        assert_eq!(extract_verification_code("订单 20260518 已发货"), None);
        assert_eq!(extract_verification_code("call me at 13800138000"), None);
    }

    #[test]
    fn sms_payload_converts_to_text_capture_with_bridge_metadata() {
        let payload = SmsBridgePayload {
            message_id: "sms-1".to_string(),
            address: "10086".to_string(),
            body: "您的验证码是 476291，5分钟内有效".to_string(),
            received_at: 1_771_000_000_000,
            direction: SmsDirection::Incoming,
        };

        let entry = payload.into_capture_entry();

        assert_eq!(entry.kind, CaptureKind::Text);
        assert_eq!(
            entry.source,
            CaptureSource::AppApi {
                app: SMS_BRIDGE_APP.to_string()
            }
        );
        assert_eq!(entry.data, "您的验证码是 476291，5分钟内有效".as_bytes());
        assert_eq!(entry.meta.app_name.as_deref(), Some("SMS"));
        assert_eq!(entry.meta.title.as_deref(), Some("SMS from 10086"));
        assert_eq!(
            entry.meta.extra.get(BRIDGE_KIND_KEY).map(String::as_str),
            Some("sms")
        );
        assert_eq!(
            entry
                .meta
                .extra
                .get("verification_code")
                .map(String::as_str),
            Some("476291")
        );
    }

    #[test]
    fn email_payload_converts_to_text_capture_with_subject_metadata() {
        let payload = EmailBridgePayload {
            message_id: "mail-1".to_string(),
            from: "noreply@example.test".to_string(),
            to: vec!["wc@example.test".to_string()],
            subject: "Build finished".to_string(),
            body_preview: "AEON build completed successfully".to_string(),
            received_at: 1_771_000_000_111,
            labels: vec!["inbox".to_string()],
        };

        let entry = payload.into_capture_entry();

        assert_eq!(entry.kind, CaptureKind::Text);
        assert_eq!(
            entry.source,
            CaptureSource::AppApi {
                app: EMAIL_BRIDGE_APP.to_string()
            }
        );
        assert_eq!(entry.meta.app_name.as_deref(), Some("Email"));
        assert_eq!(entry.meta.title.as_deref(), Some("Build finished"));
        assert_eq!(
            entry.meta.extra.get(BRIDGE_KIND_KEY).map(String::as_str),
            Some("email")
        );
        assert_eq!(
            entry.meta.extra.get("from").map(String::as_str),
            Some("noreply@example.test")
        );
    }
}
