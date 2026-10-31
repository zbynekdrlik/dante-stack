---
paths:
  - "crates/dante-stack/tests/**"
  - "crates/**/tests.rs"
  - "crates/**/tests/**"
  - ".config/nextest.toml"
  - ".cargo/mutants.toml"
---

# Tests in dstack

- **Whole-device tests** (`tests/system_clock.rs`): the host refuses loopback (no MAC, unreachable for Dante), so run the device on the box's routed IPv4 (`UdpSocket::connect` to 192.0.2.1:9, then `local_addr`; sends nothing). Each test binds the Dante ports on that address: take the file's `ONE_DEVICE` lock.
- **Timing on CI and dev boxes:** a normal-priority TX thread wakes over 1 ms late now and then; at the test config's 1 ms latency that restarts the TX schedule (a 2 to 5 ms RX dropout). Whole-path tests use a 10 ms latency, take their counts after the start (0.1 s of RX read), and require no restart and every packet within the latency; real-time lateness is the hardware gate's, never a shared runner's.
- **Windows CI runs every test:** paths must be absolute on Windows too (`std::env::temp_dir()`), not `/run/...`.
- **Async inputs:** the directory's ARC and DBC records come from separate browse threads; wait for every record a lookup needs before it (a lookup without them registers no want).
- **Mutation gate:** per mutant only the unit tests and `tests/hot_path.rs` run (`.cargo/mutants.toml`); a whole-device test cannot run per mutant (ports). A test bound inside a step must carry the step's own time (`Instant` around `step`), not a fixed bound.
- **Fake time vs real time:** TX lateness adds real time inside a step to the fake PTP time; bound it by the measured step duration.
