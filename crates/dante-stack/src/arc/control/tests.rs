use super::*;

use std::net::Ipv4Addr;

use crate::arc::{ArcOutcome, Live, handle};
use crate::device::{DeviceConfig, DeviceState};
use crate::media::tx::TxTable;
use crate::subscribe::{MAX_FLOWS, Manager};
use crate::wire::Header;

const TEST_DEVICE: &str = include_str!("../../../../../config/test-dante-device.toml");

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- the requests, byte for byte as netaudio v0.0.10 sends them ----

#[test]
fn an_rx_label_is_netaudio_s_request() {
    // netaudio: "27ff 00{len} {seq} 3001 0000 0201 00{ch} 0014 00000000
    // {name} 00", len = name + 21.
    let want = [
        "27ff", "0018", "0000", "3001", "0000", "0201", "0005", "0014", "00000000", "566f78", "00",
    ];
    assert_eq!(
        hex(&rename_request(0, Dir::Rx, 5, Some("Vox"))),
        want.concat()
    );
    let reset = [
        "27ff", "0015", "0000", "3001", "0000", "0201", "0005", "0014", "00000000", "00",
    ];
    assert_eq!(hex(&rename_request(0, Dir::Rx, 5, None)), reset.concat());
}

#[test]
fn a_tx_label_is_netaudio_s_request() {
    // netaudio: "... 2013 0000 0201 0000 00{ch} 0018 000000000000 {name}
    // 00", len = name + 25.
    let want = [
        "27ff",
        "001e",
        "0000",
        "2013",
        "0000",
        "0201",
        "0000",
        "0007",
        "0018",
        "000000000000",
        "436c69636b",
        "00",
    ];
    assert_eq!(
        hex(&rename_request(0, Dir::Tx, 7, Some("Click"))),
        want.concat()
    );
    let reset = [
        "27ff",
        "0019",
        "0000",
        "2013",
        "0000",
        "0201",
        "0000",
        "0007",
        "0018",
        "000000000000",
        "00",
    ];
    assert_eq!(hex(&rename_request(0, Dir::Tx, 7, None)), reset.concat());
}

#[test]
fn a_device_name_is_netaudio_s_request() {
    // netaudio: "27ff 00{len} {seq} 1001 0000 {name} 00", len = name + 11.
    let want = [
        "27ff",
        "0013",
        "0000",
        "1001",
        "0000",
        &hex(b"iem-card"),
        "00",
    ];
    assert_eq!(hex(&device_rename_request(0, "iem-card")), want.concat());
}

// ---- against our own server ----

/// Our own device answering in process: its config, state and flows.
pub(crate) struct Own {
    pub(crate) cfg: DeviceConfig,
    pub(crate) state: DeviceState,
    tx: TxTable,
    rx: Manager,
    seq: u16,
}

impl Own {
    pub(crate) fn new() -> Self {
        let cfg = DeviceConfig::from_toml(TEST_DEVICE).expect("the test device");
        let rx = Manager::new(&cfg, &cfg.name, Ipv4Addr::new(10, 0, 0, 10), [0; MAX_FLOWS]);
        Own {
            state: DeviceState::from_config(&cfg),
            tx: TxTable::new(7),
            rx,
            cfg,
            seq: 0,
        }
    }
}

impl Call for Own {
    fn call(&mut self, op1: u16, body: &[u8]) -> Result<(u16, Vec<u8>), ReadError> {
        self.seq = self.seq.wrapping_add(1);
        let req = request(START, self.seq, op1, 0, body);
        let live = Live {
            tx: &self.tx,
            rx: &self.rx,
        };
        let bytes = match handle(&mut self.state, &self.cfg, &live, &req) {
            ArcOutcome::Reply(b) | ArcOutcome::Changed(b, _) => b,
            // No reply: as a device that does not answer.
            _ => return Err(ReadError::Timeout { op: op1 }),
        };
        let (h, body) = Header::parse(&bytes).expect("a reply parses");
        assert_eq!((h.seq, h.op1), (self.seq, op1), "the reply to our request");
        Ok((h.op2, body.to_vec()))
    }
}

#[test]
fn our_device_s_name_and_labels_read_back() {
    let mut own = Own::new();
    assert_eq!(device_name(&mut own).unwrap(), own.cfg.name);
    let got = labels(&mut own).unwrap();
    assert_eq!(
        (got.tx_count, got.rx_count),
        (own.cfg.tx.len(), own.cfg.rx.len())
    );
    assert!(got.complete(), "{got:?}");
    for (i, name) in own.cfg.tx.iter().enumerate() {
        assert_eq!(got.get(Dir::Tx, i as u16 + 1), Some(name.as_str()));
    }
    for (i, name) in own.cfg.rx.iter().enumerate() {
        assert_eq!(got.get(Dir::Rx, i as u16 + 1), Some(name.as_str()));
    }
}

#[test]
fn a_label_set_and_reset_reads_back() {
    let mut own = Own::new();
    rename_channel(&mut own, Dir::Rx, 2, Some("Vox L")).unwrap();
    rename_channel(&mut own, Dir::Tx, 3, Some("Click")).unwrap();
    assert_eq!(own.state.rx[1], "Vox L");
    assert_eq!(own.state.tx[2], "Click");
    let got = labels(&mut own).unwrap();
    assert_eq!(got.get(Dir::Rx, 2), Some("Vox L"));
    assert_eq!(got.get(Dir::Tx, 3), Some("Click"));
    // Reset: the default comes back.
    rename_channel(&mut own, Dir::Rx, 2, None).unwrap();
    rename_channel(&mut own, Dir::Tx, 3, None).unwrap();
    let got = labels(&mut own).unwrap();
    assert_eq!(got.get(Dir::Rx, 2), Some(own.cfg.rx[1].as_str()));
    assert_eq!(got.get(Dir::Tx, 3), Some(own.cfg.tx[2].as_str()));
}

#[test]
fn a_refused_label_is_a_refusal() {
    let mut own = Own::new();
    let past = own.cfg.rx.len() as u16 + 1;
    assert!(matches!(
        rename_channel(&mut own, Dir::Rx, past, Some("X")),
        Err(ReadError::Refused {
            op: op::RX_RENAME,
            ..
        })
    ));
    assert!(matches!(
        rename_channel(&mut own, Dir::Tx, 1, Some("no.dots")),
        Err(ReadError::Refused {
            op: op::TX_RENAME,
            ..
        })
    ));
}

#[test]
fn our_device_does_not_answer_a_device_rename() {
    // Inferno and our server leave 0x1001 unanswered: no reply, as a
    // device that does not support it.
    let mut own = Own::new();
    assert_eq!(
        rename_device(&mut own, "other-name"),
        Err(ReadError::Timeout { op: SET_NAME })
    );
    assert_eq!(device_name(&mut own).unwrap(), own.cfg.name);
}

/// A device whose name reply is empty, or that refuses.
struct Fixed(u16, Vec<u8>);

impl Call for Fixed {
    fn call(&mut self, _: u16, _: &[u8]) -> Result<(u16, Vec<u8>), ReadError> {
        Ok((self.0, self.1.clone()))
    }
}

#[test]
fn an_empty_or_refused_name_does_not_read() {
    assert_eq!(
        device_name(&mut Fixed(STATUS_OK, vec![0])),
        Err(ReadError::Malformed { op: GET_NAME })
    );
    assert_eq!(
        device_name(&mut Fixed(0x0022, Vec::new())),
        Err(ReadError::Refused {
            op: GET_NAME,
            status: 0x0022
        })
    );
}
