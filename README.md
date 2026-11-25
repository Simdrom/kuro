# Mango Desktop

Unofficial anime and manga tracker for Windows, written in Rust. It talks to
AniList and MyAnimeList, and also works with a purely local list.

Not affiliated with ManGo, RocketScience IT, MyAnimeList or AniList. It
contains no code or artwork from any of them.

## Development

    cargo run -p mango-desktop
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
