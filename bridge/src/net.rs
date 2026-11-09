use std::io;

use tokio::net::{ToSocketAddrs, UdpSocket};

pub(crate) async fn bind_udp<A: ToSocketAddrs>(addr: A) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(addr).await?;
    disable_conn_reset(&socket);
    Ok(socket)
}

pub(crate) fn is_transient(err: &io::Error) -> bool {
    use io::ErrorKind::{
        ConnectionAborted, ConnectionRefused, ConnectionReset, HostUnreachable, NetworkDown,
        NetworkUnreachable,
    };
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
    matches!(
        err.raw_os_error(),
        Some(10050 | 10051 | 10052 | 10053 | 10054 | 10065)
    )
}

#[cfg(windows)]
fn disable_conn_reset(socket: &UdpSocket) {
    use std::os::windows::io::AsRawSocket;

    const SIO_UDP_CONNRESET: u32 = 0x9800_000C;
    const OPTION_LEN: u32 = i32::BITS / 8;

    #[link(name = "ws2_32")]
    unsafe extern "system" {
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

    let Ok(raw) = usize::try_from(socket.as_raw_socket()) else {
        return;
    };
    let disable: i32 = 0;
    let mut bytes_returned: u32 = 0;
    // SAFETY: `raw` is `socket`'s live handle, `disable` outlives the call, and no out-buffer is used.
    let rc = unsafe {
        WSAIoctl(
            raw,
            SIO_UDP_CONNRESET,
            (&raw const disable).cast::<core::ffi::c_void>(),
            OPTION_LEN,
            std::ptr::null_mut(),
            0,
            &raw mut bytes_returned,
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
        assert!(!is_transient(&io::Error::from_raw_os_error(10013)));
    }
}
