//! A whole device on the test-only system clock (dante-asio's CI E2E: a
//! GitHub-hosted runner has no dantesync and no PTP network). The real
//! host is prepared on the box's first non-loopback IPv4 address (the
//! production checks stay: no loopback, a real MAC) with
//! `ClockSource::System` and a state
//! file that subscribes RX 1 to our own TX 1 (resolved from our own
//! records, no mDNS); it starts its threads as in production. TX 1 sends
//! the counter and RX 1's counter check must read clean within the bound,
//! with no TX packet 1 ms past due. Only our own device is subscribed.

#![cfg(any(target_os = "linux", windows))]

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

#[test]
fn our_tx_reaches_our_rx_clean_on_the_system_clock() {
    let dir = tempfile::tempdir().unwrap();
    let ip = host::interfaces()
        .expect("the box's interfaces")
        .into_iter()
        .map(|a| a.ip)
        .find(|ip| !ip.is_loopback() && !ip.is_link_local())
        .expect("a non-loopback IPv4 address on this box");
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
    assert_eq!(ctl::answer("source 1 counter", &media), "ok");
    assert_eq!(ctl::answer("check 1 counter", &media), "ok");
    let stop = Arc::new(AtomicBool::new(false));
    let host = prepared
        .start(Signals::Report(Arc::clone(&stop)))
        .expect("the device started");
    let start = Instant::now();
    let (clean, last) = loop {
        let last = ctl::answer("status", &media);
        let checked = field(&last, "\"checks\":[{", "checked").unwrap_or(0);
        if checked >= 96_000 {
            break (true, last);
        }
        if start.elapsed() > BOUND || host.ended().is_some() {
            break (false, last);
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stopped = host.stop(Duration::from_secs(5));
    assert!(clean, "no second of RX 1 read within {BOUND:?}: {last}");
    for key in ["mismatch", "dropout", "slips"] {
        assert_eq!(field(&last, "\"checks\":[{", key), Some(0), "{key}: {last}");
    }
    // Late bins: < 0.25, < 0.5, < 1 ms, then the ones that count.
    let at = last.find("\"late_bins\":[").expect("the TX lane") + "\"late_bins\":[".len();
    let bins: Vec<u64> = last[at..last[at..].find(']').unwrap() + at]
        .split(',')
        .map(|n| n.parse().unwrap())
        .collect();
    assert_eq!(bins.len(), 6, "{last}");
    assert!(bins[..3].iter().sum::<u64>() > 0, "{last}");
    assert_eq!(bins[3..], [0, 0, 0], "a TX packet 1 ms past due: {last}");
    assert!(matches!(stopped, host::Stopped::Done), "{stopped:?}");
}
