# Security policy

## Supported versions

Only the [latest release](https://github.com/kvnshroom/giana-sisters-native/releases/latest) gets fixes. The game
checks for updates at start and can install them itself.

## What counts

The game talks to the network in three places, and problems there matter most:

- the **update check and installer** (downloads from GitHub releases, verified against `SHA256SUMS.txt`),
- **network play** (port 47064, TCP and UDP; the host can send a ROM module to the guest),
- **files it opens**: disk images (`.g64`, `.d64`) and ROM modules (`.grom`).

## Reporting

Please **do not open a public issue** for a security problem. Use
[Report a vulnerability](https://github.com/kvnshroom/giana-sisters-native/security/advisories/new) on the Security tab
of this repository instead. Say what an attacker could do and how to reproduce it.

This is a hobby project maintained in spare time: expect an answer within a few weeks, not days. Fixes are released as
a normal update.
