//! dantesync's time server reply (DSYX), read for the request id in the
//! first four bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((id, reply)) = data.split_first_chunk::<4>() else {
        return;
    };
    let _ = dante_stack::clock::dsyx::parse(reply, u32::from_be_bytes(*id));
});
