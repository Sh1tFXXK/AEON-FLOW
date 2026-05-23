const passwordInput = document.getElementById("password");
const ttlInput = document.getElementById("ttl");
const unlockButton = document.getElementById("unlock");
const fillButton = document.getElementById("fill");
const statusEl = document.getElementById("status");

function setStatus(text, error = false) {
  statusEl.textContent = text;
  statusEl.style.color = error ? "#b91c1c" : "#4b5563";
}

function send(message) {
  return chrome.runtime.sendMessage(message);
}

unlockButton.addEventListener("click", async () => {
  const password = passwordInput.value;
  if (!password) {
    setStatus("Enter the vault password.", true);
    return;
  }
  unlockButton.disabled = true;
  try {
    const response = await send({
      type: "aeon_unlock_vault",
      password,
      ttl_ms: Number(ttlInput.value),
    });
    if (!response?.ok) {
      throw new Error(response?.error || "unlock failed");
    }
    passwordInput.value = "";
    const expires = new Date(Number(response.expires_at)).toLocaleTimeString();
    setStatus(`Unlocked until ${expires}.`);
  } catch (error) {
    setStatus(String(error?.message || error), true);
  } finally {
    unlockButton.disabled = false;
  }
});

fillButton.addEventListener("click", async () => {
  fillButton.disabled = true;
  try {
    const response = await send({ type: "aeon_try_fill_credentials" });
    if (!response?.ok) {
      throw new Error(response?.error || "fill failed");
    }
    setStatus("Fill requested.");
  } catch (error) {
    setStatus(String(error?.message || error), true);
  } finally {
    fillButton.disabled = false;
  }
});
