# Changelog

## 0.1.2 (2026-10-30)

- New: each TX lane counts how late its packets left past due, in bins < 0.25, < 0.5, < 1, < 2, < 5 and >= 5 ms (`LaneCounts::late`), with the latest (`late_max_ns`) and the longest single send (`send_max_ns`); the control socket's status shows `late`, `late_max_us` and `send_max_us` per TX flow (added fields). A stall inside the latency, which sends every packet late, used to read 0 in every counter (problem A, the 5.4 ms stall of 30.10 on the FOH PC); catalog test `a_stall_within_the_latency_is_counted_as_late_packets`.
- Removed: the hot-path log of a send blocking over 200 us (Windows experiment); `send_max_us` replaces it, and the hot path logs nothing again.

## 0.1.1 (2026-10-30)

- Fixed: `dante-audio-io` did not build with `--cfg iem_rtsan` (E0658: its crate root lacked `feature(sanitize)` for `hist::PeriodHist::record`), so an application's rtsan job failed (iemmixer). New CI job `rtsan` runs `tests/rtsan.rs`: the pump block and its histograms under RealtimeSanitizer, plus a self-test violation.

## 0.1.0 (2026-10-30)

First release: the Dante stack moves out of its two application copies.

- `dante-stack` (lib `dante_stack`): iemmixer's `iem-dante` (iemmixer `a510531`, frozen there at `131f44a`, no later change) with dante-asio's Windows port (dante-asio `21e79fa`): ARC, announce, CMC, flows, media TX/RX, the clock client of dantesync, health, the engine's pump, the device daemon.
- `dante-audio-io`: the callback contract (`Process`, `Block`, `StreamStats`) and the stream histograms, from iemmixer's `iem-audio-io`, which re-exports them.
- The device CLI is a library function, `daemon::cli::main(Build)`: each application keeps a thin binary with its own name, version and build (`dante-stack-dev` here, `iem-dante-dev` in iemmixer). `health::check` takes the build to compare.
- Changed against iemmixer's copy, by the Windows port (dante-asio), and visible to an application:
  - `stream::BLOCKS` and the device config's `block` take 16, 32, 64, 128 or 256 frames (was 16 or 32). An application whose own block contract is narrower checks it itself.
  - Windows only: the media threads run at `THREAD_PRIORITY_TIME_CRITICAL` (31 in a REALTIME_PRIORITY_CLASS process) instead of MMCSS "Pro Audio"; their waits are a yielding spin, never the timer.
  - The TX thread logs why its schedule starts again (the lag or a jump); on Windows it also logs a send that blocks over 200 us.
  - `daemon::version_line()` is gone (`Build::line`), `health::check` takes the binary's `Build`, the help text is `daemon::usage(name)`.
- Fixed quirks: none yet (the quirk catalog starts with the open TX/RX problems).
