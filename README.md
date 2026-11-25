# Kuro 黒

*Kuro* is Japanese for black, like the ink of manga.

Unofficial anime and manga tracker for Windows, written in Rust. It works with
AniList and MyAnimeList, or with a list that stays on your computer.

Independent project. Not affiliated with ManGo, RocketScience IT, AniList or
MyAnimeList, and built without any of their code or artwork.

## Run

    cargo run --release -p kuro

Needs stable Rust with the MSVC toolchain. Nothing else is required to browse
and keep a local list.

### Signing in

Each service asks every app for its own client ID. Create one (free), with the
redirect URL shown, and paste it in **Settings › Accounts**:

| Service | Page | Redirect URL |
| --- | --- | --- |
| AniList | https://anilist.co/settings/developer | `http://127.0.0.1:42813/anilist` |
| MyAnimeList (app type "other") | https://myanimelist.net/apiconfig | `http://127.0.0.1:42813/mal` |

To build them in, set `KURO_ANILIST_CLIENT_ID` and `KURO_MAL_CLIENT_ID` when
compiling. No client secret is used.

## Kioku

*Kioku* (記憶) is Japanese for memory. It marks as watched the episodes you
watched on streaming platforms, after you review every change. Open it from
**Settings › Accounts** or the Ctrl+K palette.

> **Unofficial integration.** Crunchyroll has no public API. Kioku uses the
> API of Crunchyroll's own website, which can change or stop working at any
> time, and using it this way may go against Crunchyroll's terms of use. Use
> it at your own risk. Kioku is not affiliated with or endorsed by Crunchyroll.

How it works with Crunchyroll:

1. **Connect** opens Crunchyroll's own sign-in page in a private window of
   Kuro (WebView2). Your password is typed into Crunchyroll and never reaches
   Kuro. The session cookie is exchanged once for a refresh token, the only
   thing kept, in the Windows Credential Manager.
2. **Sync history** reads the history (anime episodes and films only) and
   looks up each series on your primary service. Nothing is written yet.
3. The review lists each title with its change, its source and how sure the
   match is. Unsure matches, gaps in the history and paused or dropped titles
   are not ticked; series that were not found can be ignored.
4. Changes are written only after you confirm. Progress never moves back,
   completed titles and rewatches are left alone, and imported items are
   remembered, so syncing the same history again changes nothing.

Kioku appears as a device called "Kuro" in your Crunchyroll account.
**Disconnect** forgets the session in Kuro; to end it on Crunchyroll as well,
remove that device in your account settings.

Netflix, Disney+ and Prime Video are planned as imports of the files those
services hand out, and are not available yet.

## What changes from a phone app

| On a phone | Here |
| --- | --- |
| Home screen widgets | Always-on-top desktop widget |
| Live Activity | Binge session mini window |
| Swipe actions | Drag a row, double-click, middle-click, context menu |
| Siri Shortcuts | Ctrl+K palette and `kuro://` links |
| Safari extension | `kuro://` links, bookmarklet, copied links |
| Watch app | Tray icon and Windows notifications |

Not included: watch complications, visionOS, and Arabic (the interface toolkit
has no right-to-left layout). Spotify opens a search, since its catalogue API
needs a private key.

## Layout

- `crates/core`: domain model, progress rules, SQLite store. No network, no UI.
  Kioku's matcher and review rules live in `kioku/`.
- `crates/api`: AniList, MyAnimeList and music clients, OAuth, secret storage,
  and Kioku's history sources in `kioku/`.
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
- Kioku talks only to www.crunchyroll.com, follows no redirects and sends its
  own user agent (`Kuro-Kioku/<version>`). Its sign-in window only shows
  Crunchyroll's pages over HTTPS and keeps nothing on disk.
- `cargo deny check` covers advisories, licences and sources.

## Tests

    cargo test --workspace
    cargo test -p kuro-api --test live -- --ignored     # real AniList; Crunchyroll with KURO_KIOKU_ETP_RT
    cargo test -p kuro --test shots -- --ignored # renders every screen
    cargo clippy --workspace --all-targets -- -D warnings

`KURO_PROFILE_DIR=<folder>` runs a separate profile.
