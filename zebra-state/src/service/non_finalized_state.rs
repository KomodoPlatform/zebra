//! Non-finalized chain state management as defined by [RFC0005]
//!
//! [RFC0005]: https://zebra.zfnd.org/dev/rfcs/0005-state-updates.html

use std::{
    collections::{BTreeSet, HashMap},
    mem,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use zebra_chain::{
    block::{self, Block},
    parameters::Network,
    sprout, transparent::{self, outputs_from_utxos, utxos_from_ordered_utxos}, komodo_nota::BackNotarisationData,
};

use crate::{
    request::{ContextuallyValidBlock, FinalizedWithTrees},
    service::{check, finalized_state::ZebraDb},
    PreparedBlock, ValidateContextError, komodo_notaries::komodo_block_has_notarisation_tx,
};

mod chain;

#[cfg(test)]
mod tests;

pub(crate) use chain::Chain;

/// The state of the chains in memory, including queued blocks.
///
/// Clones of the non-finalized state contain independent copies of the chains.
/// This is different from `FinalizedState::clone()`,
/// which returns a shared reference to the database.
///
/// Most chain data is clone-on-write using [`Arc`].
#[derive(Clone, Debug)]
pub struct NonFinalizedState {
    /// Verified, non-finalized chains, in ascending order.
    ///
    /// The best chain is `chain_set.last()` or `chain_set.iter().next_back()`.
    pub chain_set: BTreeSet<Arc<Chain>>,

    /// The configured Zcash network.
    pub network: Network,

    #[cfg(feature = "getblocktemplate-rpcs")]
    /// Configures the non-finalized state to count metrics.
    ///
    /// Used for skipping metrics counting when testing block proposals
    /// with a commit to a cloned non-finalized state.
    pub should_count_metrics: bool,

    /// notarisation object pointing to the last notarised height
    pub last_nota: Option<BackNotarisationData>,

    /// block hash where the last nota is located
    pub last_nota_block_hash: Option<block::Hash>,
}

impl NonFinalizedState {
    /// Returns a new non-finalized state for `network`.
    pub fn new(network: Network) -> NonFinalizedState {
        NonFinalizedState {
            chain_set: Default::default(),
            network,
            #[cfg(feature = "getblocktemplate-rpcs")]
            should_count_metrics: true,
            last_nota: None,
            last_nota_block_hash: None,
        }
    }

    /// Is the internal state of `self` the same as `other`?
    ///
    /// [`Chain`] has a custom [`Eq`] implementation based on proof of work,
    /// which is used to select the best chain. So we can't derive [`Eq`] for [`NonFinalizedState`].
    ///
    /// Unlike the custom trait impl, this method returns `true` if the entire internal state
    /// of two non-finalized states is equal.
    ///
    /// If the internal states are different, it returns `false`,
    /// even if the chains and blocks are equal.
    #[cfg(test)]
    pub(crate) fn eq_internal_state(&self, other: &NonFinalizedState) -> bool {
        // this method must be updated every time a field is added to NonFinalizedState

        self.chain_set.len() == other.chain_set.len()
            && self
                .chain_set
                .iter()
                .zip(other.chain_set.iter())
                .all(|(self_chain, other_chain)| self_chain.eq_internal_state(other_chain))
            && self.network == other.network
    }

    /// Finalize the lowest height block in the non-finalized portion of the best
    /// chain and update all side-chains to match.
    pub fn finalize(&mut self) -> FinalizedWithTrees {
        // Chain::cmp uses the partial cumulative work, and the hash of the tip block.
        // Neither of these fields has interior mutability.
        // (And when the tip block is dropped for a chain, the chain is also dropped.)
        #[allow(clippy::mutable_key_type)]
        let chains = mem::take(&mut self.chain_set);
        let mut chains = chains.into_iter();

        // extract best chain
        let mut best_chain = chains.next_back().expect("there's at least one chain");

        // clone if required
        let mut_best_chain = Arc::make_mut(&mut best_chain);

        // extract the rest into side_chains so they can be mutated
        let side_chains = chains;

        // Pop the lowest height block from the best chain to be finalized, and
        // also obtain its associated treestate.
        let (best_chain_root, root_treestate) = mut_best_chain.pop_root();

        // add best_chain back to `self.chain_set`
        if !best_chain.is_empty() {
            self.chain_set.insert(best_chain);
        }

        // for each remaining chain in side_chains
        for mut side_chain in side_chains {
            if side_chain.non_finalized_root_hash() != best_chain_root.hash {
                // If we popped the root, the chain would be empty or orphaned,
                // so just drop it now.
                drop(side_chain);

                continue;
            }

            // otherwise, the popped root block is the same as the finalizing block

            // clone if required
            let mut_side_chain = Arc::make_mut(&mut side_chain);

            // remove the first block from `chain`
            let (side_chain_root, _treestate) = mut_side_chain.pop_root();
            assert_eq!(side_chain_root.hash, best_chain_root.hash);

            // add the chain back to `self.chain_set`
            self.chain_set.insert(side_chain);
        }

        self.update_metrics_for_chains();

        // Add the treestate to the finalized block.
        FinalizedWithTrees::new(best_chain_root, root_treestate)
    }

    /// Commit block to the non-finalized state, on top of:
    /// - an existing chain's tip, or
    /// - a newly forked chain.
    #[tracing::instrument(level = "debug", skip(self, finalized_state, prepared))]
    pub fn commit_block(
        &mut self,
        prepared: PreparedBlock,
        finalized_state: &ZebraDb,
    ) -> Result<(), ValidateContextError> {
        let parent_hash = prepared.block.header.previous_block_hash;
        let (height, hash) = (prepared.height, prepared.hash);

        let parent_chain = self.parent_chain(parent_hash)?;

        // If the block is invalid, return the error,
        // and drop the cloned parent Arc, or newly created chain fork.
        let modified_chain = self.validate_and_commit(parent_chain, prepared, finalized_state)?;

        // If the block is valid:
        // - add the new chain fork or updated chain to the set of recent chains
        // - remove the parent chain, if it was in the chain set
        //   (if it was a newly created fork, it won't be in the chain set)
        self.chain_set.insert(modified_chain);
        self.chain_set
            .retain(|chain| chain.non_finalized_tip_hash() != parent_hash);

        self.update_metrics_for_committed_block(height, hash);

        Ok(())
    }

    /// Commit block to the non-finalized state as a new chain where its parent
    /// is the finalized tip.
    #[tracing::instrument(level = "debug", skip(self, finalized_state, prepared))]
    #[allow(clippy::unwrap_in_result)]
    pub fn commit_new_chain(
        &mut self,
        prepared: PreparedBlock,
        finalized_state: &ZebraDb,
    ) -> Result<(), ValidateContextError> {
        let finalized_tip_height = finalized_state.finalized_tip_height();

        // TODO: fix tests that don't initialize the finalized state
        #[cfg(not(test))]
        let finalized_tip_height = finalized_tip_height.expect("finalized state contains blocks");
        #[cfg(test)]
        let finalized_tip_height = finalized_tip_height.unwrap_or(zebra_chain::block::Height(0));

        let chain = Chain::new(
            self.network,
            finalized_tip_height,
            finalized_state.sprout_note_commitment_tree(),
            finalized_state.sapling_note_commitment_tree(),
            finalized_state.orchard_note_commitment_tree(),
            finalized_state.history_tree(),
            finalized_state.finalized_value_pool(),
        );
        let (height, hash) = (prepared.height, prepared.hash);

        // If the block is invalid, return the error, and drop the newly created chain fork
        let chain = self.validate_and_commit(Arc::new(chain), prepared, finalized_state)?;

        // If the block is valid, add the new chain fork to the set of recent chains.
        self.chain_set.insert(chain);
        self.update_metrics_for_committed_block(height, hash);

        Ok(())
    }

    /// Contextually validate `prepared` using `finalized_state`.
    /// If validation succeeds, push `prepared` onto `new_chain`.
    ///
    /// `new_chain` should start as a clone of the parent chain fork,
    /// or the finalized tip.
    #[tracing::instrument(level = "debug", skip(self, finalized_state, new_chain))]
    fn validate_and_commit(
        &mut self,
        new_chain: Arc<Chain>,
        prepared: PreparedBlock,
        finalized_state: &ZebraDb,
    ) -> Result<Arc<Chain>, ValidateContextError> {

        // komodo: get chain tip last block time to calc interest
        let last_block_time = Self::komodo_get_last_block_time(finalized_state, new_chain.clone(), &prepared);

        // Reads from disk
        //
        // TODO: if these disk reads show up in profiles, run them in parallel, using std::thread::spawn()
        let spent_utxos = check::utxo::transparent_spend(
            self.network,
            &prepared,
            last_block_time,
            &new_chain.unspent_utxos(),
            &new_chain.spent_utxos,
            finalized_state,
        )?;

        // Reads from disk
        check::anchors::block_sapling_orchard_anchors_refer_to_final_treestates(
            finalized_state,
            &new_chain,
            &prepared,
        )?;

        // Reads from disk
        let sprout_final_treestates = check::anchors::block_fetch_sprout_final_treestates(
            finalized_state,
            &new_chain,
            &prepared,
        ); 

        // Quick check that doesn't read from disk
        let contextual = ContextuallyValidBlock::with_block_and_spent_utxos(
            self.network,
            prepared.clone(),
            last_block_time,
            spent_utxos.clone(),    // TODO: check, this is mutable
        )
        .map_err(|value_balance_error| {
            ValidateContextError::CalculateBlockChainValueChange {
                value_balance_error,
                height: prepared.height,
                block_hash: prepared.hash,
                transaction_count: prepared.block.transactions.len(),
                spent_utxo_count: spent_utxos.len(),
            }
        })?;

        let new_chain = Self::validate_and_update_parallel(new_chain, contextual, sprout_final_treestates)?;

        let utxos = utxos_from_ordered_utxos(spent_utxos);
        let outputs = outputs_from_utxos(utxos);

        // komodo checks related to notarisation:
        self.komodo_checkpoint(&prepared)?;     // prevent new blocks to be added from below last notarised height
        self.komodo_check_fork_is_valid(&new_chain)?;    // prevent existing branches without nota to grow 
        self.komodo_find_block_nota_and_update_last(&prepared.block, &outputs, &prepared.height); // try to find nota in the new block and set it as the latest nota

        Ok(new_chain)
    }

    /// Validate `contextual` and update `new_chain`, doing CPU-intensive work in parallel batches.
    #[allow(clippy::unwrap_in_result)]
    #[tracing::instrument(skip(new_chain, sprout_final_treestates))]
    fn validate_and_update_parallel(
        new_chain: Arc<Chain>,
        contextual: ContextuallyValidBlock,
        sprout_final_treestates: HashMap<sprout::tree::Root, Arc<sprout::tree::NoteCommitmentTree>>,
    ) -> Result<Arc<Chain>, ValidateContextError> {
        let mut block_commitment_result = None;
        let mut sprout_anchor_result = None;
        let mut chain_push_result = None;

        // Clone function arguments for different threads
        let block = contextual.block.clone();
        let network = new_chain.network();
        let history_tree = new_chain.history_block_commitment_tree();
        let sapling_root = new_chain.sapling_note_commitment_tree().root();

        let block2 = contextual.block.clone();
        let height = contextual.height;
        let transaction_hashes = contextual.transaction_hashes.clone();

        rayon::in_place_scope_fifo(|scope| {
            scope.spawn_fifo(|_scope| {
                block_commitment_result = Some(check::block_commitment_is_valid_for_chain_history(
                    block,
                    network,
                    &history_tree,
                    &sapling_root,
                ));
            });

            scope.spawn_fifo(|_scope| {
                sprout_anchor_result =
                    Some(check::anchors::block_sprout_anchors_refer_to_treestates(
                        sprout_final_treestates,
                        block2,
                        transaction_hashes,
                        height,
                    ));
            });

            // We're pretty sure the new block is valid,
            // so clone the inner chain if needed, then add the new block.
            //
            // Pushing a block onto a Chain can launch additional parallel batches.
            // TODO: should we pass _scope into Chain::push()?
            scope.spawn_fifo(|_scope| {
                let new_chain = Arc::try_unwrap(new_chain)
                    .unwrap_or_else(|shared_chain| (*shared_chain).clone());
                chain_push_result = Some(new_chain.push(contextual).map(Arc::new));
            });
        });

        // Don't return the updated Chain unless all the parallel results were Ok
        block_commitment_result.expect("scope has finished")?;
        sprout_anchor_result.expect("scope has finished")?;

        chain_push_result.expect("scope has finished")
    }

    /// Returns the length of the non-finalized portion of the current best chain.
    pub fn best_chain_len(&self) -> u32 {
        self.best_chain()
            .expect("only called after inserting a block")
            .blocks
            .len() as u32
    }

    /// Returns `true` if `hash` is contained in the non-finalized portion of any
    /// known chain.
    #[allow(dead_code)]
    pub fn any_chain_contains(&self, hash: &block::Hash) -> bool {
        self.chain_set
            .iter()
            .rev()
            .any(|chain| chain.height_by_hash.contains_key(hash))
    }

    /// Removes and returns the first chain satisfying the given predicate.
    ///
    /// If multiple chains satisfy the predicate, returns the chain with the highest difficulty.
    /// (Using the tip block hash tie-breaker.)
    fn find_chain<P>(&mut self, mut predicate: P) -> Option<&Arc<Chain>>
    where
        P: FnMut(&Chain) -> bool,
    {
        // Reverse the iteration order, to find highest difficulty chains first.
        self.chain_set.iter().rev().find(|chain| predicate(chain))
    }

    /// Returns the [`transparent::Utxo`] pointed to by the given
    /// [`transparent::OutPoint`] if it is present in any chain.
    ///
    /// UTXOs are returned regardless of whether they have been spent.
    pub fn any_utxo(&self, outpoint: &transparent::OutPoint) -> Option<transparent::Utxo> {
        self.chain_set
            .iter()
            .rev()
            .find_map(|chain| chain.created_utxo(outpoint))
    }

    /// Returns the `block` with the given hash in any chain.
    pub fn any_block_by_hash(&self, hash: block::Hash) -> Option<Arc<Block>> {
        for chain in self.chain_set.iter().rev() {
            if let Some(prepared) = chain
                .height_by_hash
                .get(&hash)
                .and_then(|height| chain.blocks.get(height))
            {
                return Some(prepared.block.clone());
            }
        }

        None
    }

    /// Returns the hash for a given `block::Height` if it is present in the best chain.
    #[allow(dead_code)]
    pub fn best_hash(&self, height: block::Height) -> Option<block::Hash> {
        self.best_chain()?
            .blocks
            .get(&height)
            .map(|prepared| prepared.hash)
    }

    /// Returns the tip of the best chain.
    #[allow(dead_code)]
    pub fn best_tip(&self) -> Option<(block::Height, block::Hash)> {
        let best_chain = self.best_chain()?;
        let height = best_chain.non_finalized_tip_height();
        let hash = best_chain.non_finalized_tip_hash();

        Some((height, hash))
    }

    /// Returns the block at the tip of the best chain.
    #[allow(dead_code)]
    pub fn best_tip_block(&self) -> Option<&ContextuallyValidBlock> {
        let best_chain = self.best_chain()?;

        best_chain.tip_block()
    }

    /// Returns the height of `hash` in the best chain.
    #[allow(dead_code)]
    pub fn best_height_by_hash(&self, hash: block::Hash) -> Option<block::Height> {
        let best_chain = self.best_chain()?;
        let height = *best_chain.height_by_hash.get(&hash)?;
        Some(height)
    }

    /// Returns the height of `hash` in any chain.
    pub fn any_height_by_hash(&self, hash: block::Hash) -> Option<block::Height> {
        for chain in self.chain_set.iter().rev() {
            if let Some(height) = chain.height_by_hash.get(&hash) {
                return Some(*height);
            }
        }

        None
    }

    /// Returns `true` if the best chain contains `sprout_nullifier`.
    #[cfg(test)]
    pub fn best_contains_sprout_nullifier(&self, sprout_nullifier: &sprout::Nullifier) -> bool {
        self.best_chain()
            .map(|best_chain| best_chain.sprout_nullifiers.contains(sprout_nullifier))
            .unwrap_or(false)
    }

    /// Returns `true` if the best chain contains `sapling_nullifier`.
    #[cfg(test)]
    pub fn best_contains_sapling_nullifier(
        &self,
        sapling_nullifier: &zebra_chain::sapling::Nullifier,
    ) -> bool {
        self.best_chain()
            .map(|best_chain| best_chain.sapling_nullifiers.contains(sapling_nullifier))
            .unwrap_or(false)
    }

    /// Returns `true` if the best chain contains `orchard_nullifier`.
    #[cfg(test)]
    pub fn best_contains_orchard_nullifier(
        &self,
        orchard_nullifier: &zebra_chain::orchard::Nullifier,
    ) -> bool {
        self.best_chain()
            .map(|best_chain| best_chain.orchard_nullifiers.contains(orchard_nullifier))
            .unwrap_or(false)
    }

    /// Return the non-finalized portion of the current best chain.
    /// Modified by Komodo to return the best chain which has the last nota
    /// if no last nota in any chain the first best chain is returned
    pub(crate) fn best_chain(&self) -> Option<&Arc<Chain>> {
        let mut iter = self.chain_set.iter();
        let mut next_chain = iter.next_back();
        let chain_0 = next_chain;
        let mut has_last_nota = false;
        if let Some(last_nota_block_hash) = self.last_nota_block_hash {
            while let Some(chain) = next_chain {
                if let Some(_height) = chain.height_by_hash.get(&last_nota_block_hash) { // chain has nota
                    has_last_nota = true;
                    break;
                }  
                next_chain = iter.next();
            }
        }
        
        if has_last_nota {
            next_chain
        } else {
            chain_0
        }
    }

    /// Return the chain whose tip block hash is `parent_hash`.
    ///
    /// The chain can be an existing chain in the non-finalized state, or a freshly
    /// created fork.
    #[allow(clippy::unwrap_in_result)]
    fn parent_chain(
        &mut self,
        parent_hash: block::Hash,
    ) -> Result<Arc<Chain>, ValidateContextError> {
        match self.find_chain(|chain| chain.non_finalized_tip_hash() == parent_hash) {
            // Clone the existing Arc<Chain> in the non-finalized state
            Some(chain) => Ok(chain.clone()),
            // Create a new fork
            None => {
                // Check the lowest difficulty chains first,
                // because the fork could be closer to their tip.
                let fork_chain = self
                    .chain_set
                    .iter()
                    .find_map(|chain| chain.fork(parent_hash))
                    .ok_or(ValidateContextError::NotReadyToBeCommitted)?;

                Ok(Arc::new(fork_chain))
            }
        }
    }

    /// Update the metrics after `block` is committed
    fn update_metrics_for_committed_block(&self, height: block::Height, hash: block::Hash) {
        #[cfg(feature = "getblocktemplate-rpcs")]
        if !self.should_count_metrics {
            return;
        }

        metrics::counter!("state.memory.committed.block.count", 1);
        metrics::gauge!("state.memory.committed.block.height", height.0 as f64);

        if self
            .best_chain()
            .unwrap()
            .blocks
            .iter()
            .next_back()
            .unwrap()
            .1
            .hash
            == hash
        {
            metrics::counter!("state.memory.best.committed.block.count", 1);
            metrics::gauge!("state.memory.best.committed.block.height", height.0 as f64);
        }

        self.update_metrics_for_chains();
    }

    /// Update the metrics after `self.chain_set` is modified
    fn update_metrics_for_chains(&self) {
        #[cfg(feature = "getblocktemplate-rpcs")]
        if !self.should_count_metrics {
            return;
        }

        metrics::gauge!("state.memory.chain.count", self.chain_set.len() as f64);
        metrics::gauge!(
            "state.memory.best.chain.length",
            self.best_chain_len() as f64,
        );
    }

    /// check if block has a back KMD nota and update latest nota in the mem state
    fn komodo_find_block_nota_and_update_last(&mut self, block: &Block, spent_outputs: &HashMap<transparent::OutPoint, transparent::Output>, height: &block::Height) {
        match (komodo_block_has_notarisation_tx(self.network, block, spent_outputs, height), self.last_nota.as_ref()) {
            (Some(found_nota), Some(last_nota)) => {
                if last_nota.notarised_height < found_nota.notarised_height {
                    debug!("komodo found update nota at height={:?}, last notarised height={:?}", height, found_nota.notarised_height);
                    self.last_nota = Some(found_nota);
                    self.last_nota_block_hash = Some(block.hash());
                }
            },
            (Some(found_nota), None) =>  {
                debug!("komodo found new nota at height={:?}, last notarised height={:?}", height, found_nota.notarised_height);
                self.last_nota = Some(found_nota);
                self.last_nota_block_hash = Some(block.hash());
            },
            (None, _) => (),
        }
    }

    /// check if new chain is notarised and allowed to fork
    /// it should not fork below the last notarised block
    pub fn komodo_check_fork_is_valid(&self, chain_with_new_block: &Chain) -> Result<(), ValidateContextError> {

        if let Some(last_nota) = &self.last_nota {
            debug!("komodo_check_fork_is_valid chain_new height={:?} hash={:?} last_nota.height={:?}", chain_with_new_block.non_finalized_tip_height(), chain_with_new_block.non_finalized_tip_hash(), last_nota.notarised_height);
            if let Some(best_chain) = self.best_chain() {
                
                if let Some(chain_with_nota) = self.chain_set.iter().rev().find(|chain| chain.height_by_hash.contains_key(&last_nota.notarised_block_hash))  {

                    trace!("komodo_check_fork_is_valid best_chain.tip={:?} hash={:?}", best_chain.non_finalized_tip_height(), best_chain.non_finalized_tip_hash());
                    // find the fork point
                    // I think it is important to start search from the tip (in rev order) 
                    // as the bottom part of the chain has many common blocks with the best_chain because both grow from the finalized tip
                    if let Some(fork) = chain_with_new_block.blocks.iter().rev().find(|pair| chain_with_nota.height_by_hash.contains_key(&pair.1.hash) ) {

                        // truncate the new chain's bottom blocks below the fork point (and leave only block hashes in the top part):
                        let block_hashes_truncated = chain_with_new_block.blocks.iter()
                            .skip_while(|e| e.1 != fork.1)
                            .map(|p| (p.0, p.1.hash))
                            .collect::<Vec<_>>();
                        trace!("komodo_check_fork_is_valid block_hashes_truncated={:?}", block_hashes_truncated);

                        let new_has_nota = block_hashes_truncated.iter().find(|pair| pair.1 == last_nota.notarised_block_hash).is_some();
                    
                        debug!(
                            chain_with_new_block_non_fin_height = chain_with_new_block.non_finalized_tip_height().0,
                            best_chain_non_fin_height = best_chain.non_finalized_tip_height().0,
                            new_chain_has_last_nota = chain_with_new_block.height_by_hash.contains_key(&last_nota.notarised_block_hash),
                            blocks_truncated_has_last_nota = new_has_nota,
                            best_chain_tip_height_over_notarised_height = best_chain.non_finalized_tip_height() > last_nota.notarised_height,
                            is_fork_below_notarised_height = fork.0 < &last_nota.notarised_height,
                            best_chain_root_height = best_chain.non_finalized_tip_height().0,
                            fork_height = fork.0.0,
                            last_notarised_height = last_nota.notarised_height.0,
                            new_chain_has_more_power = chain_with_new_block > best_chain,
                            "komodo checking notarised height for new chain:"
                        );

                        // The condition for forks below the last notarised height:
                        // if chain_with_new_block has more work than best_chain (that is, a new best chain candidate)
                        // and chain_with_new_block does not contain the last nota
                        // and fork.height < last_nota.height
                        // and best_chain.height > last_nota.height 
                        // then do not allow this block and this fork:

                        if chain_with_new_block > best_chain &&  // new chain has more work

                            // There is no a similar check in ActivateBestChainStep() because in there notas are processed only when the chain is activated,
                            // so it is guaranteed that the known 'latest' nota is in the active chain.
                            // Since the current fix in zebra the best_chain also always has the last nota 
                            // so this is practically impossible if a new chain has the last nota and is different from the best chain:
                            // !new_has_nota  && // no error if the new chain contains the last nota 

                            best_chain.non_finalized_tip_height() > last_nota.notarised_height && // not sure why this condition is needed as assumed best chain could not be built without notas in it
                            fork.0 < &last_nota.notarised_height {  
                            return Err(ValidateContextError::KomodoInvalidNotarisedChain(chain_with_new_block.non_finalized_tip_hash(), *fork.0, last_nota.notarised_height));
                        }
                    }
                    else {
                        // this should not happen actually
                        error!("komodo internal error: could not find fork point for chain {:?}", chain_with_new_block);
                    }
                } else {
                    debug!("komodo: last nota not found in non-finalized branches, assuming it is in finalized state");
                }
            }
        }
        Ok(())
    }

    /// komodo notarisation checks: new block height must be >= the last notarised height
    pub fn komodo_checkpoint(&self, prepared: &PreparedBlock) -> Result<(), ValidateContextError> {
        if let Some(last_nota) = &self.last_nota {
            tracing::debug!("komodo_checkpoint prepared.height={:?}, last_nota_height={:?}, last_nota_block_hash={:?}", prepared.height, last_nota.notarised_height, last_nota.notarised_block_hash);
            // verify that the block info returned from komodo_notariseddata matches the actual block
            if let Some(last_nota_ht) = self.best_height_by_hash(last_nota.notarised_block_hash) {
                if last_nota_ht == last_nota.notarised_height { // if notarised_hash not in chain, reorg
                    if prepared.height < last_nota.notarised_height {
                        // forked chain %d older than last notarised (height %d) vs %d" case
                        return Err(ValidateContextError::KomodoInvalidNotarisedChain(
                            prepared.hash,
                            (prepared.height - 1).unwrap_or(block::Height(0)), // fork point is the latest common block
                            last_nota.notarised_height
                        ));
                    } else if prepared.height == last_nota.notarised_height && prepared.hash != last_nota.notarised_block_hash {
                        // [%s] nHeight.%d == notarised_HEIGHT.%d, diff hash case
                        return Err(ValidateContextError::KomodoInvalidNotarisedChain(
                            prepared.hash,
                            (prepared.height - 1).unwrap_or(block::Height(0)),
                            last_nota.notarised_height
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// komodo: get chain tip last block time to calc interest
    fn komodo_get_last_block_time(finalized_state: &ZebraDb, chain: Arc<Chain>, prepared: &PreparedBlock) -> Option<DateTime<Utc>> {
            
        if prepared.height > block::Height(0)  {
            if let Some(tip) = chain.tip_block() { 
                Some(tip.block.header.time) 
            } else { 
                // if new_chain is empty then get the finalized tip (in zebra any forks could exist only in the non-finalized part):
                if let Some(tip) = finalized_state.tip_block()  {
                    Some(tip.header.time)
                }
                else {
                    // let's not panic here because several unit tests create a state started from arbitrary height
                    info!("komodo could not get chain tip for block={:?}", prepared.height); 
                    None
                }
            }
        } else {
            None
        }
    }
}

