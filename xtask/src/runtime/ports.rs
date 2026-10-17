//! Loopback ports for the spike (spec 0003): bind `127.0.0.1:0`, read the
//! port, and let it go right before the server that uses it starts.

use std::net::{Ipv4Addr, TcpListener};

use anyhow::{Context, Result};

/// Returns `count` different loopback ports that are free right now.
///
/// Every listener stays bound until all ports are known, so the OS never
/// hands out the same port twice. Nothing holds the ports afterwards, so
/// start the server straight away.
pub fn allocate(count: usize) -> Result<Vec<u16>> {
    let listeners = (0..count)
        .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).context("cannot bind a loopback port"))
        .collect::<Result<Vec<_>>>()?;
    listeners
        .iter()
        .map(|listener| {
            Ok(listener
                .local_addr()
                .context("cannot read the bound loopback port")?
                .port())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocated_ports_differ_and_can_be_bound_again() {
        let ports = allocate(3).unwrap();
        assert_eq!(ports.len(), 3);
        assert!(ports[0] != ports[1] && ports[1] != ports[2] && ports[0] != ports[2]);
        for port in ports {
            TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
        }
    }
}
