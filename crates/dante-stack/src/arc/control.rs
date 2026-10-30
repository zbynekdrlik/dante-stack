// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 iemmixer contributors.

//! Control of another device's ARC (UDP 4440): what an application asks
//! of a device it was told to manage (the IEM card's labels, the web
//! controller): its name, its channels' labels, and the renames.
//!
//! The requests are netaudio's (v0.0.10, `netaudio/dante/device.py`), byte
//! for byte; our own server ([`super::handle`]) answers them, so every
//! request here is tested against it:
//!
//! - 0x1002, the device's name: no body; the reply's body is the name,
//!   NUL-ended.
//! - 0x1000, the counts: no body; TX at body bytes 2–3, RX at 4–5.
//! - 0x3000, the RX channels, paged ([`super::client::page_request`]):
//!   `[slots, count]`, then `slots` 20-byte descriptors `id, flags,
//!   common, tx channel, tx device, name, status (2), 0, 0`; offsets count
//!   from the datagram's first byte.
//! - The TX labels: the 0x2010 own name, else the 0x2000 default
//!   ([`super::client::read_directory`]), as netaudio's `friendly_name or
//!   name`.
//! - 0x3001 (RX) and 0x2013 (TX), a channel's label ([`rename_request`]):
//!   RX body `02 01 <channel u16> <name offset 20> 00000000 <name> 00`, TX
//!   `02 01 0000 <channel u16> <name offset 24> 000000000000 <name> 00`;
//!   an empty name resets the label to the channel's default.
//! - 0x1001, the device's name ([`device_rename_request`]): body `<name>
//!   00`. Our own server does not answer it (inferno does not).
//!
//! Nothing here changes anything but what the caller names, on the one
//! device it talks to; whether a device may be changed at all is the
//! application's call (the owner's rule: no foreign device without the
//! owner).

use std::collections::BTreeMap;

use tracing::debug;

use super::client::{Directory, Exchange, ReadError, START, page_request, read_directory};
use super::op;
use crate::wire::{HEADER_LEN, STATUS_MORE, STATUS_OK, read_u16, request, string_at};

/// The device's own name (netaudio's `device_name`).
pub const GET_NAME: u16 = 0x1002;
/// The device's rename (netaudio's `set_name`).
pub const SET_NAME: u16 = 0x1001;
/// A 0x3000 descriptor's size.
const RX_ITEM: usize = 20;
/// The most pages an RX list is read in (512 channels at 16 a page).
const MAX_PAGES: usize = 32;

/// A channel's direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Tx,
    Rx,
}

/// One exchange of a whole request body: the reply's status and body.
pub trait Call {
    fn call(&mut self, op1: u16, body: &[u8]) -> Result<(u16, Vec<u8>), ReadError>;
}

/// A [`Call`] read page by page, as the directory reader asks.
struct Pages<'a, C: Call>(&'a mut C);

impl<C: Call> Exchange for Pages<'_, C> {
    fn ask(&mut self, op1: u16, start: u16) -> Result<(u16, Vec<u8>), ReadError> {
        let page = page_request(0, op1, start);
        self.0.call(op1, &page[HEADER_LEN..])
    }
}

/// netaudio's request for a channel's label: `label` `None` resets it.
pub fn rename_request(seq: u16, dir: Dir, channel: u16, label: Option<&str>) -> Vec<u8> {
    let [hi, lo] = channel.to_be_bytes();
    let mut body = match dir {
        Dir::Rx => vec![2, 1, hi, lo, 0, 20, 0, 0, 0, 0],
        Dir::Tx => vec![2, 1, 0, 0, hi, lo, 0, 24, 0, 0, 0, 0, 0, 0],
    };
    body.extend_from_slice(label.unwrap_or("").as_bytes());
    body.push(0);
    let op1 = match dir {
        Dir::Rx => op::RX_RENAME,
        Dir::Tx => op::TX_RENAME,
    };
    request(START, seq, op1, 0, &body)
}

/// netaudio's request for the device's new name.
pub fn device_rename_request(seq: u16, name: &str) -> Vec<u8> {
    let mut body = name.as_bytes().to_vec();
    body.push(0);
    request(START, seq, SET_NAME, 0, &body)
}

/// The request body of `datagram` (built by one of the requests above).
fn body_of(datagram: &[u8]) -> &[u8] {
    &datagram[HEADER_LEN..]
}

/// The device's own name (a reply with an empty name does not read).
pub fn device_name(c: &mut impl Call) -> Result<String, ReadError> {
    let (_, body) = ok(c.call(GET_NAME, &[])?, GET_NAME)?;
    let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    let name =
        std::str::from_utf8(&body[..end]).map_err(|_| ReadError::Malformed { op: GET_NAME })?;
    if name.is_empty() {
        return Err(ReadError::Malformed { op: GET_NAME });
    }
    Ok(name.to_string())
}

/// A reply with status OK, else the refusal.
fn ok(reply: (u16, Vec<u8>), op: u16) -> Result<(u16, Vec<u8>), ReadError> {
    if reply.0 == STATUS_OK {
        Ok(reply)
    } else {
        Err(ReadError::Refused {
            op,
            status: reply.0,
        })
    }
}

/// A device's channel counts and labels, by channel number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labels {
    pub tx_count: usize,
    pub rx_count: usize,
    pub tx: BTreeMap<u16, String>,
    pub rx: BTreeMap<u16, String>,
}

impl Labels {
    /// Every channel the counts promise has a label.
    pub fn complete(&self) -> bool {
        self.tx_count > 0
            && self.rx_count > 0
            && self.tx.len() == self.tx_count
            && self.rx.len() == self.rx_count
    }

    /// The label of `dir`'s channel `channel`.
    pub fn get(&self, dir: Dir, channel: u16) -> Option<&str> {
        match dir {
            Dir::Tx => self.tx.get(&channel),
            Dir::Rx => self.rx.get(&channel),
        }
        .map(String::as_str)
    }
}

/// The device's counts and every channel's label.
pub fn labels(c: &mut impl Call) -> Result<Labels, ReadError> {
    let (_, counts) = ok(c.call(op::COUNTS, &[])?, op::COUNTS)?;
    let malformed = ReadError::Malformed { op: op::COUNTS };
    let tx_count = usize::from(read_u16(&counts, 2).ok_or(malformed)?);
    let rx_count = usize::from(read_u16(&counts, 4).ok_or(malformed)?);
    let directory: Directory = read_directory(&mut Pages(c))?;
    let tx = directory
        .channels()
        .iter()
        .map(|ch| (ch.id, ch.name.clone().unwrap_or_else(|| ch.default.clone())))
        .collect();
    let rx = rx_names(c, rx_count)?;
    Ok(Labels {
        tx_count,
        rx_count,
        tx,
        rx,
    })
}

/// The RX channels' labels of a device with `total` RX channels.
fn rx_names(c: &mut impl Call, total: usize) -> Result<BTreeMap<u16, String>, ReadError> {
    let mut out = BTreeMap::new();
    let mut counted = 0;
    let mut start: u16 = 1;
    for _ in 0..MAX_PAGES {
        if counted >= total {
            break;
        }
        let page = page_request(0, op::RX_CHANNELS, start);
        let (status, body) = c.call(op::RX_CHANNELS, body_of(&page))?;
        if status != STATUS_OK && status != STATUS_MORE {
            return Err(ReadError::Refused {
                op: op::RX_CHANNELS,
                status,
            });
        }
        let malformed = ReadError::Malformed {
            op: op::RX_CHANNELS,
        };
        let (&slots, &count) = (
            body.first().ok_or(malformed)?,
            body.get(1).ok_or(malformed)?,
        );
        let (slots, count) = (usize::from(slots), usize::from(count));
        if count > slots || 2 + slots * RX_ITEM > body.len() {
            return Err(malformed);
        }
        for i in 0..count {
            let at = 2 + i * RX_ITEM;
            let id = read_u16(&body, at).filter(|&id| id > 0);
            let name = read_u16(&body, at + 10).and_then(|off| string_at(&body, off));
            if let (Some(id), Some(name)) = (id, name.filter(|n| !n.is_empty())) {
                out.insert(id, name.to_string());
            }
        }
        counted += count;
        if count == 0 || !(status == STATUS_MORE || count == slots) {
            break;
        }
        match u16::try_from(usize::from(start) + count) {
            Ok(next) => start = next,
            Err(_) => break,
        }
    }
    Ok(out)
}

/// Sets `dir`'s channel `channel` to `label` (`None`: its default).
pub fn rename_channel(
    c: &mut impl Call,
    dir: Dir,
    channel: u16,
    label: Option<&str>,
) -> Result<(), ReadError> {
    debug!(
        ?dir,
        channel,
        reset = label.is_none(),
        "ARC: a channel's label sent"
    );
    let req = rename_request(0, dir, channel, label);
    let op1 = match dir {
        Dir::Rx => op::RX_RENAME,
        Dir::Tx => op::TX_RENAME,
    };
    ok(c.call(op1, body_of(&req))?, op1).map(|_| ())
}

/// Renames the device to `name`.
pub fn rename_device(c: &mut impl Call, name: &str) -> Result<(), ReadError> {
    debug!("ARC: the device's new name sent");
    let req = device_rename_request(0, name);
    ok(c.call(SET_NAME, body_of(&req))?, SET_NAME).map(|_| ())
}

#[cfg(test)]
pub(crate) mod tests;
