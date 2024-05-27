/// The prefix for a Base58Check-encoded mainnet transparent P2SH address.
pub(crate) const MAINNET: [u8; 1] = [85];

/// The prefix for a Base58Check-encoded testnet transparent P2SH address.
pub(crate) const TESTNET: [u8; 1] = [5];

pub(crate) type Data = [u8; 20];
