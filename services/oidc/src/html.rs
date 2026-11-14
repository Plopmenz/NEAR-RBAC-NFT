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
button:disabled { opacity: 0.6; cursor: default; }
.muted { opacity: 0.75; font-size: 0.9rem; }
.status.error { color: #b42318; font-size: 0.9rem; }
code { word-break: break-all; }
"#;

/// Login page shown by `/authorize`.
pub fn login_page(config: &Config, client: &ClientConfig, pending: &PendingLogin) -> String {
    let nonce = STANDARD.encode(pending.nep413_nonce);
    let recipient = config.recipient();
    let callback_url = config.nep413_callback_url();
    let network = config.network();
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

<button type="button" id="connect-wallet">Connect NEAR wallet</button>
<p class="muted" id="wallet-status" role="status">Connect a wallet, then press the button again to sign the NEP-413 message.</p>

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

<script type="module">
const message = {message_js};
const nonce = Uint8Array.from(atob({nonce_js}), (c) => c.charCodeAt(0));
const recipient = {recipient_js};
const callbackUrl = {callback_js};
const network = {network_js};

const button = document.getElementById("connect-wallet");
const walletStatus = document.getElementById("wallet-status");
const form = document.getElementById("login");

function setWalletStatus(text, isError) {{
  walletStatus.textContent = text;
  walletStatus.className = isError ? "status error" : "muted";
}}

let connector = null;
let connectedWallet = null;
let connectedSignerId;

async function getConnector() {{
  if (connector) return connector;
  // NEAR Connect ships as a browser module. The OIDC service has no bundler, so load it from an
  // ESM CDN. Wallets run sandboxed; this page never sees a private key.
  const module = await import("https://esm.sh/@hot-labs/near-connect@0.12.0");
  connector = new module.NearConnector({{
    network,
    features: {{
      signMessage: true,
      mainnet: network === "mainnet",
      testnet: network === "testnet",
    }},
    footerBranding: null,
  }});
  return connector;
}}

function errorText(error) {{
  return error && error.message ? error.message : error;
}}

async function connectWallet() {{
  button.disabled = true;
  setWalletStatus("Opening your wallet…", false);
  try {{
    const connector = await getConnector();
    setWalletStatus("Choose a wallet…", false);
    // Always open the selector so it only offers wallets that can sign NEP-413 messages.
    const wallet = await connector.connect();
    // Wallets with several connected accounts need an explicit signer.
    try {{
      connectedSignerId = (await wallet.getAccounts())[0]?.accountId;
    }} catch {{}}
    connectedWallet = wallet;
    button.textContent = "Sign the login message";
    button.disabled = false;
    setWalletStatus(
      "Connected" +
        (connectedSignerId ? " as " + connectedSignerId : "") +
        ". Press “Sign the login message” to approve the NEP-413 signature.",
      false,
    );
  }} catch (error) {{
    button.disabled = false;
    setWalletStatus("Wallet connection failed: " + errorText(error), true);
  }}
}}

async function signLoginMessage() {{
  // This must run directly from a click: opening the wallet's signing popup after an await would
  // be treated as a blocked popup by the browser.
  button.disabled = true;
  setWalletStatus("Waiting for the signature…", false);
  try {{
    const params = {{ message, recipient, nonce }};
    if (connectedSignerId) params.signerId = connectedSignerId;
    const signed = await connectedWallet.signMessage(params);
    document.getElementById("account_id").value = signed.accountId;
    document.getElementById("public_key").value = signed.publicKey;
    document.getElementById("signature").value = signed.signature;
    setWalletStatus("Signed in as " + signed.accountId + ". Continuing…", false);
    form.requestSubmit();
  }} catch (error) {{
    button.disabled = false;
    setWalletStatus("Wallet sign-in failed: " + errorText(error), true);
  }}
}}

if (callbackUrl) {{
  button.disabled = true;
  setWalletStatus(
    "This provider requires a NEP-413 callback URL, which NEAR Connect cannot include. Sign the message manually below.",
    true,
  );
}} else {{
  button.addEventListener("click", () => {{
    if (connectedWallet) {{
      void signLoginMessage();
    }} else {{
      void connectWallet();
    }}
  }});
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
        network_js = js_string(network),
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
<title>PayRole OIDC provider</title>
<style>{STYLE}</style>
</head>
<body>
<h1>PayRole OIDC provider</h1>
<p>Issuer: <code>{issuer}</code></p>
<p>PayRole contract: <code>{contract}</code></p>
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        toml::from_str(include_str!("../config.example.toml")).unwrap()
    }

    fn pending() -> PendingLogin {
        PendingLogin {
            id: "request-1".to_owned(),
            client_id: "demo-app".to_owned(),
            redirect_uri: "https://app.example.com/callback".to_owned(),
            state: Some("state".to_owned()),
            scope: "openid profile roles".to_owned(),
            oidc_nonce: None,
            code_challenge: "challenge".to_owned(),
            nep413_nonce: [7; 32],
            message: "Sign <in>".to_owned(),
            created_at: 0,
            expires_at: 0,
        }
    }

    #[test]
    fn login_page_wires_near_connect() {
        let config = config();
        let html = login_page(&config, &config.clients[0], &pending());
        assert!(html.contains("Connect NEAR wallet"));
        assert!(html.contains("Sign the login message"));
        assert!(html.contains("https://esm.sh/@hot-labs/near-connect@0.12.0"));
        assert!(html.contains("const network = \"testnet\";"));
        assert!(html.contains("new module.NearConnector({"));
        assert!(html.contains("void connectWallet();"));
        assert!(html.contains("void signLoginMessage();"));
        assert!(html.contains("form.requestSubmit();"));
        assert!(html.contains("Sign &lt;in&gt;"));
    }

    #[test]
    fn login_page_requires_manual_signing_when_a_callback_url_is_configured() {
        let mut config = config();
        config.nep413_callback_url = Some("https://auth.example.com/callback".to_owned());
        let html = login_page(&config, &config.clients[0], &pending());
        assert!(html.contains("const callbackUrl = \"https://auth.example.com/callback\";"));
        assert!(html.contains("NEAR Connect cannot include"));
    }
}
