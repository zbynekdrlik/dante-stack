//! The info requests (UDP 8700): one datagram, answered for the info group.
#![no_main]

use std::net::Ipv4Addr;

use dante_stack::device::DeviceConfig;
use dante_stack::info::{Identity, Multicaster, NetInfo};
use libfuzzer_sys::fuzz_target;

const DEVICE: &str = include_str!("../../config/test-dante-device.toml");

fuzz_target!(|data: &[u8]| {
    let cfg = DeviceConfig::from_toml(DEVICE).expect("the test device");
    let net = NetInfo {
        mac: [2, 0, 0, 0, 0, 1],
        ip: Ipv4Addr::new(10, 0, 0, 1),
        netmask: Ipv4Addr::new(255, 255, 255, 0),
        gateway: Ipv4Addr::UNSPECIFIED,
        link_speed: 1_000,
    };
    let mut m = Multicaster::new(Identity::new(&cfg, net));
    let _ = m.answer(data);
});
