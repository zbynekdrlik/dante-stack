//! A whole device on the test-only system clock (dante-asio's CI E2E: a
//! GitHub-hosted runner has no dantesync and no PTP network). The real
//! host is prepared on the box's IPv4 address on its default route (the
//! production checks stay: no loopback, a real MAC) with
//! `ClockSource::System` and a state
//! file that subscribes RX 1 to our own TX 1 (resolved from our own
//! records, no mDNS); it starts its threads as in production. TX 1 sends
//! the counter; after the start, a second of RX 1 must read clean with no
//! TX schedule restart and every packet within the latency. Only our own
//! device is subscribed.

#![cfg(any(target_os = "linux", windows))]

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use dante_stack::ctl;
use dante_stack::daemon::{Options, Place};
use dante_stack::device::{DeviceConfig, DeviceState, Subscription};
use dante_stack::host::{self, ClockSource, Signals};

const TEST_DEVICE: &str = include_str!("../../../config/test-dante-device.toml");
/// How long the flow may take to be asked for, granted and read clean.
const BOUND: Duration = Duration::from_secs(20);

/// The test device's config with its state file in `dir`, and a state
/// file that subscribes RX 1 to our own TX 1.
fn device_in(dir: &Path) -> PathBuf {
    let state = dir.join("state.toml");
    let text = format!(
        "{}\nstate_file = {:?}\n",
        TEST_DEVICE
            .lines()
            .filter(|l| !l.starts_with("state_file"))
            .map(|l| {
                // 10 ms, not the test device's 1 ms: a CI runner (and a dev
                // box) schedules a normal-priority thread over 1 ms late now
                // and then, which restarts the TX schedule (dev1, 30.10:
                // one restart and a 192-frame dropout in a second). This
                // test proves the path; real-time timing is the hardware
                // gate's.
                if l.starts_with("latency_ns") {
                    "latency_ns = 10000000"
                } else {
                    l
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        state.display().to_string()
    );
    let path = dir.join("device.toml");
    std::fs::write(&path, &text).unwrap();
    let cfg = DeviceConfig::from_toml(&text).unwrap();
    let mut saved = DeviceState::from_config(&cfg);
    saved.subscriptions = vec![None; cfg.rx.len()];
    saved.subscriptions[0] = Some(Subscription {
        tx_channel: cfg.tx[0].clone(),
        tx_device: cfg.name.clone(),
    });
    saved.save(&state).unwrap();
    path
}

/// The box's IPv4 address on its default route: the adapter a runner
/// really uses (not a virtual or down one). Connecting a UDP socket picks
/// the route and sends nothing.
fn routed_ipv4() -> Ipv4Addr {
    let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("a UDP socket");
    probe
        .connect((Ipv4Addr::new(192, 0, 2, 1), 9))
        .expect("a default route on this box");
    match probe.local_addr().expect("the routed address") {
        SocketAddr::V4(a) if !a.ip().is_loopback() && !a.ip().is_unspecified() => *a.ip(),
        other => panic!("no routed IPv4 address on this box: {other}"),
    }
}

/// The number after `"key":` in the first object of `json` that starts
/// with `head`, if any.
fn field(json: &str, head: &str, key: &str) -> Option<u64> {
    let at = json.find(head)?;
    let rest = &json[at..];
    let end = rest.find('}')?;
    let obj = &rest[..end];
    let k = format!("\"{key}\":");
    let v = &obj[obj.find(&k)? + k.len()..];
    v.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// What the test compares across the measured second.
struct Counts {
    checked: u64,
    mismatch: u64,
    dropout: u64,
    slips: u64,
    rebootstraps: u64,
    late_bins: Vec<u64>,
}

impl Counts {
    fn of(status: &str) -> Counts {
        let check = |key| field(status, "\"checks\":[{", key).unwrap_or(u64::MAX);
        let at = status
            .find("\"late_bins\":[")
            .map(|i| i + "\"late_bins\":[".len());
        let late_bins = at
            .and_then(|at| status[at..].find(']').map(|end| &status[at..at + end]))
            .map(|list| list.split(',').filter_map(|n| n.parse().ok()).collect())
            .unwrap_or_default();
        Counts {
            checked: field(status, "\"checks\":[{", "checked").unwrap_or(0),
            mismatch: check("mismatch"),
            dropout: check("dropout"),
            slips: check("slips"),
            rebootstraps: field(status, "\"tx\":[{", "rebootstraps").unwrap_or(u64::MAX),
            late_bins,
        }
    }
}

/// The status once RX 1's check has read `frames` frames, or the last one
/// at the bound (or when the device ended).
fn wait_checked(
    media: &dante_stack::daemon::media::Media,
    host: &host::Host,
    frames: u64,
) -> String {
    let start = Instant::now();
    loop {
        let status = ctl::answer("status", media);
        let done = field(&status, "\"checks\":[{", "checked").unwrap_or(0) >= frames;
        if done || start.elapsed() > BOUND || host.ended().is_some() {
            return status;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn our_tx_reaches_our_rx_clean_on_the_system_clock() {
    let dir = tempfile::tempdir().unwrap();
    let ip = routed_ipv4();
    let opts = Options {
        config: device_in(dir.path()),
        place: Place::Ip(ip),
        control: dir.path().join("control.sock"),
    };
    let prepared = host::prepare_with(&opts, &[], ClockSource::System).expect("the device set up");
    let media = prepared.media();
    // Ready at once: no dantesync.
    let status = ctl::answer("status", &media);
    assert_eq!(field(&status, "\"clock\":", "ready"), Some(1), "{status}");
    assert!(status.contains("\"source\":\"system\"}"), "{status}");
    assert_eq!(ctl::answer("source 1 counter", &media), "ok");
    assert_eq!(ctl::answer("check 1 counter", &media), "ok");
    let stop = Arc::new(AtomicBool::new(false));
    let host = prepared
        .start(Signals::Report(Arc::clone(&stop)))
        .expect("the device started");
    // The start (threads scheduled for the first time, the flow asked for
    // and granted) is left out: the counts are read once RX 1 has been read
    // for 0.1 s, and the second after that must add nothing bad. A slow
    // runner (the coverage build) restarted the schedule at the start.
    let warm = wait_checked(&media, &host, 9_600);
    let base = Counts::of(&warm);
    let last = wait_checked(&media, &host, base.checked + 96_000);
    let stopped = host.stop(Duration::from_secs(5));
    let end = Counts::of(&last);
    assert!(
        end.checked >= base.checked + 96_000,
        "no second of RX 1 read within {BOUND:?}: {last}"
    );
    assert_eq!(
        (end.mismatch, end.dropout, end.slips),
        (base.mismatch, base.dropout, base.slips),
        "RX 1 after the start: {last}"
    );
    assert_eq!(
        end.rebootstraps, base.rebootstraps,
        "the TX schedule restarted: {last}"
    );
    // Every packet left within the latency (else the schedule would have
    // restarted, checked above, and RX 1 would have dropped out). How late
    // inside it is the machine's real-time behaviour: shown, and judged by
    // the hardware gate, not by a shared runner.
    assert_eq!(end.late_bins.len(), 6, "{last}");
    assert!(
        end.late_bins.iter().sum::<u64>() > base.late_bins.iter().sum::<u64>(),
        "{last}"
    );
    let max_us = field(&last, "\"tx\":[{", "late_max_us").unwrap_or(u64::MAX);
    assert!(
        max_us < 10_000,
        "a TX packet past the 10 ms latency: {last}"
    );
    println!(
        "TX late bins over the measured second (< 0.25, 0.5, 1, 2, 5, >= 5 ms): {:?}",
        end.late_bins
            .iter()
            .zip(&base.late_bins)
            .map(|(e, b)| e - b)
            .collect::<Vec<_>>()
    );
    assert!(matches!(stopped, host::Stopped::Done), "{stopped:?}");
}
