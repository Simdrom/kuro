# Security policy

## Supported versions

Only the latest release gets security fixes. Update to it before reporting.

## Reporting a vulnerability

Please do not open a public issue.

Report it privately through
[GitHub's private vulnerability reporting](https://github.com/Simdrom/kuro/security/advisories/new),
or, if that is not available to you, message the maintainer (Simdrom) on GitHub
and ask for a private channel.

Include:

- the Kuro version and Windows version,
- what an attacker can do and what they need first,
- steps to reproduce, or a proof of concept.

You will get an answer within 7 days. Once a fix is released, the advisory is
published and you are credited, unless you would rather not be.

## Scope

In scope: the Kuro app, its installer, and how it handles tokens, sign-in,
links from other programs, network responses and imported files.

Out of scope: AniList, MyAnimeList, Crunchyroll and other services Kuro talks
to. Report those to their owners.

The measures Kuro already takes are listed under
[Security in the README](README.md#security). Dependencies are checked by
`cargo deny` in CI and by Dependabot.
