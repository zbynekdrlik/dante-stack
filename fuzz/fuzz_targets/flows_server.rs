//! Flow control (UDP 4455): one datagram from a receiver, answered from
//! the test device's TX table.
#![no_main]

use std::net::{Ipv4Addr, SocketAddrV4};

use dante_stack::device::DeviceConfig;
use dante_stack::flows::server;
use dante_stack::media::tx::TxTable;
use libfuzzer_sys::fuzz_target;

const DEVICE: &str = include_str!("../../config/test-dante-device.toml");

fuzz_target!(|data: &[u8]| {
    let cfg = DeviceConfig::from_toml(DEVICE).expect("the test device");
    let mut table = TxTable::new(1);
    let from = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 11), 4455);
    let _ = server::handle(&mut table, &cfg, from, data, 1_000_000_000);
});
