//! active network probe used by `maranode verify network`

use anyhow::Result;

const PROBE_TARGETS: &[(&str, u16)] = &[
    ("1.1.1.1", 53), // cloudflare DNS
    ("8.8.8.8", 53), // Google DNS
];

const PROBE_TIMEOUT_MS: u64 = 2_000;

pub fn probe_external_blocked() -> Result<bool> {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let timeout = Duration::from_millis(PROBE_TIMEOUT_MS);

    for (host, port) in PROBE_TARGETS {
        let addr_str = format!("{}:{}", host, port);
        match addr_str.to_socket_addrs() {
            Err(_) => continue,
            Ok(mut addrs) => {
                if let Some(addr) = addrs.next() {
                    if TcpStream::connect_timeout(&addr, timeout).is_ok() {
                        return Ok(false);
                    }
                }
            }
        }
    }

    Ok(true)
}
