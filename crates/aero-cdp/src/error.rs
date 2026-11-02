//! Transport errors. Each variant is a distinct, typed outcome; none is coerced
//! into an empty successful result.

use std::fmt;

/// Error returned by CDP transport operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CdpError {
    /// The per-call deadline elapsed before a reply arrived.
    Timeout,
    /// The connection ended (EOF, close frame, transport error) before a reply.
    Disconnected,
    /// The browser answered with a protocol error object.
    Protocol {
        /// CDP/JSON-RPC error code.
        code: i64,
        /// Browser-supplied message. Treat as untrusted text.
        message: String,
    },
    /// A frame could not be interpreted as the expected CDP envelope.
    InvalidMessage(String),
    /// An event subscriber fell behind and missed this many events; its view
    /// of browser state must be resynchronized, not continued.
    EventLag(u64),
    /// The local owner closed the connection.
    Cancelled,
    /// The endpoint is not a plain loopback DevTools WebSocket URL.
    InvalidEndpoint(String),
    /// A request exceeded the configured message size limit and was not sent.
    TooLarge,
}

impl fmt::Display for CdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("CDP call deadline elapsed"),
            Self::Disconnected => f.write_str("CDP connection ended"),
            Self::Protocol { code, .. } => write!(f, "CDP protocol error {code}"),
            Self::InvalidMessage(reason) => write!(f, "invalid CDP message: {reason}"),
            Self::EventLag(missed) => write!(f, "CDP event subscriber missed {missed} events"),
            Self::Cancelled => f.write_str("CDP connection closed locally"),
            Self::InvalidEndpoint(reason) => write!(f, "invalid CDP endpoint: {reason}"),
            Self::TooLarge => f.write_str("CDP request exceeds the message size limit"),
        }
    }
}

impl std::error::Error for CdpError {}
