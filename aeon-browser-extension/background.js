const DEFAULT_ENDPOINT = "http://127.0.0.1:8080";
const CODE_POLL_ALARM = "aeon_code_poll";
const filledCodesByTab = new Map();
const filledCredentialsByTab = new Map();

async function endpoint() {
  const stored = await chrome.storage.local.get({ aeonEndpoint: DEFAULT_ENDPOINT });
  return String(stored.aeonEndpoint || DEFAULT_ENDPOINT).replace(/\/+$/, "");
}

async function accountId() {
  const stored = await chrome.storage.local.get({ aeonAccountId: "" });
  const value = String(stored.aeonAccountId || "").trim();
  return value || null;
}

function isWebUrl(url) {
  return typeof url === "string" && (url.startsWith("http://") || url.startsWith("https://"));
}

async function reportTab(tab) {
  if (!tab || !isWebUrl(tab.url)) {
    return;
  }

  const body = {
    url: tab.url,
    title: tab.title || tab.url,
    captured_at: Date.now(),
    tab_id: tab.id,
  };
  const configuredAccount = await accountId();
  if (configuredAccount) {
    body.account_id = configuredAccount;
  }

  try {
    await fetch(`${await endpoint()}/api/bridge/browser-page`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
  } catch (_error) {
    // AEON may be offline; browser activity should never interrupt browsing.
  }
}

function fillKey(candidate) {
  return `${candidate.code}:${candidate.expires_at}`;
}

async function vaultSession() {
  const stored = await chrome.storage.local.get({
    aeonVaultSessionId: "",
    aeonVaultSessionExpiresAt: 0,
  });
  const sessionId = String(stored.aeonVaultSessionId || "").trim();
  const expiresAt = Number(stored.aeonVaultSessionExpiresAt || 0);
  if (!sessionId || expiresAt <= Date.now()) {
    return null;
  }
  return { sessionId, expiresAt };
}

async function unlockVault(password, ttlMs) {
  const response = await fetch(`${await endpoint()}/api/vault/unlock`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ password: String(password || ""), ttl_ms: ttlMs || undefined }),
  });
  if (!response.ok) {
    throw new Error(`vault unlock failed: ${response.status}`);
  }
  const payload = await response.json();
  if (!payload?.session_id || !payload?.expires_at) {
    throw new Error("vault unlock returned no session");
  }
  await chrome.storage.local.set({
    aeonVaultSessionId: String(payload.session_id),
    aeonVaultSessionExpiresAt: Number(payload.expires_at),
  });
  return payload;
}

async function credentialForTab(tab) {
  const session = await vaultSession();
  if (!session || !tab?.url || !isWebUrl(tab.url)) {
    return null;
  }

  try {
    const response = await fetch(`${await endpoint()}/api/vault/fill`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ session_id: session.sessionId, url: tab.url }),
    });
    if (response.status === 401) {
      await chrome.storage.local.remove(["aeonVaultSessionId", "aeonVaultSessionExpiresAt"]);
      return null;
    }
    if (!response.ok) {
      return null;
    }
    const payload = await response.json();
    return payload?.credential || null;
  } catch (_error) {
    return null;
  }
}

async function latestVerificationCode() {
  try {
    const response = await fetch(`${await endpoint()}/api/bridge/verification-code/latest`);
    if (!response.ok) {
      return null;
    }
    const payload = await response.json();
    const candidate = payload?.code;
    if (!candidate || !/^\d{4,8}$/.test(String(candidate.code || ""))) {
      return null;
    }
    if (Number(candidate.expires_at || 0) < Date.now()) {
      return null;
    }
    return candidate;
  } catch (_error) {
    return null;
  }
}

async function tryFillLatestVerificationCode() {
  const [active] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!active?.id || !isWebUrl(active.url)) {
    return;
  }

  const candidate = await latestVerificationCode();
  if (!candidate) {
    return;
  }

  const key = fillKey(candidate);
  if (filledCodesByTab.get(active.id) === key) {
    return;
  }

  try {
    const result = await chrome.tabs.sendMessage(active.id, {
      type: "aeon_fill_verification_code",
      code: String(candidate.code),
    });
    if (result?.ok) {
      filledCodesByTab.set(active.id, key);
    }
  } catch (_error) {
  }
}

function credentialKey(credential) {
  return `${credential.id}:${credential.expires_at}`;
}

async function tryFillCredentials(tab) {
  const target = tab || (await chrome.tabs.query({ active: true, currentWindow: true }))[0];
  if (!target?.id || !isWebUrl(target.url)) {
    return;
  }

  const credential = await credentialForTab(target);
  if (!credential?.username || !credential?.password) {
    return;
  }

  const key = credentialKey(credential);
  if (filledCredentialsByTab.get(target.id) === key) {
    return;
  }

  try {
    const result = await chrome.tabs.sendMessage(target.id, {
      type: "aeon_fill_credentials",
      username: String(credential.username),
      password: String(credential.password),
    });
    if (result?.ok) {
      filledCredentialsByTab.set(target.id, key);
    }
  } catch (_error) {
  }
}

chrome.tabs.onUpdated.addListener((_tabId, changeInfo, tab) => {
  if (changeInfo.status === "complete") {
    reportTab(tab);
    tryFillLatestVerificationCode();
    tryFillCredentials(tab);
  }
});

chrome.tabs.onActivated.addListener(async ({ tabId }) => {
  try {
    const tab = await chrome.tabs.get(tabId);
    reportTab(tab);
    tryFillLatestVerificationCode();
    tryFillCredentials(tab);
  } catch (_error) {
  }
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.alarms.create(CODE_POLL_ALARM, { periodInMinutes: 0.5 });
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === CODE_POLL_ALARM) {
    tryFillLatestVerificationCode();
  }
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "aeon_unlock_vault") {
    unlockVault(message.password, message.ttl_ms)
      .then((payload) => sendResponse({ ok: true, expires_at: payload.expires_at }))
      .catch((error) => sendResponse({ ok: false, error: String(error?.message || error) }));
    return true;
  }

  if (message?.type === "aeon_try_fill_credentials") {
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
      tryFillCredentials(tabs[0])
        .then(() => sendResponse({ ok: true }))
        .catch((error) => sendResponse({ ok: false, error: String(error?.message || error) }));
    });
    return true;
  }

  if (message?.type !== "aeon_fill_verification_code") {
    return false;
  }

  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    const active = tabs[0];
    if (!active?.id) {
      sendResponse({ ok: false, error: "no active tab" });
      return;
    }
    chrome.tabs.sendMessage(
      active.id,
      { type: "aeon_fill_verification_code", code: String(message.code || "") },
      sendResponse
    );
  });
  return true;
});
