//! Which process holds a local port, so "port 8787 is already in use" can
//! name the program (e.g. "node (PID 4242)").
//!
//! * macOS: `lsof -F` output, with a short timeout.
//! * Linux: the socket inode from `/proc/net/{tcp,udp}{,6}`, then the
//!   `/proc/<pid>/fd` link that points at it. No external commands.
//! * Windows: `GetExtendedTcpTable` / `GetExtendedUdpTable` for the PID and
//!   `QueryFullProcessImageNameW` for the name.
//!
//! Best effort everywhere: no permission, a missing tool or no match gives
//! `None` rather than an error.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortOwner {
    pub pid: u32,
    /// Process name (e.g. "node", "java.exe"), when the OS tells us.
    pub name: Option<String>,
}

impl fmt::Display for PortOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{name} (PID {})", self.pid),
            None => write!(f, "PID {}", self.pid),
        }
    }
}

/// The process listening on (TCP) or bound to (UDP) `port` on this computer, if it can be found.
/// Best effort: `None` when the OS doesn't say (no permission, tool missing, not found).
/// Blocking (may run a short command); call it from `spawn_blocking` in async code.
pub fn port_owner(port: u16, protocol: Protocol) -> Option<PortOwner> {
    imp::port_owner(port, protocol)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::io::Read;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{PortOwner, Protocol, parse_lsof};

    /// How long lsof may run before it is killed.
    const LSOF_TIMEOUT: Duration = Duration::from_secs(2);

    pub(super) fn port_owner(port: u16, protocol: Protocol) -> Option<PortOwner> {
        let lsof = if Path::new("/usr/sbin/lsof").exists() { "/usr/sbin/lsof" } else { "lsof" };
        let mut cmd = Command::new(lsof);
        cmd.arg("-nP");
        match protocol {
            Protocol::Tcp => cmd.arg(format!("-iTCP:{port}")).arg("-sTCP:LISTEN"),
            Protocol::Udp => cmd.arg(format!("-iUDP:{port}")),
        };
        cmd.arg("-Fpc");
        let out = run(cmd, LSOF_TIMEOUT)?;
        parse_lsof(&String::from_utf8_lossy(&out))
    }

    /// Run `cmd` and return its stdout, or `None` if it can't start or takes
    /// longer than `timeout` (the child is killed then).
    fn run(mut cmd: Command, timeout: Duration) -> Option<Vec<u8>> {
        let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        // Read on another thread so the wait can time out; the thread ends
        // when the pipe closes.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = stdout.read_to_end(&mut out);
            let _ = tx.send(out);
        });
        let out = rx.recv_timeout(timeout).ok();
        if out.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        out
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::fs;

    use super::{PortOwner, Protocol, listening_inodes, socket_link_inode};

    pub(super) fn port_owner(port: u16, protocol: Protocol) -> Option<PortOwner> {
        let tables: [&str; 2] = match protocol {
            Protocol::Tcp => ["/proc/net/tcp", "/proc/net/tcp6"],
            Protocol::Udp => ["/proc/net/udp", "/proc/net/udp6"],
        };
        let inodes: Vec<u64> = tables
            .iter()
            .filter_map(|path| fs::read_to_string(path).ok())
            .flat_map(|table| listening_inodes(&table, port, protocol))
            .collect();
        if inodes.is_empty() {
            return None;
        }
        let pid = pid_with_socket(&inodes)?;
        let name = fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|s| s.trim_end().to_string())
            .filter(|s| !s.is_empty());
        Some(PortOwner { pid, name })
    }

    /// The first process with an open file descriptor for one of `inodes`.
    /// Processes we may not look into are skipped.
    fn pid_with_socket(inodes: &[u64]) -> Option<u32> {
        for entry in fs::read_dir("/proc").ok()?.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            let Ok(fds) = fs::read_dir(entry.path().join("fd")) else {
                continue;
            };
            for fd in fds.flatten() {
                let Ok(link) = fs::read_link(fd.path()) else {
                    continue;
                };
                if link.to_str().and_then(socket_link_inode).is_some_and(|inode| inodes.contains(&inode)) {
                    return Some(pid);
                }
            }
        }
        None
    }
}

#[cfg(windows)]
mod imp {
    use std::mem::offset_of;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, MIB_UDP6ROW_OWNER_PID, MIB_UDP6TABLE_OWNER_PID, MIB_UDPROW_OWNER_PID,
        MIB_UDPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER, UDP_TABLE_OWNER_PID,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    use super::{PortOwner, Protocol};

    pub(super) fn port_owner(port: u16, protocol: Protocol) -> Option<PortOwner> {
        let pid = owning_pid(port, protocol)?;
        Some(PortOwner { pid, name: process_name(pid) })
    }

    /// The PID from the IPv4 table, then the IPv6 one.
    fn owning_pid(port: u16, protocol: Protocol) -> Option<u32> {
        match protocol {
            Protocol::Tcp => find::<MIB_TCPROW_OWNER_PID>(port, protocol, AF_INET)
                .or_else(|| find::<MIB_TCP6ROW_OWNER_PID>(port, protocol, AF_INET6)),
            Protocol::Udp => find::<MIB_UDPROW_OWNER_PID>(port, protocol, AF_INET)
                .or_else(|| find::<MIB_UDP6ROW_OWNER_PID>(port, protocol, AF_INET6)),
        }
    }

    /// A row of an owner-PID table. Only implemented for the IP Helper row
    /// structs, which are plain integers (valid for any bit pattern).
    trait Row {
        /// Where the first row starts in its table (after the u32 count).
        const OFFSET: usize;
        /// The local port in network byte order, in the low 16 bits.
        fn local_port(&self) -> u32;
        fn pid(&self) -> u32;
    }

    macro_rules! row {
        ($row:ty, $table:ty) => {
            impl Row for $row {
                const OFFSET: usize = offset_of!($table, table);
                fn local_port(&self) -> u32 {
                    self.dwLocalPort
                }
                fn pid(&self) -> u32 {
                    self.dwOwningPid
                }
            }
        };
    }
    row!(MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID);
    row!(MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID);
    row!(MIB_UDPROW_OWNER_PID, MIB_UDPTABLE_OWNER_PID);
    row!(MIB_UDP6ROW_OWNER_PID, MIB_UDP6TABLE_OWNER_PID);

    /// The owner of `port` in one table.
    fn find<R: Row>(port: u16, protocol: Protocol, family: u16) -> Option<u32> {
        let buf = table(protocol, family)?;
        rows::<R>(&buf).iter().find(|r| u16::from_be(r.local_port() as u16) == port).map(R::pid)
    }

    /// The listening TCP (or bound UDP) table with owning PIDs for one
    /// address family, in a u32-aligned buffer. The table can grow between
    /// the size query and the fetch, so this retries a few times.
    fn table(protocol: Protocol, family: u16) -> Option<Vec<u32>> {
        let mut buf: Vec<u32> = Vec::new();
        let mut size = 0u32;
        for _ in 0..5 {
            let ptr = if buf.is_empty() { std::ptr::null_mut() } else { buf.as_mut_ptr().cast() };
            // SAFETY: `ptr` is null with `size` 0, or points to `buf`, which holds
            // at least `size` bytes and is aligned for the table (all u32 fields).
            let code = unsafe {
                match protocol {
                    Protocol::Tcp => {
                        GetExtendedTcpTable(ptr, &mut size, 0, family.into(), TCP_TABLE_OWNER_PID_LISTENER, 0)
                    }
                    Protocol::Udp => GetExtendedUdpTable(ptr, &mut size, 0, family.into(), UDP_TABLE_OWNER_PID, 0),
                }
            };
            match code {
                NO_ERROR => return Some(buf),
                ERROR_INSUFFICIENT_BUFFER => buf = vec![0; (size as usize).div_ceil(4)],
                _ => return None,
            }
        }
        None
    }

    /// The rows of a table filled in by [`table`]: a u32 row count, then the
    /// rows. Empty if the count doesn't fit the buffer.
    fn rows<R: Row>(buf: &[u32]) -> &[R] {
        let Some(&count) = buf.first() else {
            return &[];
        };
        let count = count as usize;
        let fits = count
            .checked_mul(size_of::<R>())
            .and_then(|n| n.checked_add(R::OFFSET))
            .is_some_and(|end| end <= size_of_val(buf));
        let aligned = align_of::<R>() <= align_of::<u32>() && R::OFFSET.is_multiple_of(align_of::<R>());
        if !fits || !aligned {
            return &[];
        }
        // SAFETY: the rows lie inside `buf` and are aligned (checked above), and
        // `Row` types are plain integer structs, valid for any bit pattern.
        unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>().add(R::OFFSET).cast::<R>(), count) }
    }

    /// The executable's file name (e.g. "node.exe"). `None` when the process
    /// can't be opened, like System or elevated processes.
    fn process_name(pid: u32) -> Option<String> {
        // SAFETY: plain call; failure returns a null handle.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        // Long paths can be up to 32767 UTF-16 units.
        let mut buf = vec![0u16; 32_768];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` UTF-16 units and the handle is open.
        let ok = unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
        // SAFETY: the handle came from OpenProcess and is closed once.
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(buf.get(..len as usize)?);
        path.rsplit(['\\', '/']).next().filter(|s| !s.is_empty()).map(str::to_string)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    use super::{PortOwner, Protocol};

    pub(super) fn port_owner(_port: u16, _protocol: Protocol) -> Option<PortOwner> {
        None
    }
}

/// The first process in `lsof -F` output: `p<pid>` starts a process and
/// `c<command>` names it; other fields (like `f<fd>`) are ignored.
#[cfg(any(target_os = "macos", test))]
fn parse_lsof(out: &str) -> Option<PortOwner> {
    let mut owner: Option<PortOwner> = None;
    for line in out.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            if owner.is_some() {
                break;
            }
            owner = pid.trim().parse().ok().map(|pid| PortOwner { pid, name: None });
        } else if let Some(command) = line.strip_prefix('c')
            && let Some(owner) = owner.as_mut()
            && owner.name.is_none()
            && !command.is_empty()
        {
            owner.name = Some(command.to_string());
        }
    }
    owner
}

/// TCP state `0A` in `/proc/net/tcp*`.
#[cfg(any(target_os = "linux", test))]
const TCP_LISTEN: u8 = 0x0A;

/// One row of `/proc/net/{tcp,udp}{,6}`: local port, state and socket inode.
/// `None` for the header or anything malformed.
#[cfg(any(target_os = "linux", test))]
fn parse_proc_net_line(line: &str) -> Option<(u16, u8, u64)> {
    let mut fields = line.split_whitespace();
    let _slot = fields.next()?;
    let local = fields.next()?;
    let _remote = fields.next()?;
    let state = u8::from_str_radix(fields.next()?, 16).ok()?;
    // Skip tx_queue:rx_queue, tr:tm->when, retrnsmt, uid and timeout.
    let inode = fields.nth(5)?.parse().ok()?;
    let port = u16::from_str_radix(local.rsplit_once(':')?.1, 16).ok()?;
    Some((port, state, inode))
}

/// Socket inodes in one `/proc/net` table for `port`: listening TCP sockets,
/// or any UDP socket bound to it.
#[cfg(any(target_os = "linux", test))]
fn listening_inodes(table: &str, port: u16, protocol: Protocol) -> Vec<u64> {
    table
        .lines()
        .filter_map(parse_proc_net_line)
        .filter(|&(p, state, inode)| p == port && inode != 0 && (protocol == Protocol::Udp || state == TCP_LISTEN))
        .map(|(_, _, inode)| inode)
        .collect()
}

/// The inode in a `/proc/<pid>/fd` link like `socket:[12345]`.
#[cfg(any(target_os = "linux", test))]
fn socket_link_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?.strip_suffix(']')?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_owner() {
        let owner = PortOwner { pid: 4242, name: Some("node".into()) };
        assert_eq!(owner.to_string(), "node (PID 4242)");
        let owner = PortOwner { pid: 4242, name: None };
        assert_eq!(owner.to_string(), "PID 4242");
    }

    #[test]
    fn parses_lsof_output() {
        let out = "p4242\ncnode\nf23\nf24\np5151\ncother\nf7\n";
        assert_eq!(parse_lsof(out), Some(PortOwner { pid: 4242, name: Some("node".into()) }));
        let out = "p993\ncCursor Helper (Plugin)\nf10\n";
        assert_eq!(parse_lsof(out), Some(PortOwner { pid: 993, name: Some("Cursor Helper (Plugin)".into()) }));
        assert_eq!(parse_lsof("p77\nf3\n"), Some(PortOwner { pid: 77, name: None }));
        assert_eq!(parse_lsof(""), None);
        assert_eq!(parse_lsof("cnode\n"), None);
        assert_eq!(parse_lsof("pnot-a-pid\ncnode\n"), None);
    }

    #[test]
    fn parses_proc_net_lines() {
        let header = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";
        assert_eq!(parse_proc_net_line(header), None);
        let v4 = "   0: 0100007F:2253 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 123456 1 0000000000000000 100 0 0 10 0";
        assert_eq!(parse_proc_net_line(v4), Some((8787, TCP_LISTEN, 123456)));
        let v6 = "   1: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 998877 1 0000000000000000 100 0 0 10 0";
        assert_eq!(parse_proc_net_line(v6), Some((8080, TCP_LISTEN, 998877)));
        let udp = "  512: 00000000:14E9 00000000:0000 07 00000000:00000000 00:00000000 00000000   101        0 4455 2 0000000000000000 0";
        assert_eq!(parse_proc_net_line(udp), Some((5353, 0x07, 4455)));
        assert_eq!(parse_proc_net_line("   0: 0100007F:2253"), None);
        assert_eq!(parse_proc_net_line(""), None);
    }

    #[test]
    fn finds_inodes_in_proc_net_tables() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:2253 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 111 1 0000000000000000 100 0 0 10 0
   1: 0100007F:2253 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1000        0 222 1 0000000000000000 20 4 30 10 -1
   2: 0100007F:C350 0100007F:2253 06 00000000:00000000 03:00001234 00000000     0        0 0 3 0000000000000000
   3: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 333 1 0000000000000000 100 0 0 10 0";
        assert_eq!(listening_inodes(table, 8787, Protocol::Tcp), vec![111]);
        assert_eq!(listening_inodes(table, 8787, Protocol::Udp), vec![111, 222]);
        assert_eq!(listening_inodes(table, 8080, Protocol::Tcp), vec![333]);
        assert!(listening_inodes(table, 50000, Protocol::Tcp).is_empty());
        assert!(listening_inodes(table, 9, Protocol::Tcp).is_empty());
    }

    #[test]
    fn parses_socket_links() {
        assert_eq!(socket_link_inode("socket:[12345]"), Some(12345));
        assert_eq!(socket_link_inode("pipe:[12345]"), None);
        assert_eq!(socket_link_inode("/dev/null"), None);
        assert_eq!(socket_link_inode("socket:[]"), None);
    }

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn finds_this_process_for_a_tcp_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let owner = port_owner(port, Protocol::Tcp).expect("owner of our own listener");
        assert_eq!(owner.pid, std::process::id());
        assert!(owner.name.is_some(), "no name for our own process: {owner}");
    }

    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn finds_this_process_for_a_udp_socket() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let owner = port_owner(port, Protocol::Udp).expect("owner of our own socket");
        assert_eq!(owner.pid, std::process::id());
        assert!(owner.name.is_some(), "no name for our own process: {owner}");
    }

    #[test]
    fn free_port_does_not_panic() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        // Something else may grab the port in between; only check it returns.
        let _ = port_owner(port, Protocol::Tcp);
        let _ = port_owner(port, Protocol::Udp);
    }
}
