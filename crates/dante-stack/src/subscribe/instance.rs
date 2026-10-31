// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 iemmixer contributors.

//! A transmitter's channel record read by a direct query of its instance
//! (dstack #1): a wireless mic receiver answered no browse (PTR) query for
//! `_netaudio-chan` or `_netaudio-arc` in any form, but answered a query
//! for its instance `<channel>@<device>._netaudio-chan._udp.local.` at
//! once (dev1, 31.10.2026). Without its record the stack read it through
//! ARC: no latency, the config's fpp.
//!
//! - The packets ([`query`], [`host_query`], [`read`]) are built and parsed
//!   with simple-dns: SRV and TXT of the instance, then A of the SRV's
//!   target when the answer carries none. A legacy unicast query (sent
//!   from a port of our own to 224.0.0.251:5353): the answer comes to that
//!   port only, so mdns-sd's own sockets are not involved.
//! - [`Table`] (pure, times in ns): the instances lookups asked for (at
//!   most [`MAX_INSTANCES`]), what each answered, and which to ask now: a
//!   name not answered is asked every [`REQUERY_NS`]; an answer is used for
//!   [`MAX_AGE_NS`] and asked again from half of it while it is looked up.
//! - [`Instances`]: the [`Resolver`] the network resolver asks after the
//!   records and before ARC; its thread ([`Instances::start`]) sends the
//!   queries and reads the answers, on one socket bound to our address.
//!
//! Logs carry counts only, never a name or an address.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use simple_dns::rdata::RData;
use simple_dns::{CLASS, Name, Packet, QCLASS, QTYPE, Question, TYPE};
use tracing::{debug, info};

use super::resolve::{Advertised, Resolver, instance, parse};

/// A name not answered is asked again after this long.
pub const REQUERY_NS: u64 = 10_000_000_000;
/// An answer is used this long; it is asked again from half of it.
pub const MAX_AGE_NS: u64 = 120_000_000_000;
/// Instances kept at most; a lookup of a further one is not asked.
pub const MAX_INSTANCES: usize = 64;
/// A records an unfinished answer keeps at most.
const MAX_HOSTS: usize = 8;
/// The thread's round: queries due are sent, answers read.
pub const ROUND: Duration = Duration::from_millis(250);
/// mDNS's group and port.
pub const MDNS: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(224, 0, 0, 251), 5353);

/// `name` as a DNS name, `None` past DNS's sizes (a label of 63 bytes, a
/// name of 255). Dante names hold spaces and an instance an `@`, which
/// simple-dns's host-name check refuses, so only the sizes are checked.
fn dns_name(name: &str) -> Option<Name<'_>> {
    let name = name.trim_end_matches('.');
    let fits = name.len() < 255 && name.split('.').all(|l| (1..=63).contains(&l.len()));
    fits.then(|| Name::new_unchecked(name))
}

/// A query for `instance`'s SRV and TXT records (a full name, `.local.`
/// at its end), `None` for a name DNS cannot carry.
pub fn query(id: u16, instance: &str) -> Option<Vec<u8>> {
    let name = dns_name(instance)?;
    let mut p = Packet::new_query(id);
    for t in [TYPE::SRV, TYPE::TXT] {
        p.questions.push(Question::new(
            name.clone(),
            QTYPE::TYPE(t),
            QCLASS::CLASS(CLASS::IN),
            false,
        ));
    }
    p.build_bytes_vec().ok()
}

/// A query for `host`'s A record.
pub fn host_query(id: u16, host: &str) -> Option<Vec<u8>> {
    let name = dns_name(host)?;
    let mut p = Packet::new_query(id);
    p.questions.push(Question::new(
        name,
        QTYPE::TYPE(TYPE::A),
        QCLASS::CLASS(CLASS::IN),
        false,
    ));
    p.build_bytes_vec().ok()
}

/// What an answer says about one instance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The SRV's port and target host (lower case, no dot at the end).
    pub srv: Option<(u16, String)>,
    /// The TXT's `key=value` strings.
    pub txt: Option<HashMap<String, String>>,
    /// Each A record of the datagram: host (lower case) and address.
    pub hosts: Vec<(String, Ipv4Addr)>,
}

/// A name as compared: lower case, no dot at the end.
fn key(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

/// What datagram `bytes` says about `instance` (and every A record in it);
/// `None` for a datagram that is no DNS answer.
pub fn read(bytes: &[u8], instance: &str) -> Option<Found> {
    let packet = Packet::parse(bytes).ok()?;
    let want = key(instance);
    let mut found = Found::default();
    for rr in packet.answers.iter().chain(&packet.additional_records) {
        let name = key(&rr.name.to_string());
        match &rr.rdata {
            RData::SRV(srv) if name == want => {
                found.srv = Some((srv.port, key(&srv.target.to_string())));
            }
            RData::TXT(txt) if name == want => {
                found.txt = Some(
                    txt.attributes()
                        .into_iter()
                        .map(|(k, v)| (k, v.unwrap_or_default()))
                        .collect(),
                );
            }
            RData::A(a) => found.hosts.push((name, Ipv4Addr::from(a.address))),
            _ => {}
        }
    }
    Some(found)
}

/// One instance's state.
#[derive(Debug, Clone, Default)]
struct Entry {
    /// What it answered so far (SRV, TXT, its host's address).
    found: Found,
    /// The record, once complete, and when.
    advert: Option<(Advertised, u64)>,
    /// When its last query went out.
    asked: Option<u64>,
    /// When a lookup last wanted it.
    wanted: u64,
}

/// What the thread is to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// SRV and TXT of the instance.
    Instance(String),
    /// A of the instance's target host.
    Host(String, String),
}

/// The instances and their answers (see the module).
#[derive(Debug, Default)]
pub struct Table {
    entries: BTreeMap<String, Entry>,
}

impl Table {
    /// The record of `tx_channel` on `tx_device` if one is fresh at `now`;
    /// otherwise the instance is wanted (and asked when due).
    pub fn lookup(&mut self, tx_device: &str, tx_channel: &str, now: u64) -> Option<Advertised> {
        let name = key(&instance(tx_device, tx_channel));
        if self.entries.len() >= MAX_INSTANCES && !self.entries.contains_key(&name) {
            return None;
        }
        let entry = self.entries.entry(name).or_default();
        entry.wanted = now;
        entry
            .advert
            .filter(|&(_, at)| now.saturating_sub(at) < MAX_AGE_NS)
            .map(|(a, _)| a)
    }

    /// The queries due at `now` (each counted as asked): an instance wanted
    /// within [`MAX_AGE_NS`] and not answered for [`REQUERY_NS`], or
    /// answered more than half of [`MAX_AGE_NS`] ago; the A of a target an
    /// answer left without an address.
    pub fn due(&mut self, now: u64) -> Vec<Ask> {
        let mut out = Vec::new();
        for (name, e) in &mut self.entries {
            if now.saturating_sub(e.wanted) >= MAX_AGE_NS {
                continue;
            }
            let stale = e
                .advert
                .is_none_or(|(_, at)| now.saturating_sub(at) >= MAX_AGE_NS / 2);
            let waited = e.asked.is_none_or(|t| now.saturating_sub(t) >= REQUERY_NS);
            if !(stale && waited) {
                continue;
            }
            e.asked = Some(now);
            match &e.found.srv {
                Some((_, host)) if e.found.txt.is_some() && e.advert.is_none() => {
                    out.push(Ask::Host(name.clone(), host.clone()));
                }
                _ => out.push(Ask::Instance(name.clone())),
            }
        }
        out
    }

    /// Takes datagram `bytes` at `now`: every wanted instance it answers is
    /// updated, and complete once SRV, TXT and its host's address are known
    /// and the TXT reads as a channel record. The number completed.
    pub fn answer(&mut self, bytes: &[u8], now: u64) -> usize {
        let mut completed = 0;
        for (name, e) in &mut self.entries {
            let Some(found) = read(bytes, name) else {
                return 0;
            };
            if found.srv.is_some() {
                e.found.srv = found.srv;
            }
            if found.txt.is_some() {
                e.found.txt = found.txt;
            }
            e.found.hosts.extend(found.hosts);
            // The latest few addresses only (an answer's own A records).
            let extra = e.found.hosts.len().saturating_sub(MAX_HOSTS);
            e.found.hosts.drain(..extra);
            let (Some((port, host)), Some(txt)) = (&e.found.srv, &e.found.txt) else {
                continue;
            };
            let Some(&(_, ip)) = e.found.hosts.iter().rev().find(|(h, _)| h == host) else {
                continue;
            };
            match parse(
                |k| txt.get(k).map(String::as_str),
                SocketAddrV4::new(ip, *port),
            ) {
                Ok(a) => {
                    e.advert = Some((a, now));
                    e.found = Found::default();
                    completed += 1;
                }
                Err(err) => debug!(%err, "instance answer: not a channel record"),
            }
        }
        completed
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The direct instance queries of a device (see the module).
#[derive(Debug)]
pub struct Instances {
    table: Arc<Mutex<Table>>,
    stop: Arc<AtomicBool>,
    asked: AtomicBool,
    /// Its start was tried (once, whatever came of it).
    tried: bool,
    thread: Option<JoinHandle<()>>,
    base: Option<Instant>,
}

impl Instances {
    pub fn new() -> Self {
        Self {
            table: Arc::default(),
            stop: Arc::default(),
            asked: AtomicBool::new(false),
            tried: false,
            thread: None,
            base: Some(Instant::now()),
        }
    }

    /// A lookup reached it (the thread is then due).
    pub fn asked(&self) -> bool {
        self.asked.load(Ordering::Relaxed)
    }

    pub fn started(&self) -> bool {
        self.thread.is_some()
    }

    /// Its start was tried: a start that failed is not tried again (the
    /// records and ARC still resolve).
    pub fn tried(&self) -> bool {
        self.tried
    }

    fn now(&self) -> u64 {
        self.base.map_or(0, |b| {
            u64::try_from(b.elapsed().as_nanos()).unwrap_or(u64::MAX)
        })
    }

    /// Starts the thread on a socket bound to our address `ip`, once:
    /// `true` when it started now.
    pub fn start(&mut self, ip: Ipv4Addr) -> io::Result<bool> {
        self.start_to(ip, MDNS)
    }

    /// [`Instances::start`] with the queries sent to `to` (mDNS's group, or
    /// a test's responder).
    pub fn start_to(&mut self, ip: Ipv4Addr, to: SocketAddrV4) -> io::Result<bool> {
        if self.started() {
            return Ok(false);
        }
        self.tried = true;
        let sock = UdpSocket::bind(SocketAddrV4::new(ip, 0))?;
        sock.set_read_timeout(Some(ROUND))?;
        if to.ip().is_multicast() {
            socket2::SockRef::from(&sock).set_multicast_if_v4(&ip)?;
        }
        let (table, stop) = (Arc::clone(&self.table), Arc::clone(&self.stop));
        let base = self.base.unwrap_or_else(Instant::now);
        self.thread = Some(
            std::thread::Builder::new()
                .name("dante-mdns-instance".to_string())
                .spawn(move || run(&sock, to, &table, &stop, base))?,
        );
        Ok(true)
    }
}

impl Default for Instances {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Instances {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Resolver for Instances {
    fn lookup(&self, tx_device: &str, tx_channel: &str) -> Option<Advertised> {
        self.asked.store(true, Ordering::Relaxed);
        let now = self.now();
        lock(&self.table).lookup(tx_device, tx_channel, now)
    }
}

/// The thread: the due queries every [`ROUND`], the answers in between.
fn run(sock: &UdpSocket, to: SocketAddrV4, table: &Mutex<Table>, stop: &AtomicBool, base: Instant) {
    let now = || u64::try_from(base.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let mut id: u16 = 0;
    let mut buf = vec![0u8; 9000];
    let (mut sent, mut answered) = (0u64, 0u64);
    while !stop.load(Ordering::Relaxed) {
        let asks = lock(table).due(now());
        for ask in asks {
            id = id.wrapping_add(1);
            let bytes = match &ask {
                Ask::Instance(name) => query(id, name),
                Ask::Host(_, host) => host_query(id, host),
            };
            if let Some(bytes) = bytes {
                match sock.send_to(&bytes, to) {
                    Ok(_) => sent += 1,
                    Err(e) => debug!(%e, "instance query not sent"),
                }
            }
        }
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => {
                let done = lock(table).answer(&buf[..n], now());
                if done > 0 {
                    answered += done as u64;
                    info!(
                        sent,
                        answered, "a channel record read by a direct instance query"
                    );
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(e) => debug!(%e, "instance answer not read"),
        }
    }
}

#[cfg(test)]
mod tests;
