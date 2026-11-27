//! Conditional removal of the private nftables table. A concurrent editor can add a
//! foreign rule to an existing table without changing its handle. Pin the ruleset
//! generation *before* ownership inspection and compare it in the kernel's batch.
use crate::guard_journal::{Failure, Result};
use std::{
    io, mem,
    os::{
        fd::{FromRawFd, OwnedFd},
        unix::net::UnixDatagram,
    },
    time::Duration,
};

fn socket() -> io::Result<UnixDatagram> {
    // SAFETY: socket returns a fresh fd or -1. OwnedFd takes ownership exactly once.
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_NETFILTER,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let owner = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: a fully zeroed sockaddr_nl (kernel peer, no groups) with the specified family.
    let mut kernel: libc::sockaddr_nl = unsafe { mem::zeroed() };
    kernel.nl_family = libc::AF_NETLINK as _;
    let result = unsafe {
        libc::connect(
            fd,
            (&kernel as *const libc::sockaddr_nl).cast(),
            mem::size_of_val(&kernel) as _,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    let socket = UnixDatagram::from(owner);
    socket.set_read_timeout(Some(Duration::from_secs(3)))?;
    socket.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(socket)
}

// UAPI linux/netfilter/{nfnetlink,nf_tables}.h. nlmsghdr and nla headers are
// native endian. nfgenmsg.res_id and numeric nftables attributes are big endian.
fn attribute(kind: u16, payload: &[u8]) -> Vec<u8> {
    let size = 4 + payload.len();
    let mut bytes = Vec::from((size as u16).to_ne_bytes());
    bytes.extend(kind.to_ne_bytes());
    bytes.extend(payload);
    bytes.resize((size + 3) & !3, 0);
    bytes
}
fn message(kind: u16, flags: u16, seq: u32, family: u8, subsystem: u16, attrs: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::from(((20 + attrs.len()) as u32).to_ne_bytes());
    bytes.extend(kind.to_ne_bytes());
    bytes.extend(flags.to_ne_bytes());
    bytes.extend(seq.to_ne_bytes());
    bytes.extend(0u32.to_ne_bytes());
    bytes.extend([family, 0]);
    bytes.extend(subsystem.to_be_bytes());
    bytes.extend(attrs);
    bytes
}
fn nft(op: i32) -> u16 {
    ((libc::NFNL_SUBSYS_NFTABLES << 8) | op) as u16
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "unexpected netfilter response")
}
fn response(socket: &UnixDatagram, seq: u32, kind: u16) -> io::Result<Vec<u8>> {
    let mut buffer = [0u8; 4096];
    let count = socket.recv(&mut buffer)?;
    if count < 16 || count == buffer.len() {
        return Err(invalid());
    }
    let size = u32::from_ne_bytes(buffer[..4].try_into().unwrap()) as usize;
    if !(16..=count).contains(&size) {
        return Err(invalid());
    }
    let msg = u16::from_ne_bytes(buffer[4..6].try_into().unwrap());
    if msg == libc::NLMSG_ERROR as u16 {
        if size < 20 {
            return Err(invalid());
        }
        let err = i32::from_ne_bytes(buffer[16..20].try_into().unwrap());
        if err != 0 {
            return Err(io::Error::from_raw_os_error(err.saturating_neg()));
        }
    }
    if msg != kind || buffer[8..12] != seq.to_ne_bytes() {
        return Err(invalid());
    }
    Ok(buffer[16..size].to_vec())
}
fn generation(socket: &UnixDatagram) -> io::Result<u32> {
    let request = message(
        nft(libc::NFT_MSG_GETGEN),
        libc::NLM_F_REQUEST as u16,
        1,
        0,
        0,
        &[],
    );
    socket.send(&request)?;
    let reply = response(socket, 1, nft(libc::NFT_MSG_NEWGEN))?;
    let mut attrs = reply.get(4..).ok_or_else(invalid)?;
    while attrs.len() >= 4 {
        let size = u16::from_ne_bytes(attrs[..2].try_into().unwrap()) as usize;
        let kind = u16::from_ne_bytes(attrs[2..4].try_into().unwrap()) & 0x3fff;
        if !(4..=attrs.len()).contains(&size) {
            return Err(invalid());
        }
        if kind == 1 && size == 8 {
            // NFTA_GEN_ID. Zero would disable the generation guard.
            let value = u32::from_be_bytes(attrs[4..8].try_into().unwrap());
            return if value != 0 {
                Ok(value)
            } else {
                Err(invalid())
            };
        }
        attrs = attrs.get(((size + 3) & !3)..).ok_or_else(invalid)?;
    }
    Err(invalid())
}
fn guarded_delete(socket: &UnixDatagram, handle: u64, generation: u32) -> io::Result<()> {
    let request = libc::NLM_F_REQUEST as u16;
    let subsystem = libc::NFNL_SUBSYS_NFTABLES as u16;
    let expected = attribute(libc::NFNL_BATCH_GENID as u16, &generation.to_be_bytes());
    let mut batch = message(
        libc::NFNL_MSG_BATCH_BEGIN as u16,
        request,
        2,
        0,
        subsystem,
        &expected,
    );
    // NFTA_TABLE_HANDLE=4; target precisely the previously inspected inet table.
    batch.extend(message(
        nft(libc::NFT_MSG_DELTABLE),
        request | libc::NLM_F_ACK as u16,
        3,
        libc::NFPROTO_INET as u8,
        0,
        &attribute(4, &handle.to_be_bytes()),
    ));
    batch.extend(message(
        libc::NFNL_MSG_BATCH_END as u16,
        request,
        4,
        0,
        subsystem,
        &[],
    ));
    socket.send(&batch)?;
    response(socket, 3, libc::NLMSG_ERROR as u16).map(|_| ())
}
pub fn remove_if_current(handle: u64, verify: impl FnOnce() -> Result<()>) -> Result<()> {
    let unverified = |e: io::Error| {
        Failure(
            "ROLLBACK_UNVERIFIED",
            format!("nftables generation check: {e}"),
        )
    };
    let sock = socket().map_err(unverified)?;
    let before = generation(&sock).map_err(unverified)?;
    verify()?;
    guarded_delete(&sock, handle, before).map_err(unverified)
}
