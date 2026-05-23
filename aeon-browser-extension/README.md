# AEON Browser Bridge

This is a local Chrome/Chromium extension for AEON desktop.

Current capabilities:

- Reports completed and activated `http://` / `https://` tabs to
  `POST /api/bridge/browser-page`.
- Stores only URL/title/tab metadata as normal AEON capture records.
- Optionally includes `aeonAccountId` from `chrome.storage.local` for each
  Chrome Profile.
- Polls loopback-only `GET /api/bridge/verification-code/latest` and asks the
  content script to fill the active page when a fresh SMS-derived code is
  available.
- Contains a content-script action for explicitly requested verification-code
  fills as well.
- Provides an extension popup for unlocking the AEON credential vault for a
  short in-memory session.
- Requests `POST /api/vault/fill` for the active tab URL after unlock and fills
  matching username/password fields without submitting the form.

Current limits:

- No cookie export.
- No browser-native password-store import/export.
- No browser-native messaging host yet.
- Password fill requires a loopback AEON endpoint, an unexpired vault session,
  a `Password` credential with `auto_fill=true`, and a matching domain.
- Verification-code fill only works on pages with a detectable text, telephone,
  numeric, code, OTP, or verification input.

Load it from Chrome with `chrome://extensions` -> Developer mode -> Load
unpacked -> `aeon-browser-extension`.

Optional profile configuration from the extension service-worker console:

```js
chrome.storage.local.set({
  aeonEndpoint: "http://127.0.0.1:8080",
  aeonAccountId: "google-work"
});
```

Credential fill flow:

1. Open the extension popup.
2. Enter the AEON vault master password.
3. Choose an unlock duration.
4. Click `Unlock`.
5. Navigate to a matching login page or click `Fill Current Page`.
