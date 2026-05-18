# AEON Foundations

Date: 2026-05-18

This document records the implemented AEON foundations that sit above the original capture stream.

## Implemented Foundations

### Data Bridge

AEON now accepts typed bridge payloads and stores them through the existing capture engine.

Endpoints:

- `POST /api/bridge/sms`
- `POST /api/bridge/email`

SMS payload:

```json
{
  "message_id": "sms-1",
  "address": "10086",
  "body": "Your code is 476291",
  "received_at": 1771000000000,
  "direction": "Incoming"
}
```

Email payload:

```json
{
  "message_id": "email-1",
  "from": "noreply@example.test",
  "to": ["wc@example.test"],
  "subject": "Build finished",
  "body_preview": "AEON build completed successfully",
  "received_at": 1771000000100,
  "labels": ["inbox"]
}
```

SMS verification code extraction is deterministic and tested in both Rust and Android Kotlin.

### Operation Context Bus

AEON now has a single JSON-backed owner for current operational state.

State file:

- `~/.aeon/context.json`

Endpoints:

- `GET /api/context`
- `POST /api/context/task`
- `POST /api/context/clipboard`
- `POST /api/context/scratch`
- `POST /api/context/ai-session`

The context bus stores current task, shared clipboard text, scratch pad text, and resumable AI sessions. It is not a durable event log; durable history remains in capture records and `events.jsonl`.

### Account Profile Registry

AEON now stores external account profiles and browser profile launch plans.

State file:

- `~/.aeon/account-profiles.json`

Endpoints:

- `GET /api/accounts`
- `POST /api/accounts`
- `POST /api/accounts/:id/browser-plan`

Sensitive browser state is isolated by default:

- cookies
- local storage
- passwords

The browser plan endpoint returns launch arguments. It does not spawn Chrome directly.

### Credential Vault

AEON now has an encrypted credential vault foundation.

State file:

- `~/.aeon/vault.json`

Endpoints:

- `GET /api/vault/entries`
- `POST /api/vault/entries`

Security properties:

- PBKDF2-HMAC-SHA256 key derivation.
- Per-vault salt.
- AES-256-GCM authenticated encryption.
- Per-entry nonce.
- Metadata listing does not return decrypted secrets.
- No HTTP endpoint returns decrypted secret material in this slice.

### Query Foundation

AEON now exposes deterministic capture search.

Endpoint:

- `POST /api/query`

Payload:

```json
{
  "text": "context",
  "kind": "Text",
  "limit": 20
}
```

This is intentionally not a fake natural-language AI parser. It is a stable query surface that a future local LLM or cloud-backed parser can call.

### Android SMS Preparation

Android now includes:

- `VerificationCodeExtractor`
- `AeonAgent.SmsBridgePayload`
- `AeonAgent.captureSmsResult`

This prepares Android SMS ingestion, but a foreground/background SMS service and Android permissions are still separate work.

## Explicit Limits

Not implemented as completed integrations:

- OS keylogging.
- Network proxy capture.
- Microphone capture.
- Browser extension packaging.
- Browser credential auto-fill.
- IMAP/Gmail/Outlook sync workers.
- Android SMS observer service.
- WeChat multi-instance VM workaround.
- iOS SMS capture.
- LLM-backed natural language query planning.

These require explicit user consent, platform permissions, and separate threat modeling.

## Verification

Focused tests cover:

- bridge conversion and verification-code extraction
- operation context state ownership
- account profile launch planning
- credential vault encryption and wrong-password failure
- deterministic query filtering
- Android verification-code extraction
