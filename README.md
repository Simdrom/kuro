# Kuro 黒

[![CI](https://github.com/Simdrom/kuro/actions/workflows/ci.yml/badge.svg?branch=develop)](https://github.com/Simdrom/kuro/actions/workflows/ci.yml?query=branch%3Adevelop)
[![Release](https://github.com/Simdrom/kuro/actions/workflows/release.yml/badge.svg)](https://github.com/Simdrom/kuro/releases/latest)
[![Rust 1.95+](https://img.shields.io/badge/rust-1.95%2B-orange?logo=rust)](Cargo.toml)
![Windows](https://img.shields.io/badge/platform-Windows-0078D4)

*Kuro* is Japanese for black, like the ink of manga.

Unofficial anime and manga tracker for Windows, written in Rust. It works with
AniList and MyAnimeList, or with a list that stays on your computer.

Independent project. Not affiliated with ManGo, RocketScience IT, AniList or
MyAnimeList, and built without any of their code or artwork.

## Install

Download `kuro-<version>-windows-x64-setup.exe` from
[Releases](https://github.com/Simdrom/kuro/releases/latest). It installs for
your user only, without administrator rights. The `.zip` holds the same
program without an installer.

The setup looks at what is on this computer and offers the one action that
fits:

| Installed | Button | What it does |
| --- | --- | --- |
| None | **Install** | Installs, with a Start menu shortcut and, if you want, a desktop one |
| Older | **Update to X** | Closes Kuro and replaces it |
| The same | **Reinstall** | Writes the program again, which repairs it |
| Newer | **Go back to X** | Installs this older version, if it can still read your lists; if a newer Kuro changed them, it says so and changes nothing |

Your lists, settings and sessions are kept in every case. The new program is
written next to the old one and checked before it replaces it.

**Uninstall…**, in the setup or in Settings → Apps, closes Kuro and removes the
program, its shortcuts and its entries. Your lists and settings, in
`%LOCALAPPDATA%\Kuro`, and your sessions, in the Windows Credential Manager, stay
unless you tick *Also delete my lists, settings and sessions*.

The files are not signed, so Windows may warn that the publisher is unknown.
`SHA256SUMS` has their checksums. The setup writes what it did to
`%TEMP%\Kuro-Setup.log`.

For scripts: `/S` (silent), `/uninstall`, `/purge` (with `/uninstall`),
`/desktop=yes|no` and `/downgrade` (silent only goes back when asked). Silent
mode ends with exit code 1 when it refused or failed.

## Run from source

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

A sync shows how far it has got: the pages of history read, then the titles
looked up (about two seconds each on AniList, to respect its limits). To
follow it in a terminal too, with counts but never titles or ids, use a debug
build: release builds are windowed and have no console.

    $env:RUST_LOG = "kuro_app=info,kuro_api=info"; cargo run -p kuro

Kioku appears as a device called "Kuro" in your Crunchyroll account.
**Disconnect** forgets the session in Kuro; to end it on Crunchyroll as well,
remove that device in your account settings.

**Netflix** is an import, with nothing unofficial about it: on netflix.com,
open Account › Profiles › your profile › Viewing activity, choose **Download
all** and import that file in Kioku. Nothing is sent to Netflix. The file
gives episode titles but almost never their numbers, so episodes are counted
in the order they were watched and the review says so.

Series that are not found (most of a Netflix history is not anime) are folded
into one line. Any of them, or any title matched wrongly, can be set with
**Choose title**; Kioku remembers the choice for later imports. Title lookups
are kept for 30 days, so importing again is quick.

**Prime Video** offers no download of the history. Kioku imports the file
saved by [Watch History Exporter for Amazon Prime Video](https://github.com/twocaretcat/watch-history-exporter-for-amazon-prime-video),
a community script, not from Amazon, that you run in the browser console on
primevideo.com/settings/watch-history. It reads the page and makes no
requests of its own (checked in November 2026), but read it before running
it, and expect it to break if Amazon changes that page. Its file gives
episode numbers and ids, so nothing has to be counted. Some series appear
only as "Season 1"; the review shows one of their episodes so you can
choose the right title.

Disney+ is planned as an import of the file Disney hands out, and is not
available yet.

## Automation

**Settings › Widget & automation** gathers what Kuro does on its own.

- The **desktop widget** shows the next episodes to air with a countdown,
  what you are watching and reading with a +1 button, and your statistics;
  each can be turned off. Left open, it opens again with Kuro.
- **Automatic backup** saves a copy of your list every day or week in a
  folder you choose, as `kuro-backup-<date>.json`, and deletes the oldest
  copies beyond the number you keep. Other files in the folder are never
  touched. A copy restores with **Settings › Data › Import…**.
- **Sync** can read your list from your account again every few hours, so
  changes made on the website or another device show up on their own. Your
  own changes are always sent at once, and retried while offline.
- **Calendar** writes the next episode of everything you watch or plan to
  watch to an `.ics` file you choose, and rewrites it when the list or the
  schedule changes. Import it into a calendar, or subscribe to it where the
  calendar app allows a file.
- **Reminders** mention a title you are watching or reading that has not
  moved for one, two or four weeks (not one that is only waiting for its
  next episode), with a button to mark the next episode. Several at once
  come as one notification. A **weekly summary** on Monday counts what airs
  that week and the episodes you have left to catch up.
- **Kioku** can read the Crunchyroll history on its own once a day or week.
  As after **Sync history**, nothing is written: a notification says how
  many changes wait for your review, and opens Kioku.
  It can also look in your Downloads folder, about once an hour, for a
  Netflix or Prime Video history saved in the last two weeks: files with
  "netflix", "prime" or "amazon" in their name that read as that history.
  Kioku then offers to review the file; nothing is read beyond that folder,
  and a file you set aside is not offered again.
- **Links** open titles, searches and tabs, and a few do things without
  bringing the window forward, so a desktop shortcut, a hotkey tool or a
  Stream Deck can use them:

  | Link | Does |
  | --- | --- |
  | `kuro://step/anilist/anime/<id>?by=1` | Adds 1 to 10 episodes or chapters (negative removes them) |
  | `kuro://binge/anilist/anime/<id>` | Opens the binge session |
  | `kuro://random/anime` | Opens a random title |
  | `kuro://refresh` | Reads the list from the account again |

  Step links change your list, so they do nothing until **Allow links that
  change my list** is on; then a title's menu copies one. They only touch
  titles already in the list, never move a finished title and answer with a
  notification when the window is not in front.

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
- `crates/setup`: the setup, with the program inside (`KURO_PAYLOAD` at build
  time). `packaging/windows/build.ps1` builds it here, into `dist\`;
  `packaging/windows/test.ps1` tests it in the release.

## Security

- `unsafe` code is forbidden in every crate of this workspace, except the
  setup's Start menu and desktop shortcuts (`crates/setup/src/win.rs`), which
  need COM.
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
    cargo test -p kuro-api --test live -- --ignored     # real AniList; Crunchyroll with KURO_KIOKU_ETP_RT,
                                                        # a Netflix file with KURO_KIOKU_NETFLIX_CSV
                                                        # a Prime Video file with KURO_KIOKU_AMAZON_FILE
    cargo test -p kuro --test shots -- --ignored # renders every screen
    cargo clippy --workspace --all-targets -- -D warnings

`KURO_PROFILE_DIR=<folder>` runs a separate profile.

## Branches and releases

Work happens on `develop`, the default branch. `main` holds what is published
and only moves through a release.

The version is the one in the root `Cargo.toml`, and on `develop` it is always
the next release. To publish it, run **Actions › Release** from `develop` with
*dry run* off. It runs the whole CI, fast-forwards `main`, tags it, publishes
the executable and moves `develop` to the next minor version. Nothing is
pushed until everything has been built. It needs a write deploy key in the
`RELEASE_DEPLOY_KEY` secret; the client IDs come from the repository variables
`KURO_ANILIST_CLIENT_ID` and `KURO_MAL_CLIENT_ID`.

After cloning, turn on the local guard for `main`, `develop` and version tags:

    git config core.hooksPath .githooks

`.github/rulesets/apply.sh` sets the same rules on GitHub, which a private
repository only allows with GitHub Pro.
