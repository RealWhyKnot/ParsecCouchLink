//! Shared UDP socket construction with Windows hardening.
//!
//! On Windows a UDP socket reports `WSAECONNRESET` (os error 10054) on the next
//! recv/send after an earlier datagram drew an ICMP "port unreachable" from an
//! offline peer. For CouchLink that happens whenever a Pico reboots or drops off
//! Wi-Fi mid-session, and left unhandled it tears the whole stream down.
//! Disabling `SIO_UDP_CONNRESET` suppresses the reset, and [`is_transient`] is
//! the cross-platform backstop the stream loop uses to ride out any error that
//! still slips through.

use std::io;

use tokio::net::{ToSocketAddrs, UdpSocket};

/// Bind a UDP socket and apply CouchLink's standard hardening.
pub async fn bind_udp<A: ToSocketAddrs>(addr: A) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(addr).await?;
    disable_conn_reset(&socket);
    Ok(socket)
}

/// True for socket errors that are expected when a peer is briefly gone -- it
/// rebooted, dropped off Wi-Fi, or its IP moved. The stream loop logs these and
/// keeps running instead of exiting, letting the recovery path rediscover.
pub fn is_transient(err: &io::Error) -> bool {
    use io::ErrorKind::*;
    if matches!(
        err.kind(),
        ConnectionReset
            | ConnectionRefused
            | ConnectionAborted
            | HostUnreachable
            | NetworkUnreachable
            | NetworkDown
    ) {
        return true;
    }
    // Winsock codes std does not always map onto an ErrorKind:
    //   10050 NETDOWN   10051 NETUNREACH 10052 NETRESET
    //   10053 CONNABORTED 10054 CONNRESET 10065 HOSTUNREACH
    matches!(
        err.raw_os_error(),
        Some(10050 | 10051 | 10052 | 10053 | 10054 | 10065)
    )
}

#[cfg(windows)]
fn disable_conn_reset(socket: &UdpSocket) {
    use std::os::windows::io::AsRawSocket;

    // Stable Winsock IOCTL code. The `windows` crate's generated `WSAIoctl`
    // signature pulls in overlapped-I/O types behind a feature we do not need,
    // so we declare just the synchronous form against ws2_32 (already linked).
    const SIO_UDP_CONNRESET: u32 = 0x9800_000C;

    #[link(name = "ws2_32")]
    extern "system" {
        fn WSAIoctl(
            s: usize,
            dwIoControlCode: u32,
            lpvInBuffer: *const core::ffi::c_void,
            cbInBuffer: u32,
            lpvOutBuffer: *mut core::ffi::c_void,
            cbOutBuffer: u32,
            lpcbBytesReturned: *mut u32,
            lpOverlapped: *mut core::ffi::c_void,
            lpCompletionRoutine: *mut core::ffi::c_void,
        ) -> i32;
    }

    let raw = socket.as_raw_socket() as usize;
    let disable: i32 = 0; // FALSE -- stop reporting ICMP-unreachable as an error
    let mut bytes_returned: u32 = 0;
    // SAFETY: `raw` is a valid socket owned by `socket` for the duration of the
    // call; `disable` is a live read-only i32 in-buffer; no out-buffer,
    // overlapped I/O, or completion routine is used.
    let rc = unsafe {
        WSAIoctl(
            raw,
            SIO_UDP_CONNRESET,
            &disable as *const i32 as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes_returned,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        tracing::debug!(
            "net: disabling SIO_UDP_CONNRESET failed: {}",
            io::Error::last_os_error()
        );
    }
}

#[cfg(not(windows))]
fn disable_conn_reset(_socket: &UdpSocket) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_peer_gone_errors_as_transient() {
        for kind in [
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::HostUnreachable,
            io::ErrorKind::NetworkUnreachable,
            io::ErrorKind::NetworkDown,
        ] {
            assert!(is_transient(&io::Error::from(kind)), "{kind:?}");
        }
    }

    #[test]
    fn classifies_winsock_raw_codes_as_transient() {
        // WSAECONNRESET and friends arrive as raw OS errors on Windows.
        for code in [10050, 10051, 10052, 10053, 10054, 10065] {
            assert!(is_transient(&io::Error::from_raw_os_error(code)), "{code}");
        }
    }

    #[test]
    fn leaves_real_failures_fatal() {
        assert!(!is_transient(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!is_transient(&io::Error::from(io::ErrorKind::AddrInUse)));
        assert!(!is_transient(&io::Error::from_raw_os_error(10013))); // WSAEACCES
    }
}
