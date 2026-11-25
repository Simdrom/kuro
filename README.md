# Mango Desktop

Unofficial anime and manga tracker for Windows, written in Rust. It works with
AniList and MyAnimeList, or with a list that stays on your computer.

Independent project. Not affiliated with ManGo, RocketScience IT, AniList or
MyAnimeList, and built without any of their code or artwork.

## Run

    cargo run --release -p mango-desktop

Needs stable Rust with the MSVC toolchain. Nothing else is required to browse
and keep a local list.

### Signing in

Each service asks every app for its own client ID. Create one (free), with the
redirect URL shown, and paste it in **Settings › Accounts**:

| Service | Page | Redirect URL |
| --- | --- | --- |
| AniList | https://anilist.co/settings/developer | `http://127.0.0.1:42813/anilist` |
| MyAnimeList (app type "other") | https://myanimelist.net/apiconfig | `http://127.0.0.1:42813/mal` |

To build them in, set `MANGO_ANILIST_CLIENT_ID` and `MANGO_MAL_CLIENT_ID` when
compiling. No client secret is used.

## What changes from a phone app

| On a phone | Here |
| --- | --- |
| Home screen widgets | Always-on-top desktop widget |
| Live Activity | Binge session mini window |
| Swipe actions | Drag a row, double-click, middle-click, context menu |
| Siri Shortcuts | Ctrl+K palette and `mango://` links |
| Safari extension | `mango://` links, bookmarklet, copied links |
| Watch app | Tray icon and Windows notifications |

Not included: watch complications, visionOS, and Arabic (the interface toolkit
has no right-to-left layout). Spotify opens a search, since its catalogue API
needs a private key.

## Layout

- `crates/core`: domain model, progress rules, SQLite store. No network, no UI.
- `crates/api`: AniList, MyAnimeList and music clients, OAuth, secret storage.
- `crates/app`: interface (egui) and Windows integration.

## Security

- `unsafe` code is forbidden in every crate of this workspace.
- Tokens are kept in the Windows Credential Manager, never on disk or in logs.
- Sign-in listens on 127.0.0.1 only while it runs, and checks Host, Origin,
  state and a one-time nonce. MyAnimeList uses PKCE.
- HTTPS only, bounded response sizes, images only from known hosts and decoded
  within limits.
- Links from other programs must match a known form. Only web addresses are
  ever handed to the system.
- `cargo deny check` covers advisories, licences and sources.

## Tests

    cargo test --workspace
    cargo test -p mango-api --test live -- --ignored     # real AniList
    cargo test -p mango-desktop --test shots -- --ignored # renders every screen
    cargo clippy --workspace --all-targets -- -D warnings

`MANGO_PROFILE_DIR=<folder>` runs a separate profile.
