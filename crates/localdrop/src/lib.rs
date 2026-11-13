//! LocalDrop: hands files from the desktop app to Zeidex Convert on a phone over the local
//! network. Opt-in and LAN-only: nothing leaves the computer unless the user picks "Send to
//! phone", scans the code with the phone, and accepts the file list there. Conversions never
//! use this crate; `zeidex-core` still opens no socket.
//!
//! Pairing travels in the QR code: `zeidexdrop://v1?h=<ip>&p=<port>&k=<64 hex>&n=<name>`.
//! The 256-bit key `k` exists only in that code and in memory, so a device on the same network
//! that did not see the screen can neither read nor alter the transfer.
//!
//! Wire format, both directions: frames of a `u32` big-endian length followed by AES-256-GCM
//! ciphertext with its 16-byte tag. Nonces are implicit — `[direction, 0, 0, 0, counter]`
//! with the counter as a big-endian `u64` counting frames per direction from 0 — so a dropped,
//! replayed or reordered frame fails authentication. Direction 1 is sender → receiver, 2 is
//! receiver → sender.
//!
//! The conversation:
//! 1. receiver: `ZDROP/1 HELLO\n<device name>` (proves it holds the key)
//! 2. sender: `ZDROP/1 OFFER <count> <bytes>\n`, then `<size> <sha256> <name>\n` per file
//! 3. receiver: `ZDROP/1 ACCEPT` or `ZDROP/1 DECLINE`
//! 4. sender: the bytes of every file in order (no frame spans two files), then `ZDROP/1 END`
//! 5. receiver: `ZDROP/1 DONE <count>` or `ZDROP/1 FAIL <reason>`
//!
//! The Android receiver (`LocalDropReceiver.kt`) implements the same rules; the shared test
//! vector in this file's tests pins the two to identical bytes.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// URI scheme of a pairing link.
pub const SCHEME: &str = "zeidexdrop";
/// Bytes in the session key.
pub const KEY_LEN: usize = 32;
const MAGIC: &str = "ZDROP/1";
const TAG_LEN: usize = 16;
/// Largest plaintext of one frame (the offer is the only frame that can come close).
const MAX_PLAIN: usize = 1 << 20;
/// Plaintext bytes per data frame.
const CHUNK: usize = 256 * 1024;
/// Files per offer; keeps the offer frame under [`MAX_PLAIN`] even with long names.
pub const MAX_FILES: usize = 2_000;
const TO_RECEIVER: u8 = 1;
const TO_SENDER: u8 = 2;
/// A connection must prove the key this fast, or it is treated as a stranger.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Time the person on the phone has to read the file list and decide.
const DECISION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Longest silence while bytes are flowing.
const IO_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the waiting sender checks for a connection, cancellation or the deadline.
const POLL: Duration = Duration::from_millis(100);
/// Connections without the key before the sender gives up (someone is probing the port).
const MAX_STRANGERS: usize = 10;
/// Longest device or file name shown and stored.
const MAX_NAME: usize = 200;
const MAX_DEVICE_NAME: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    fn new(msg: impl Into<String>) -> Self {
        Error(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Error::new("the other device stopped responding"),
            io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe => {
                Error::new("the connection was closed")
            }
            _ => Error(e.to_string()),
        }
    }
}

impl From<zeidex_core::Error> for Error {
    fn from(e: zeidex_core::Error) -> Self {
        Error(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// ---------- pairing link ----------

/// Everything the phone needs to find the computer and talk to it privately.
#[derive(Clone, PartialEq, Eq)]
pub struct Link {
    pub addr: SocketAddr,
    pub key: [u8; KEY_LEN],
    /// The sending computer's name, shown on the phone.
    pub name: String,
}

impl fmt::Debug for Link {
    // Never print the key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Link").field("addr", &self.addr).field("name", &self.name).finish_non_exhaustive()
    }
}

impl Link {
    pub fn to_uri(&self) -> String {
        format!(
            "{SCHEME}://v1?h={}&p={}&k={}&n={}",
            self.addr.ip(),
            self.addr.port(),
            hex(&self.key),
            clean_device_name(&self.name)
        )
    }

    /// Parses and checks a scanned link: the address must be on a local network.
    pub fn parse(uri: &str) -> Result<Link> {
        let query = uri
            .strip_prefix(SCHEME)
            .and_then(|r| r.strip_prefix("://v1?"))
            .ok_or_else(|| Error::new("not a LocalDrop code"))?;
        let (mut host, mut port, mut key, mut name) = (None, None, None, String::new());
        for pair in query.split('&') {
            match pair.split_once('=') {
                Some(("h", v)) => host = v.parse::<IpAddr>().ok(),
                Some(("p", v)) => port = v.parse::<u16>().ok().filter(|p| *p != 0),
                Some(("k", v)) => key = unhex(v),
                Some(("n", v)) => name = clean_device_name(v),
                _ => {}
            }
        }
        let (host, port, key) = match (host, port, key) {
            (Some(h), Some(p), Some(k)) => (h, p, k),
            _ => return Err(Error::new("the LocalDrop code is incomplete")),
        };
        if !is_lan(host) {
            return Err(Error::new("LocalDrop only connects to devices on the local network"));
        }
        Ok(Link { addr: SocketAddr::new(host, port), key, name })
    }
}

/// Private, link-local and loopback addresses: the only places a LocalDrop listener lives.
pub fn is_lan(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// The address this computer uses on its local network.
pub fn lan_ip() -> Result<IpAddr> {
    // Ask the OS which local address routes outwards. A UDP connect only picks the route; it
    // sends nothing, and 192.0.2.1 (TEST-NET-1) would never answer anyway.
    let probe = UdpSocket::bind(("0.0.0.0", 0))?;
    probe
        .connect(("192.0.2.1", 9))
        .map_err(|_| Error::new("no network connection: join a Wi-Fi or Ethernet network first"))?;
    let ip = probe.local_addr()?.ip();
    if is_lan(ip) {
        Ok(ip)
    } else {
        Err(Error::new("this computer has no local network address (LocalDrop never listens on the internet)"))
    }
}

/// This computer's name for the phone's confirmation screen.
pub fn this_device_name() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| fs::read_to_string("/etc/hostname").ok())
        .unwrap_or_default();
    let name = clean_device_name(raw.trim());
    if name.is_empty() {
        "Zeidex-desktop".into()
    } else {
        name
    }
}

/// Device names travel inside the link: letters, digits, `.`, `_` and `-` only.
fn clean_device_name(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' })
        .take(MAX_DEVICE_NAME)
        .collect()
}

/// A file name that is only a name on every system the file can land on: no separators,
/// no control or reserved characters, never `.` or `..`, at most [`MAX_NAME`] characters.
pub fn clean_file_name(raw: &str) -> String {
    let mapped: String =
        raw.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let trimmed = mapped.trim().trim_end_matches(['.', ' ']);
    let mut name: String = trimmed.chars().take(MAX_NAME).collect();
    if name.is_empty() || name == "." || name == ".." {
        name = "file".into();
    }
    name
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

// ---------- encrypted channel ----------

struct Channel {
    stream: TcpStream,
    key: LessSafeKey,
    send_dir: u8,
    recv_dir: u8,
    sent: u64,
    received: u64,
}

impl Channel {
    fn new(stream: TcpStream, key: &[u8; KEY_LEN], sender: bool) -> Result<Self> {
        let key = UnboundKey::new(&AES_256_GCM, key).map_err(|_| Error::new("bad session key"))?;
        let (send_dir, recv_dir) = if sender { (TO_RECEIVER, TO_SENDER) } else { (TO_SENDER, TO_RECEIVER) };
        Ok(Channel { stream, key: LessSafeKey::new(key), send_dir, recv_dir, sent: 0, received: 0 })
    }

    fn send(&mut self, plain: &[u8]) -> Result<()> {
        let frame = seal(&self.key, self.send_dir, self.sent, plain)?;
        self.sent += 1;
        self.stream.write_all(&frame)?;
        Ok(())
    }

    fn recv(&mut self) -> Result<Vec<u8>> {
        let mut len = [0u8; 4];
        self.stream.read_exact(&mut len)?;
        let len = u32::from_be_bytes(len) as usize;
        if !(TAG_LEN..=MAX_PLAIN + TAG_LEN).contains(&len) {
            return Err(Error::new("malformed LocalDrop frame"));
        }
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf)?;
        let plain_len = self
            .key
            .open_in_place(nonce(self.recv_dir, self.received), Aad::empty(), &mut buf)
            .map_err(|_| Error::new("the other device does not hold this code's key, or the data was altered"))?
            .len();
        self.received += 1;
        buf.truncate(plain_len);
        Ok(buf)
    }

    fn recv_text(&mut self) -> Result<String> {
        String::from_utf8(self.recv()?).map_err(|_| Error::new("malformed LocalDrop message"))
    }

    fn set_read_timeout(&self, t: Duration) -> Result<()> {
        Ok(self.stream.set_read_timeout(Some(t))?)
    }
}

fn nonce(dir: u8, counter: u64) -> Nonce {
    let mut n = [0u8; NONCE_LEN];
    n[0] = dir;
    n[4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::assume_unique_for_key(n)
}

/// One frame on the wire: length prefix + ciphertext + tag.
fn seal(key: &LessSafeKey, dir: u8, counter: u64, plain: &[u8]) -> Result<Vec<u8>> {
    if plain.len() > MAX_PLAIN {
        return Err(Error::new("LocalDrop frame too large"));
    }
    let mut buf = plain.to_vec();
    key.seal_in_place_append_tag(nonce(dir, counter), Aad::empty(), &mut buf)
        .map_err(|_| Error::new("encryption failed"))?;
    let mut frame = Vec::with_capacity(4 + buf.len());
    frame.extend_from_slice(&(buf.len() as u32).to_be_bytes());
    frame.extend_from_slice(&buf);
    Ok(frame)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut ctx = digest::Context::new(&digest::SHA256);
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    Ok(hex(ctx.finish().as_ref()))
}

// ---------- sending (desktop) ----------

/// A file offered to the phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// What the waiting sender reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Waiting,
    Connected { device: String },
    Sending { sent: u64, total: u64 },
}

/// How a session ended without an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Delivered { device: String, files: usize, bytes: u64 },
    Declined { device: String },
}

/// A one-time listener with its key and the files it will hand over.
pub struct Offer {
    listener: TcpListener,
    link: Link,
    files: Vec<Outgoing>,
}

/// Prepares to send `paths` from this computer's LAN address.
pub fn offer(paths: &[PathBuf]) -> Result<Offer> {
    offer_on(lan_ip()?, paths, &this_device_name())
}

/// [`offer`] on a given local address (tests use loopback).
pub fn offer_on(ip: IpAddr, paths: &[PathBuf], device_name: &str) -> Result<Offer> {
    if !is_lan(ip) {
        return Err(Error::new("LocalDrop only listens on a local network address"));
    }
    if paths.is_empty() {
        return Err(Error::new("no files to send"));
    }
    if paths.len() > MAX_FILES {
        return Err(Error::new(format!("LocalDrop sends at most {MAX_FILES} files at once")));
    }
    let mut files = Vec::with_capacity(paths.len());
    for p in paths {
        let meta = fs::metadata(p)?;
        if !meta.is_file() {
            return Err(Error::new(format!(
                "{} is a folder: archive it to ZIP first, then send the archive",
                p.display()
            )));
        }
        let raw = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        files.push(Outgoing {
            path: p.clone(),
            name: clean_file_name(&raw),
            size: meta.len(),
            sha256: sha256_file(p)?,
        });
    }
    let mut key = [0u8; KEY_LEN];
    SystemRandom::new().fill(&mut key).map_err(|_| Error::new("no secure random source"))?;
    let listener = TcpListener::bind((ip, 0))?;
    let addr = listener.local_addr()?;
    Ok(Offer { listener, link: Link { addr, key, name: clean_device_name(device_name) }, files })
}

/// Why one incoming connection did not become the session.
enum Fail {
    /// It never proved the key: keep waiting for the real phone.
    Stranger,
    /// The phone was there and something went wrong: the offer is spent.
    Fatal(Error),
}

impl From<Error> for Fail {
    fn from(e: Error) -> Self {
        Fail::Fatal(e)
    }
}

impl Offer {
    pub fn link(&self) -> &Link {
        &self.link
    }

    pub fn files(&self) -> &[Outgoing] {
        &self.files
    }

    /// The pairing code as an SVG image.
    pub fn qr_svg(&self) -> Result<String> {
        let code = qrcode::QrCode::with_error_correction_level(self.link.to_uri(), qrcode::EcLevel::M)
            .map_err(|e| Error::new(format!("cannot draw the pairing code: {e}")))?;
        Ok(code.render::<qrcode::render::svg::Color<'_>>().min_dimensions(256, 256).quiet_zone(true).build())
    }

    /// Waits up to `wait` for the phone, then hands the files over. The offer serves one
    /// session; `cancel` stops it at any point.
    pub fn serve(self, wait: Duration, cancel: &AtomicBool, mut on: impl FnMut(Status)) -> Result<Delivery> {
        self.listener.set_nonblocking(true)?;
        let deadline = Instant::now() + wait;
        let mut strangers = 0;
        on(Status::Waiting);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::new("cancelled"));
            }
            if Instant::now() >= deadline {
                return Err(Error::new("no phone connected in time"));
            }
            match self.listener.accept() {
                Ok((stream, _)) => match self.session(stream, cancel, &mut on) {
                    Ok(d) => return Ok(d),
                    Err(Fail::Fatal(e)) => return Err(e),
                    Err(Fail::Stranger) => {
                        strangers += 1;
                        if strangers >= MAX_STRANGERS {
                            return Err(Error::new("too many connections without the code: stopped for safety"));
                        }
                    }
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(POLL),
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn session(
        &self,
        stream: TcpStream,
        cancel: &AtomicBool,
        on: &mut impl FnMut(Status),
    ) -> std::result::Result<Delivery, Fail> {
        // Accepted sockets inherit non-blocking mode on Windows and macOS.
        let setup = (|| -> io::Result<()> {
            stream.set_nonblocking(false)?;
            stream.set_nodelay(true)?;
            stream.set_read_timeout(Some(HELLO_TIMEOUT))?;
            stream.set_write_timeout(Some(IO_TIMEOUT))
        })();
        if setup.is_err() {
            return Err(Fail::Stranger);
        }
        let mut ch = Channel::new(stream, &self.link.key, true)?;
        let hello = ch.recv_text().map_err(|_| Fail::Stranger)?;
        let device = match hello.strip_prefix(MAGIC).and_then(|r| r.strip_prefix(" HELLO\n")) {
            Some(name) => clean_device_name(name),
            None => return Err(Fail::Stranger),
        };
        on(Status::Connected { device: device.clone() });

        let total: u64 = self.files.iter().map(|f| f.size).sum();
        let mut offer = format!("{MAGIC} OFFER {} {total}\n", self.files.len());
        for f in &self.files {
            offer += &format!("{} {} {}\n", f.size, f.sha256, f.name);
        }
        ch.send(offer.as_bytes())?;
        ch.set_read_timeout(DECISION_TIMEOUT)?;
        let answer = ch.recv_text()?;
        if answer == format!("{MAGIC} DECLINE") {
            return Ok(Delivery::Declined { device });
        }
        if answer != format!("{MAGIC} ACCEPT") {
            return Err(Fail::Fatal(Error::new("unexpected answer from the phone")));
        }
        ch.set_read_timeout(IO_TIMEOUT)?;

        let mut sent = 0u64;
        let mut buf = vec![0u8; CHUNK];
        on(Status::Sending { sent, total });
        for f in &self.files {
            let mut src = File::open(&f.path).map_err(Error::from)?;
            let mut left = f.size;
            while left > 0 {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Fail::Fatal(Error::new("cancelled")));
                }
                let n = left.min(CHUNK as u64) as usize;
                src.read_exact(&mut buf[..n])
                    .map_err(|_| Error::new(format!("{} changed while it was being sent", f.name)))?;
                ch.send(&buf[..n])?;
                left -= n as u64;
                sent += n as u64;
                on(Status::Sending { sent, total });
            }
        }
        ch.send(format!("{MAGIC} END").as_bytes())?;
        let result = ch.recv_text()?;
        if let Some(reason) = result.strip_prefix(MAGIC).and_then(|r| r.strip_prefix(" FAIL ")) {
            return Err(Fail::Fatal(Error::new(format!("the phone could not save the files: {reason}"))));
        }
        match result.strip_prefix(MAGIC).and_then(|r| r.strip_prefix(" DONE ")).and_then(|n| n.parse::<usize>().ok()) {
            Some(n) if n == self.files.len() => Ok(Delivery::Delivered { device, files: n, bytes: total }),
            _ => Err(Fail::Fatal(Error::new("the phone did not confirm the files"))),
        }
    }
}

// ---------- receiving (reference for the Android app; also used by the tests) ----------

/// A file announced by the sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

/// Parses `ZDROP/1 OFFER` text, refusing anything inconsistent.
pub fn parse_offer(text: &str) -> Result<Vec<Incoming>> {
    let bad = || Error::new("malformed LocalDrop offer");
    let mut lines = text.lines();
    let head = lines.next().ok_or_else(bad)?;
    let mut parts = head.strip_prefix(MAGIC).and_then(|r| r.strip_prefix(" OFFER ")).ok_or_else(bad)?.split(' ');
    let count: usize = parts.next().and_then(|n| n.parse().ok()).ok_or_else(bad)?;
    let total: u64 = parts.next().and_then(|n| n.parse().ok()).ok_or_else(bad)?;
    if count == 0 || count > MAX_FILES {
        return Err(bad());
    }
    let mut files = Vec::with_capacity(count);
    for line in lines {
        let mut f = line.splitn(3, ' ');
        let size: u64 = f.next().and_then(|n| n.parse().ok()).ok_or_else(bad)?;
        let sha256 = f.next().filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).ok_or_else(bad)?;
        let name = clean_file_name(f.next().ok_or_else(bad)?);
        files.push(Incoming { name, size, sha256: sha256.to_ascii_lowercase() });
    }
    let sum = files.iter().try_fold(0u64, |acc, f| acc.checked_add(f.size)).ok_or_else(bad)?;
    if files.len() != count || sum != total {
        return Err(bad());
    }
    Ok(files)
}

/// A connected receiver that has seen the offer and has not answered yet.
pub struct Session {
    ch: Channel,
    pub sender: String,
    pub files: Vec<Incoming>,
}

/// Connects to the computer in `link` and reads its offer.
pub fn connect(link: &Link, device_name: &str) -> Result<Session> {
    let stream = TcpStream::connect_timeout(&link.addr, CONNECT_TIMEOUT)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut ch = Channel::new(stream, &link.key, false)?;
    ch.send(format!("{MAGIC} HELLO\n{}", clean_device_name(device_name)).as_bytes())?;
    let files = parse_offer(&ch.recv_text()?)?;
    Ok(Session { ch, sender: link.name.clone(), files })
}

impl Session {
    pub fn decline(mut self) -> Result<()> {
        self.ch.send(format!("{MAGIC} DECLINE").as_bytes())
    }

    /// Receives every file into `dest`, checks each SHA-256 and commits it without replacing
    /// existing files (`name 2.ext`). Returns the saved paths.
    pub fn accept(mut self, dest: &Path, mut on: impl FnMut(u64, u64)) -> Result<Vec<PathBuf>> {
        fs::create_dir_all(dest)?;
        self.ch.send(format!("{MAGIC} ACCEPT").as_bytes())?;
        let total: u64 = self.files.iter().map(|f| f.size).sum();
        let mut saved = Vec::new();
        let mut staged: Option<PathBuf> = None;
        let result = (|| -> Result<()> {
            let mut got = 0u64;
            for f in &self.files {
                let part = zeidex_core::runner::staging_path(dest, "part");
                staged = Some(part.clone());
                let mut out = File::create(&part)?;
                let mut ctx = digest::Context::new(&digest::SHA256);
                let mut left = f.size;
                while left > 0 {
                    let data = self.ch.recv()?;
                    if data.is_empty() || data.len() as u64 > left {
                        return Err(Error::new("the sender sent more data than it announced"));
                    }
                    out.write_all(&data)?;
                    ctx.update(&data);
                    left -= data.len() as u64;
                    got += data.len() as u64;
                    on(got, total);
                }
                out.sync_all()?;
                drop(out);
                if hex(ctx.finish().as_ref()) != f.sha256 {
                    return Err(Error::new(format!("{} arrived damaged (checksum mismatch)", f.name)));
                }
                saved.push(zeidex_core::runner::commit_new(&part, &dest.join(&f.name))?);
                staged = None;
            }
            if self.ch.recv_text()? != format!("{MAGIC} END") {
                return Err(Error::new("the sender did not finish the transfer"));
            }
            Ok(())
        })();
        if let Some(part) = staged {
            let _ = fs::remove_file(part);
        }
        match result {
            Ok(()) => {
                self.ch.send(format!("{MAGIC} DONE {}", saved.len()).as_bytes())?;
                Ok(saved)
            }
            Err(e) => {
                let _ = self.ch.send(format!("{MAGIC} FAIL {e}").as_bytes());
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::sync::Arc;

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zeidex-drop-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn serve_in_background(
        offer: Offer,
        wait: Duration,
    ) -> (std::thread::JoinHandle<Result<Delivery>>, Arc<AtomicBool>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        (std::thread::spawn(move || offer.serve(wait, &flag, |_| {})), cancel)
    }

    #[test]
    fn link_round_trip_and_checks() {
        let link = Link { addr: "192.168.1.20:5050".parse().unwrap(), key: [7; KEY_LEN], name: "My PC!".into() };
        let uri = link.to_uri();
        assert_eq!(uri, format!("zeidexdrop://v1?h=192.168.1.20&p=5050&k={}&n=My-PC-", "07".repeat(32)));
        let back = Link::parse(&uri).unwrap();
        assert_eq!((back.addr, back.key, back.name.as_str()), (link.addr, link.key, "My-PC-"));
        assert!(!format!("{back:?}").contains("0707"), "Debug never prints the key");

        let public = uri.replace("192.168.1.20", "8.8.8.8");
        assert!(Link::parse(&public).is_err(), "public addresses are refused");
        assert!(Link::parse(&uri.replace(&"07".repeat(32), "07")).is_err(), "short key");
        assert!(Link::parse("https://example.com").is_err());
        assert!(is_lan("fe80::1".parse().unwrap()) && is_lan("fd12::1".parse().unwrap()));
        assert!(!is_lan("2001:db8::1".parse().unwrap()));
    }

    #[test]
    fn file_names_cannot_escape() {
        assert_eq!(clean_file_name("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(clean_file_name(".."), "file");
        assert_eq!(clean_file_name("  "), "file");
        assert_eq!(clean_file_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(clean_file_name("name.\u{0}"), "name._");
        assert_eq!(clean_file_name("صورة 1.jpg"), "صورة 1.jpg");
    }

    #[test]
    fn offers_must_add_up() {
        let h = "a".repeat(64);
        assert_eq!(parse_offer(&format!("ZDROP/1 OFFER 1 5\n5 {h} x.txt\n")).unwrap()[0].name, "x.txt");
        assert!(parse_offer(&format!("ZDROP/1 OFFER 2 5\n5 {h} x.txt\n")).is_err(), "count");
        assert!(parse_offer(&format!("ZDROP/1 OFFER 1 6\n5 {h} x.txt\n")).is_err(), "total");
        assert!(parse_offer("ZDROP/1 OFFER 1 5\n5 nothex x.txt\n").is_err(), "digest");
        assert!(parse_offer(&format!("ZDROP/1 OFFER 1 5\n5 {h} ../x\n")).unwrap()[0].name == ".._x");
    }

    /// Pins the wire format. `LocalDropReceiverTest.kt` asserts the same frame.
    #[test]
    fn shared_test_vector() {
        let key: [u8; KEY_LEN] = std::array::from_fn(|i| i as u8);
        let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).unwrap());
        let frame = seal(&k, TO_RECEIVER, 0, b"ZDROP/1 END").unwrap();
        assert_eq!(hex(&frame), VECTOR);
    }
    /// Checked independently with Python `cryptography` (AESGCM) when it was pinned.
    const VECTOR: &str = "0000001bb868d070f480dd270f4a41b71ab30684ab4877f81d724561a00169";

    #[test]
    fn sends_files_end_to_end() {
        let d = scratch("e2e");
        let (src, dest) = (d.join("src"), d.join("dest"));
        fs::create_dir_all(&src).unwrap();
        let big: Vec<u8> = (0..(CHUNK * 2 + 12345)).map(|i| (i * 31 % 251) as u8).collect();
        fs::write(src.join("video.bin"), &big).unwrap();
        fs::write(src.join("empty.txt"), b"").unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("video.bin"), b"already here").unwrap();

        let offer = offer_on(LOOPBACK, &[src.join("video.bin"), src.join("empty.txt")], "Desk").unwrap();
        let link = offer.link().clone();
        let (server, _) = serve_in_background(offer, Duration::from_secs(20));

        let session = connect(&link, "Phone").unwrap();
        assert_eq!(session.files.len(), 2);
        assert_eq!(session.files[0].size, big.len() as u64);
        let mut last = (0, 0);
        let saved = session.accept(&dest, |got, total| last = (got, total)).unwrap();
        assert_eq!(last, (big.len() as u64, big.len() as u64));
        assert_eq!(saved, vec![dest.join("video 2.bin"), dest.join("empty.txt")]);
        assert_eq!(fs::read(&saved[0]).unwrap(), big);
        assert_eq!(fs::read(dest.join("video.bin")).unwrap(), b"already here", "existing file kept");
        assert_eq!(
            server.join().unwrap().unwrap(),
            Delivery::Delivered { device: "Phone".into(), files: 2, bytes: big.len() as u64 }
        );
        let leftovers =
            fs::read_dir(&dest).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with('.')).count();
        assert_eq!(leftovers, 0);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_stranger_is_ignored_and_the_phone_still_gets_through() {
        let d = scratch("stranger");
        fs::write(d.join("a.txt"), b"hello").unwrap();
        let offer = offer_on(LOOPBACK, &[d.join("a.txt")], "Desk").unwrap();
        let link = offer.link().clone();
        let (server, _) = serve_in_background(offer, Duration::from_secs(20));

        let mut wrong = link.clone();
        wrong.key[0] ^= 1;
        assert!(connect(&wrong, "Intruder").is_err(), "a wrong key never sees the offer");

        let session = connect(&link, "Phone").unwrap();
        session.decline().unwrap();
        assert_eq!(server.join().unwrap().unwrap(), Delivery::Declined { device: "Phone".into() });
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn waiting_can_be_cancelled_and_folders_are_refused() {
        let d = scratch("cancel");
        fs::write(d.join("a.txt"), b"x").unwrap();
        assert!(offer_on(LOOPBACK, std::slice::from_ref(&d), "Desk").is_err());
        assert!(offer_on("8.8.8.8".parse().unwrap(), &[d.join("a.txt")], "Desk").is_err());
        let offer = offer_on(LOOPBACK, &[d.join("a.txt")], "Desk").unwrap();
        let (server, cancel) = serve_in_background(offer, Duration::from_secs(60));
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(server.join().unwrap().unwrap_err().to_string(), "cancelled");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn qr_code_renders() {
        let d = scratch("qr");
        fs::write(d.join("a.txt"), b"x").unwrap();
        let offer = offer_on(LOOPBACK, &[d.join("a.txt")], "Desk").unwrap();
        let svg = offer.qr_svg().unwrap();
        assert!(svg.starts_with("<?xml") && svg.contains("<svg"));
        fs::remove_dir_all(&d).unwrap();
    }
}
