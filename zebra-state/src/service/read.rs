//! Shared state reading code.
//!
//! Used by [`StateService`][1] and [`ReadStateService`][2] to read from the
//! best [`Chain`][5] in the [`NonFinalizedState`][3], and the database in the
//! [`FinalizedState`][4].
//!
//! [1]: super::StateService
//! [2]: super::ReadStateService
//! [3]: super::non_finalized_state::NonFinalizedState
//! [4]: super::finalized_state::FinalizedState
//! [5]: super::Chain

pub mod address;
pub mod block;
pub mod find;
pub mod tree;

#[cfg(feature = "getblocktemplate-rpcs")]
pub mod difficulty;

#[cfg(test)]
mod tests;

pub use address::{
    balance::transparent_balance,
    tx_id::transparent_tx_ids,
    utxo::{transparent_utxos, AddressUtxos, ADDRESS_HEIGHTS_FULL_RANGE},
};
pub use block::{block, block_header, mined_transaction, unspent_utxo, transaction_hashes_for_block, any_utxo};
pub use find::{
    chain_contains_hash, find_chain_hashes, find_chain_headers, hash_by_height, height_by_hash, tip,
    tip_height, read_best_chain_blocks, best_tip, depth, block_locator, komodo_next_median_time_past, komodo_init_last_nota,
};
pub use tree::{orchard_tree, sapling_tree};

/// If a finalized state query is interrupted by a new finalized block,
/// retry this many times.
///
/// Once we're at the tip, we expect up to 2 blocks to arrive at the same time.
/// If any more arrive, the client should wait until we're synchronised with our peers.
pub const FINALIZED_STATE_QUERY_RETRIES: usize = 3;
