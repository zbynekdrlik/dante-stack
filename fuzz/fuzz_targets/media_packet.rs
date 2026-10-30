//! A media packet from a transmitter: its header, then its frames read as
//! the slot count and width in the first two bytes ask.
#![no_main]

use dante_stack::media::packet;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&[slots, bps], buf)) = data.split_first_chunk::<2>() else {
        return;
    };
    let _ = packet::read_header(buf);
    let _ = packet::read_frames(buf, usize::from(slots), usize::from(bps), |_, _, _| {});
});
