//! `shutdown`: asks a running `serve` to exit, the way a provider's stop
//! command would.

use std::net::SocketAddr;

use crate::Failure;
use crate::args::Flags;
use crate::http;

/// Runs `shutdown --port=<p> [--host=<ip>]`: exit 0 when the server answers
/// `POST /shutdown` with 200, exit 1 otherwise.
pub(crate) fn run(args: &[String]) -> Result<(), Failure> {
    let flags = Flags::parse(args, &["port", "host"], &[])?;
    let address = SocketAddr::new(flags.host()?, flags.port()?);
    let status = http::post(address, "/shutdown")
        .map_err(|error| Failure::Runtime(format!("cannot reach {address}: {error}")))?;
    if status != 200 {
        return Err(Failure::Runtime(format!(
            "{address} answered the shutdown with {status}"
        )));
    }
    println!("{address} is shutting down");
    Ok(())
}
