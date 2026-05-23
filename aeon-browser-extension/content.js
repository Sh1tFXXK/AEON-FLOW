function candidateInputs() {
  return Array.from(document.querySelectorAll("input"))
    .filter((input) => {
      const text = [
        input.name,
        input.id,
        input.autocomplete,
        input.placeholder,
        input.getAttribute("aria-label"),
      ].join(" ").toLowerCase();
      return (
        input.type === "text" ||
        input.type === "tel" ||
        input.inputMode === "numeric" ||
        text.includes("code") ||
        text.includes("otp") ||
        text.includes("verification")
      );
    });
}

function passwordInput() {
  return document.querySelector('input[type="password"]');
}

function usernameInput(password) {
  const inputs = Array.from(document.querySelectorAll("input"))
    .filter((input) => input !== password && !input.disabled && !input.readOnly)
    .filter((input) => {
      const type = String(input.type || "text").toLowerCase();
      return ["text", "email", "tel", "search", "url", ""].includes(type);
    });
  const beforePassword = inputs.filter((input) => {
    if (!password || !input.compareDocumentPosition) {
      return true;
    }
    return Boolean(input.compareDocumentPosition(password) & Node.DOCUMENT_POSITION_FOLLOWING);
  });
  const scored = beforePassword
    .map((input) => {
      const text = [
        input.name,
        input.id,
        input.autocomplete,
        input.placeholder,
        input.getAttribute("aria-label"),
      ].join(" ").toLowerCase();
      const score =
        Number(text.includes("user")) * 3 +
        Number(text.includes("email")) * 3 +
        Number(text.includes("login")) * 2 +
        Number(text.includes("account")) * 2;
      return { input, score };
    })
    .sort((a, b) => b.score - a.score);
  return scored[0]?.input || beforePassword[0] || null;
}

function setInputValue(input, value) {
  input.focus();
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

function fillVerificationCode(code) {
  const value = String(code || "").trim();
  if (!/^\d{4,8}$/.test(value)) {
    return { ok: false, error: "invalid code" };
  }

  const input = candidateInputs()[0];
  if (!input) {
    return { ok: false, error: "no verification input found" };
  }

  input.focus();
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
  return { ok: true };
}

function fillCredentials(username, password) {
  const passwordEl = passwordInput();
  if (!passwordEl) {
    return { ok: false, error: "no password input found" };
  }
  const usernameEl = usernameInput(passwordEl);
  if (usernameEl) {
    setInputValue(usernameEl, String(username || ""));
  }
  setInputValue(passwordEl, String(password || ""));
  return { ok: true, username: Boolean(usernameEl) };
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "aeon_fill_verification_code") {
    sendResponse(fillVerificationCode(message.code));
    return false;
  }
  if (message?.type === "aeon_fill_credentials") {
    sendResponse(fillCredentials(message.username, message.password));
    return false;
  }
  return false;
});
