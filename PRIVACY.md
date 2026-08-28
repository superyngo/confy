# Privacy Policy — Confy — TOML/JSON/YAML Editor

_Last updated: 2026-08-28_

Mirrored verbatim at <https://confy.turkeyang.net/privacy> (`web/privacy.html`) — the URL
used for every store listing's privacy policy field (Microsoft Store, Google Play, VS
Marketplace, future App Store). Keep both copies in sync on edit.

Confy ("the app") works offline. It has no backend: there is no Confy server to talk to, and
the only requests it ever makes go to URLs written by you or by the file you opened (see
**Network activity** below).

- **No telemetry.** The app does not collect, log, or transmit any usage data,
  analytics, or diagnostics.
- **No data transmission.** Files you open and edit stay on your device (or,
  for the web build, in your browser). Confy never uploads file contents
  anywhere.
- **Network activity.** Confy makes network requests in exactly two cases, both
  driven by content you choose to open, and never to any Confy-operated server:
  1. **Open from URL** — fetching a config file from a URL you explicitly supply
     (`confy <url>`, or the web build's open-from-URL action).
  2. **JSON Schema fetch** — if an opened document declares a schema by URL
     (a JSON `"$schema"` key, a YAML `# yaml-language-server:` modeline, or a
     TOML `#:schema` comment), Confy fetches that schema so it can show
     validation hints. The request goes to the URL written in your file; only
     that URL is requested, and no part of your file is sent with it.

  Both are plain GET requests. No other network activity occurs.
- **Local preferences only.** Language and theme preferences are stored
  locally (a config file on desktop/TUI, `localStorage` on the web build) and
  are never transmitted.
- **No accounts, no third parties.** Confy has no user accounts, no
  authentication, and does not share data with any third party — because it
  does not collect any data to share.

## Contact

Questions about this policy can be raised via the project's GitHub issues:
<https://github.com/superyngo/confy/issues>
