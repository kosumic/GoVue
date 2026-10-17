# Cast networking exception

Status: approved by the owner on 2026-10-17: “Allow casting-only LAN HTTP”.
The exception below is the authorized implementation boundary.

LiTube's Cast receiver and “Open on any device” browser player fetch media from
the phone. The existing LingoPlay proxy listens only on loopback, which a receiver
cannot reach. `AGENTS.md` explicitly requires plain HTTP to remain on loopback.
The owner explicitly granted the exception for the reference transport.

Android supplies discovery, authenticated Cast TLS, Wi-Fi socket selection, and
the active-session notification. Rust owns the receiver/session state machine,
commands, media manifest, scoped routes, and transport lifetime. Live verification
and remaining parity gaps are recorded in [PORTING.md](../PORTING.md).

The approved exception is limited to a Cast session or a link the user shares:

- Bind a separate listener to the current private Wi-Fi address. Keep the native
  playback proxy on loopback and preserve the existing cleartext client policy.
  Android may reject explicit socket network selection; keep its ordinary routing
  policy in that case. Never change VPN settings or bind the whole process to
  another network to make a Cast connection succeed.
- Issue a fresh opaque capability for each session. Expose only its manifest and
  registered media parts; provide no directory listing, arbitrary URL forwarding,
  file access, or general browser endpoint.
- A selected Cast receiver must pass the existing device-authentication check.
  Restrict its media requests to that receiver's address. Browser link sharing
  uses its own capability because the user chooses the receiving browser.
- Keep upstream signed addresses in Rust memory. The link, manifest, and receiver
  metadata must not reveal upstream addresses or browser credentials. Do not log
  signed addresses, session capabilities, or raw request headers.
- Retain bounded upstream reads and limited concurrency. Validate method, Host,
  path, range, request size, and active-session ownership before serving bytes.
- Revoke a session on Stop Cast, Close link, source replacement, Wi-Fi address
  change, or expiry. Close the listener when no authorized session remains.
- Present a visible active-session notification with a Stop action while the
  transport operates in the background.

The exception authorizes implementing and testing this listener. It does
not authorize opening a general LAN proxy, changing browser-cookie access,
enabling release WebView debugging, or deploying an external relay service.
