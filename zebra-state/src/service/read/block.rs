//! Shared block, header, and transaction reading code.

use std::sync::Arc;

use zebra_chain::{
    block::{self, Block, Height},
    transaction::{self, Transaction}, transparent::{self, Utxo},
};

use crate::{
    service::{finalized_state::ZebraDb, non_finalized_state::Chain},
    HashOrHeight,
};

/// Returns the [`Block`] with [`block::Hash`](zebra_chain::block::Hash) or
/// [`Height`], if it exists in the non-finalized `chain` or finalized `db`.
pub fn block<C>(chain: Option<C>, db: &ZebraDb, hash_or_height: HashOrHeight) -> Option<Arc<Block>>
where
    C: AsRef<Chain>,
{
    // # Correctness
    //
    // The StateService commits blocks to the finalized state before updating
    // the latest chain, and it can commit additional blocks after we've cloned
    // this `chain` variable.
    //
    // Since blocks are the same in the finalized and non-finalized state, we
    // check the most efficient alternative first. (`chain` is always in memory,
    // but `db` stores blocks on disk, with a memory cache.)
    chain
        .as_ref()
        .and_then(|chain| chain.as_ref().block(hash_or_height))
        .map(|contextual| contextual.block.clone())
        .or_else(|| db.block(hash_or_height))
}

/// Returns the [`block::Header`] with [`block::Hash`](zebra_chain::block::Hash) or
/// [`Height`], if it exists in the non-finalized `chain` or finalized `db`.
pub fn block_header<C>(
    chain: Option<C>,
    db: &ZebraDb,
    hash_or_height: HashOrHeight,
) -> Option<Arc<block::Header>>
where
    C: AsRef<Chain>,
{
    // # Correctness
    //
    // The StateService commits blocks to the finalized state before updating
    // the latest chain, and it can commit additional blocks after we've cloned
    // this `chain` variable.
    //
    // Since blocks are the same in the finalized and non-finalized state, we
    // check the most efficient alternative first. (`chain` is always in memory,
    // but `db` stores blocks on disk, with a memory cache.)
    chain
        .as_ref()
        .and_then(|chain| chain.as_ref().block(hash_or_height))
        .map(|contextual| contextual.block.header.clone())
        .or_else(|| db.block_header(hash_or_height))
}

/// Returns the [`Transaction`] with [`transaction::Hash`], if it exists in the
/// non-finalized `chain` or finalized `db`.
pub fn transaction<C>(
    chain: Option<C>,
    db: &ZebraDb,
    hash: transaction::Hash,
) -> Option<(Arc<Transaction>, Height)>
where
    C: AsRef<Chain>,
{
    // # Correctness
    //
    // The StateService commits blocks to the finalized state before updating
    // the latest chain, and it can commit additional blocks after we've cloned
    // this `chain` variable.
    //
    // Since transactions are the same in the finalized and non-finalized state,
    // we check the most efficient alternative first. (`chain` is always in
    // memory, but `db` stores transactions on disk, with a memory cache.)
    chain
        .and_then(|chain| {
            chain
                .as_ref()
                .transaction(hash)
                .map(|(tx, height)| (tx.clone(), height))
        })
        .or_else(|| db.transaction(hash))
}


/// Returns the [`Utxo`] for [`transparent::OutPoint`], if it exists in the
/// non-finalized `chain` or finalized `db`.
///
/// Non-finalized UTXOs are returned regardless of whether they have been spent.
///
/// Finalized UTXOs are only returned if they are unspent in the finalized chain.
/// They may have been spent in the non-finalized chain,
/// but this function returns them without checking for non-finalized spends,
/// because we don't know which non-finalized chain will be committed to the finalized state.
pub fn utxo<C>(chain: Option<C>, db: &ZebraDb, outpoint: transparent::OutPoint) -> Option<Utxo>
where
    C: AsRef<Chain>,
{
    // # Correctness
    //
    // Since UTXOs are the same in the finalized and non-finalized state,
    // we check the most efficient alternative first. (`chain` is always in
    // memory, but `db` stores transactions on disk, with a memory cache.)
    chain
        .and_then(|chain| chain.as_ref().created_utxo(&outpoint))
        .or_else(|| db.utxo(&outpoint).map(|utxo| utxo.utxo))
}

/// Returns the [`Utxo`] for [`transparent::OutPoint`], if it exists and is unspent in the
/// non-finalized `chain` or finalized `db`.
pub fn unspent_utxo<C>(
    chain: Option<C>,
    db: &ZebraDb,
    outpoint: transparent::OutPoint,
) -> Option<Utxo>
where
    C: AsRef<Chain>,
{
    match chain {
        Some(chain) if chain.as_ref().spent_utxos.contains(&outpoint) => None,
        chain => utxo(chain, db, outpoint),
    }
}
