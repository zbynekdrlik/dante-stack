# Changelog

## 0.1.0 (2026-10-30)

First release: the Dante stack moves out of its two application copies.

- `dante-stack` (lib `dante_stack`): iemmixer's `iem-dante` (iemmixer `a510531`, frozen there at `131f44a`, no later change) with dante-asio's Windows port (dante-asio `21e79fa`): ARC, announce, CMC, flows, media TX/RX, the clock client of dantesync, health, the engine's pump, the device daemon.
- `dante-audio-io`: the callback contract (`Process`, `Block`, `StreamStats`) and the stream histograms, from iemmixer's `iem-audio-io`, which re-exports them.
- The device CLI is a library function, `daemon::cli::main(Build)`: each application keeps a thin binary with its own name, version and build (`dante-stack-dev` here, `iem-dante-dev` in iemmixer). `health::check` takes the build to compare.
- Fixed quirks: none yet (the quirk catalog starts with the open TX/RX problems).
