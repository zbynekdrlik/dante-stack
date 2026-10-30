# Changelog

## 0.2.5 (2026-10-30)

- New (request #2): Dante Controller can rename our device. ARC 0x1001 (netaudio's `set_name`, body = the name NUL-ended) sets the device's name when Dante holds it (1 to 31 letters, digits and hyphens, no hyphen at either end): saved in the state file, the records announced under the new name, our flow requests carry it (`Manager::set_host`, `Flows::renamed`, `Change::device_renamed`). The same name changes nothing; anything else is refused. Inferno leaves 0x1001 unanswered; Dante Controller's own frame is still to be captured (#1).
- API: `arc::Flows` gains `renamed(&self, name)`; `arc::Change` gains `device_renamed`.

## 0.2.4 (2026-10-30)

- New: `arc::control`: read another device's name (0x1002), counts (0x1000) and every channel's label (TX: the 0x2010 own name else the 0x2000 default; RX: 0x3000), set or reset a channel's label (0x2013 / 0x3001) and set the device's name (0x1001), with netaudio v0.0.10's requests byte for byte, over the new `Call` trait (`arc::client::Udp` implements it). Every request is tested against netaudio's frames and round-tripped through our own ARC server.
- New: `card read|write|rename <ip> <expected-name> ...` on the device CLI (`daemon::card`, `Command::Card`), iemmixer's `dante_card.py` contract (JSON lines, the flag file, the read back, exit 0/1/2), so iemmixer can drop netaudio. One difference: a write the device refuses or leaves unanswered is `failed` and stops the writes (netaudio counted an unanswered write as sent). Tested in process and over the network against our own running device; a real card's replies to 0x3001/0x2013/0x1001 and its 0x2010 paging past 32 channels are still to be captured (#1).

## 0.2.3 (2026-10-30)

- Proven: a TX-only device (an empty RX list) sets up, starts, grants a receiver's flow of two channels and sends them stamped in order (`tests/system_clock.rs` `a_tx_only_device_serves_a_receiver`, Linux and Windows), for songplayer. No code change was needed. The whole-device tests now run one device at a time (they bind the same ports).

## 0.2.2 (2026-10-30)

- New, TEST ONLY: `host::ClockSource { Dantesync, System }` and `host::prepare_with(opts, engine_tx, source)`; `host::prepare` is unchanged (dantesync). `System` takes PTP time from the machine's own clock, ready at once, no dantesync poller, a warning at start, and the status's `clock.source` reads `system` (else `dantesync`; added field): for a whole device process on a box with no PTP network (CI). CLI: `--clock dantesync|system` on the run command (`daemon::Command::RunOnSystemClock`). Requested by dante-asio for its whole-process E2E. Test `tests/system_clock.rs` (Linux and Windows): the real host at a 10 ms latency, RX 1 subscribed to our own TX 1; after the start, a clean second of the counter, no TX schedule restart, every packet within the latency (the lateness bins are printed; real-time lateness is the hardware gate's).

## 0.2.1 (2026-10-30)

- New: coverage-guided fuzzing in CI (`fuzz/`, cargo-fuzz): `arc_server`, `flows_server`, `cmc`, `info`, `dsyx`, `media_packet` and `replies` (ARC and flow-control replies, a grant's handle, a string at an offset), 30 s each on every push. No change to the stack's code.

## 0.2.0 (2026-10-30)

Breaking: the box health moves to the application (iemmixer, agreed on #1).

- Kept in `health`: `Clock`, `Pump`, `Status`, `parse_status`, `read_status` (Linux). New: `DeviceReason` and `device_reasons(first, second, engine)`, the device's own reasons from two status reads a second apart: `NoStatus`, `ClockNotReady`, `Holdover`, and with an engine `PumpFree`, `PumpStalled`, `PumpRebootstrap` (the rules the box verdict used for these).
- Removed from `health`: `UNITS`, `PROPERTIES`, `SERVER_PORT`, `CLOCK_GRACE_US`, `Options`, `Active`, `UnitResult`, `Unit`, `Units`, `parse_units`, `ServerAnswer`, `server_answer`, `ask_server`, `State`, `Reason`, `Build` (the enum), `Verdict`, `verdict`, `Verdict::line`, `check`; internal to it: the glue constants `READ_APART`, `STATUS_WAIT`, `SERVER_WAIT`, `VERSION_REQUEST` (`MAX_READ` stays internal to `read_status`).
- Removed from `daemon`: `Command::Health` and the `health` subcommand (`parse_args` reads `health` as an unknown argument); `daemon::cli` no longer serves it.
- To port (iemmixer): the code and tests as of `fdfc321` (v0.1.2): `crates/dante-stack/src/health.rs`, `crates/dante-stack/src/health/tests.rs`, `Command::Health`, `parse_health` and its test `health_takes_a_socket_and_a_server_port` in `crates/dante-stack/src/daemon.rs` / `daemon/tests.rs`, and `health_line` in `crates/dante-stack/src/daemon/cli.rs`; `ask_server` needs its own copy of `MAX_READ` (1 MiB).

## 0.1.2 (2026-10-30)

- New: each TX lane counts how late its packets left past due, in bins < 0.25, < 0.5, < 1, < 2, < 5 and >= 5 ms (`LaneCounts::late_bins`), with the largest lateness (`late_max_ns`) and the longest single send (`send_max_ns`) since the device started; the control socket's status shows `late_bins`, `late_max_us` and `send_max_us` per TX flow (added fields). A stall inside the latency, which sends every packet late, used to read 0 in every counter (problem A, the 5.4 ms stall of 30.10 on the FOH PC); catalog test `a_stall_within_the_latency_is_counted_as_late_packets`.
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
