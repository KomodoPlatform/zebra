//! Checkpoint lists for checkpoint-based block verification
//!
//! Each checkpoint consists of a coinbase height and block header hash.
//!
//! Checkpoints can be used to verify their ancestors, by chaining backwards
//! to another checkpoint, via each block's parent block hash.

#[cfg(test)]
mod tests;

use crate::BoxError;

use std::{
    collections::{BTreeMap, HashSet},
    ops::RangeBounds,
    str::FromStr,
};

use zebra_chain::block;
use zebra_chain::parameters::{genesis_hash, Network};

/// The hard-coded checkpoints for mainnet, generated using the
/// `zebra-checkpoints` tool.
///
/// To regenerate the latest checkpoints, use the following commands:
/// ```sh
/// LAST_CHECKPOINT=$(tail -1 main-checkpoints.txt | cut -d' ' -f1)
/// echo "$LAST_CHECKPOINT"
/// zebra-checkpoints --cli /path/to/zcash-cli --last-checkpoint "$LAST_CHECKPOINT" >> main-checkpoints.txt &
/// tail -f main-checkpoints.txt
/// ```
///
/// See the checkpoints [./README.md] for more details.
const MAINNET_CHECKPOINTS: &str = include_str!("main-checkpoints.txt");

/// The hard-coded checkpoints for testnet, generated using the
/// `zebra-checkpoints` tool.
///
/// To use testnet, use the testnet checkpoints file, and run
/// `zebra-checkpoints [other args] -- -testnet`.
///
/// See [`MAINNET_CHECKPOINTS`] for detailed `zebra-checkpoints` usage
/// information.
/// const TESTNET_CHECKPOINTS: &str = include_str!("test-checkpoints.txt");

// KMD working testnets
const TESTNET_CHECKPOINTS: &str =    //ver4 testnet dimxy genesis
                                    // "0 00040fe8ec8471911baa1db1266ea15dd06b4a8a5c453883c000b031973dce08\n\
                                    // 64 0480916fa2b9a1df4bf94bf406c2df7c424756d0bc28969f5465731031496946\n"; 

                                    "0 00040fe8ec8471911baa1db1266ea15dd06b4a8a5c453883c000b031973dce08\n\
                                    64 00a8d9d7d3ae6f6a264a75a2d31b8a4c980b8517ec3a190860dcf5fb27e442a7\n";

                                     // decker genesis dimxy blocks 's testnet
                                     // "0 e46e999d6de5ef95427ea51b8242ef7949de22b340b6a484ff154b9991ec9452\n\
                                     // 64 0514e0377c6186527fd3fca318dc8361b174b1105980f9cc01792ff6a7708c0d\n";

                                     // decker testnet for bad forks
                                     //"0 e46e999d6de5ef95427ea51b8242ef7949de22b340b6a484ff154b9991ec9452\n\
                                     //64 028d36cdd15e08d178096b9cb584edbcbf84d0ae8d1d646b7089ef743a11fc6d\n";   

/// A list of block height and hash checkpoints.
///
/// Checkpoints should be chosen to avoid forks or chain reorganizations,
/// which only happen in the last few hundred blocks in the chain.
/// (zcashd allows chain reorganizations up to 99 blocks, and prunes
/// orphaned side-chains after 288 blocks.)
///
/// This is actually a bijective map, but since it is read-only, we use a
/// BTreeMap, and do the value uniqueness check on initialisation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CheckpointList(BTreeMap<block::Height, block::Hash>);

impl FromStr for CheckpointList {
    type Err = BoxError;

    /// Parse a string into a CheckpointList.
    ///
    /// Each line has one checkpoint, consisting of a `block::Height` and
    /// `block::Hash`, separated by a single space.
    ///
    /// Assumes that the provided genesis checkpoint is correct.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut checkpoint_list: Vec<(block::Height, block::Hash)> = Vec::new();

        for checkpoint in s.lines() {
            let fields = checkpoint.split(' ').collect::<Vec<_>>();
            if let [height, hash] = fields[..] {
                checkpoint_list.push((height.parse()?, hash.parse()?));
            } else {
                Err(format!("Invalid checkpoint format: expected 2 space-separated fields but found {}: '{}'", fields.len(), checkpoint))?;
            };
        }

        CheckpointList::from_list(checkpoint_list)
    }
}

impl CheckpointList {
    /// Returns the hard-coded checkpoint list for `network`.
    pub fn new(network: Network) -> Self {
        // parse calls CheckpointList::from_list
        let checkpoint_list: CheckpointList = match network {
            Network::Mainnet => MAINNET_CHECKPOINTS
                .parse()
                .expect("Hard-coded Mainnet checkpoint list parses and validates"),
            Network::Testnet => TESTNET_CHECKPOINTS
                .parse()
                .expect("Hard-coded Testnet checkpoint list parses and validates"),
        };

        match checkpoint_list.hash(block::Height(0)) {
            Some(hash) if hash == genesis_hash(network) => checkpoint_list,
            Some(_) => {
                panic!("The hard-coded genesis checkpoint does not match the network genesis hash")
            }
            None => unreachable!("Parser should have checked for a missing genesis checkpoint"),
        }
    }

    /// Create a new checkpoint list for `network` from `checkpoint_list`.
    ///
    /// Assumes that the provided genesis checkpoint is correct.
    ///
    /// Checkpoint heights and checkpoint hashes must be unique.
    /// There must be a checkpoint for a genesis block at block::Height 0.
    /// (All other checkpoints are optional.)
    pub(crate) fn from_list(
        list: impl IntoIterator<Item = (block::Height, block::Hash)>,
    ) -> Result<Self, BoxError> {
        // BTreeMap silently ignores duplicates, so we count the checkpoints
        // before adding them to the map
        let original_checkpoints: Vec<(block::Height, block::Hash)> = list.into_iter().collect();
        let original_len = original_checkpoints.len();

        let checkpoints: BTreeMap<block::Height, block::Hash> =
            original_checkpoints.into_iter().collect();

        // Check that the list starts with the correct genesis block
        match checkpoints.iter().next() {
            Some((block::Height(0), hash))
                if (hash == &genesis_hash(Network::Mainnet)
                    || hash == &genesis_hash(Network::Testnet)) => {}
            Some((block::Height(0), _)) => {
                Err("the genesis checkpoint does not match the Mainnet or Testnet genesis hash")?
            }
            Some(_) => Err("checkpoints must start at the genesis block height 0")?,
            None => Err("there must be at least one checkpoint, for the genesis block")?,
        };

        // This check rejects duplicate heights, whether they have the same or
        // different hashes
        if checkpoints.len() != original_len {
            Err("checkpoint heights must be unique")?;
        }

        let block_hashes: HashSet<&block::Hash> = checkpoints.values().collect();
        if block_hashes.len() != original_len {
            Err("checkpoint hashes must be unique")?;
        }

        // Make sure all the hashes are valid. In Bitcoin, [0; 32] is the null
        // hash. It is also used as the parent hash of genesis blocks.
        if block_hashes.contains(&block::Hash([0; 32])) {
            Err("checkpoint list contains invalid checkpoint hash: found null hash")?;
        }

        let checkpoints = CheckpointList(checkpoints);
        if checkpoints.max_height() > block::Height::MAX {
            Err("checkpoint list contains invalid checkpoint: checkpoint height is greater than the maximum block height")?;
        }

        Ok(checkpoints)
    }

    /// Return true if there is a checkpoint at `height`.
    ///
    /// See `BTreeMap::contains_key()` for details.
    pub fn contains(&self, height: block::Height) -> bool {
        self.0.contains_key(&height)
    }

    /// Returns the hash corresponding to the checkpoint at `height`,
    /// or None if there is no checkpoint at that height.
    ///
    /// See `BTreeMap::get()` for details.
    pub fn hash(&self, height: block::Height) -> Option<block::Hash> {
        self.0.get(&height).cloned()
    }

    /// Return the block height of the highest checkpoint in the checkpoint list.
    ///
    /// If there is only a single checkpoint, then the maximum height will be
    /// zero. (The genesis block.)
    pub fn max_height(&self) -> block::Height {
        self.max_height_in_range(..)
            .expect("checkpoint lists must have at least one checkpoint")
    }

    /// Return the block height of the lowest checkpoint in a sub-range.
    pub fn min_height_in_range<R>(&self, range: R) -> Option<block::Height>
    where
        R: RangeBounds<block::Height>,
    {
        self.0.range(range).map(|(height, _)| *height).next()
    }

    /// Return the block height of the highest checkpoint in a sub-range.
    pub fn max_height_in_range<R>(&self, range: R) -> Option<block::Height>
    where
        R: RangeBounds<block::Height>,
    {
        self.0.range(range).map(|(height, _)| *height).next_back()
    }
}
