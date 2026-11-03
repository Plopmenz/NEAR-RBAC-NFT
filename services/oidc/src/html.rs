//! Small, dependency-free HTML rendering for the login flow.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::config::{ClientConfig, Config};
use crate::store::PendingLogin;

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value)
        .expect("strings always serialize")
        .replace('<', "\\u003c")
}

fn js_option_string(value: Option<&str>) -> String {
    match value {
        Some(value) => js_string(value),
        None => "null".to_owned(),
    }
}

const STYLE: &str = r#"
:root { color-scheme: light dark; }
body { font-family: system-ui, -apple-system, Segoe UI, sans-serif; max-width: 44rem;
       margin: 3rem auto; padding: 0 1.25rem; line-height: 1.5; }
h1 { font-size: 1.5rem; }
label { display: block; margin-top: 1rem; font-weight: 600; }
input, textarea { width: 100%; box-sizing: border-box; padding: 0.5rem; margin-top: 0.3rem;
                  font: inherit; border: 1px solid #888; border-radius: 0.35rem; }
textarea { min-height: 5rem; }
button { margin-top: 1rem; padding: 0.6rem 1rem; font: inherit; cursor: pointer; }
.muted { opacity: 0.75; font-size: 0.9rem; }
code { word-break: break-all; }
"#;

/// Login page shown by `/authorize`.
pub fn login_page(config: &Config, client: &ClientConfig, pending: &PendingLogin) -> String {
    let nonce = STANDARD.encode(pending.nep413_nonce);
    let recipient = config.recipient();
    let callback_url = config.nep413_callback_url();
    let message = pending.message.as_str();
    let client_name = escape(client.display_name());
    let scopes = escape(&pending.scope);
    let roles = if client.required_roles.is_empty() {
        "No specific role is required for this client.".to_owned()
    } else {
        format!(
            "You must hold all of these on-chain roles: <strong>{}</strong>.",
            escape(&client.required_roles.join(", "))
        )
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Sign in to {client_name}</title>
<style>{STYLE}</style>
</head>
<body>
<h1>Sign in to {client_name}</h1>
<p class="muted">Requested scopes: <code>{scopes}</code></p>
<p>{roles}</p>

<label for="account_id">NEAR account id</label>
<input id="account_id" name="account_id" form="login" autocomplete="username"
       placeholder="alice.near" required>

<button type="button" onclick="signWithWallet()">Sign with NEAR wallet</button>

<form id="login" method="post" action="/login">
  <input type="hidden" name="request_id" value="{request_id}">
  <label for="public_key">Public key</label>
  <input id="public_key" name="public_key" placeholder="ed25519:..." required>

  <label for="signature">Signature (base64)</label>
  <input id="signature" name="signature" required>

  <label for="message">Message that must be signed</label>
  <textarea id="message" readonly>{message_escaped}</textarea>
  <p class="muted">Recipient: <code>{recipient_escaped}</code><br>
  Nonce (base64): <code>{nonce_escaped}</code></p>

  <button type="submit">Continue</button>
</form>

<p class="muted">NEP-413: the wallet signs the SHA-256 hash of
<code>tag || borsh({{message, nonce, recipient, callback_url}})</code> with the key that
controls your account. This page never sees your private key.</p>

<script>
const message = {message_js};
const nonce = Uint8Array.from(atob({nonce_js}), c => c.charCodeAt(0));
const recipient = {recipient_js};
const callbackUrl = {callback_js};

async function findWallet() {{
  if (window.nearWallet && typeof window.nearWallet.signMessage === 'function') {{
    return window.nearWallet;
  }}
  if (window.selector && typeof window.selector.wallet === 'function') {{
    const wallet = await window.selector.wallet();
    if (wallet && typeof wallet.signMessage === 'function') {{
      return wallet;
    }}
  }}
  return null;
}}

async function signWithWallet() {{
  const accountId = document.getElementById('account_id').value.trim();
  if (!accountId) {{ alert('Enter your NEAR account id first.'); return; }}
  const wallet = await findWallet();
  if (!wallet) {{
    alert('No NEP-413 wallet was detected. Sign the message manually and paste the public key and signature.');
    return;
  }}
  try {{
    const result = await wallet.signMessage({{ message, recipient, nonce, callbackUrl }});
    document.getElementById('account_id').value = result.accountId;
    document.getElementById('public_key').value = result.publicKey;
    document.getElementById('signature').value = result.signature;
  }} catch (err) {{
    alert('Signing failed: ' + err);
  }}
}}
</script>
</body>
</html>
"#,
        request_id = escape(&pending.id),
        message_escaped = escape(message),
        recipient_escaped = escape(&recipient),
        nonce_escaped = escape(&nonce),
        message_js = js_string(message),
        nonce_js = js_string(&nonce),
        recipient_js = js_string(&recipient),
        callback_js = js_option_string(callback_url),
    )
}

/// Small landing page.
pub fn index_page(config: &Config) -> String {
    let clients = config
        .clients
        .iter()
        .map(|client| {
            format!(
                "<li><code>{}</code> — {}</li>",
                escape(&client.client_id),
                escape(client.display_name())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>NEAR RBAC OIDC provider</title>
<style>{STYLE}</style>
</head>
<body>
<h1>NEAR RBAC OIDC provider</h1>
<p>Issuer: <code>{issuer}</code></p>
<p>RBAC contract: <code>{contract}</code></p>
<h2>Registered clients</h2>
<ul>{clients}</ul>
<p><a href="/.well-known/openid-configuration">OpenID Connect discovery document</a></p>
</body>
</html>
"#,
        issuer = escape(config.base_url()),
        contract = escape(&config.contract_id),
    )
}
