//! The Unix domain socket transport: [`listener`] for the daemon,
//! [`endpoint`] and [`client`] for clients, and [`connection`] under both.

mod client;
mod connection;
mod endpoint;
mod listener;

pub use client::{Client, EventStream};
pub use connection::{FrameReader, FrameWriter, FramedConnection};
pub use endpoint::{ConnectPolicy, connect_verified, verify_path, verify_peer};
pub use listener::{Accepted, BindState, Listener, PeerCred, PeerCredSource, TokioPeerCred};
