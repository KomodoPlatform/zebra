//! Block test vectors

#![allow(missing_docs)]

use hex::FromHex;
use lazy_static::lazy_static;

use std::{collections::BTreeMap, convert::TryInto};

trait ReverseCollection {
    /// Return a reversed copy of this collection
    fn rev(self) -> Self;
}

impl ReverseCollection for [u8; 32] {
    fn rev(mut self) -> [u8; 32] {
        self.reverse();
        self
    }
}

lazy_static! {
    /// All block test vectors
    pub static ref KMDBLOCKS: Vec<&'static [u8]> = KMDMAINNET_BLOCKS
        .iter()
        .map(|(_height, block)| *block)
        .collect();

    /// Continuous mainnet blocks, indexed by height
    ///
    /// Contains the continuous blockchain from genesis onwards.  Stops at the
    /// first gap in the chain.
    pub static ref CONTINUOUS_KMDMAINNET_BLOCKS: BTreeMap<u32, &'static [u8]> = KMDMAINNET_BLOCKS
        .iter()
        .enumerate()
        .take_while(|(i, (height, _block))| *i == **height as usize)
        .map(|(_i, (height, block))| (*height, *block))
        .collect();

    // Update these lists of blocks when you add new block test vectors to
    // this file
    //
    // We use integer heights in these maps, to avoid a dependency on zebra_chain

    /// KMD Mainnet blocks, indexed by height
    ///
    /// This is actually a bijective map, the tests ensure that values are unique.
    pub static ref KMDMAINNET_BLOCKS: BTreeMap<u32, &'static [u8]> = [
            // Genesis
            (0, BLOCK_KMDMAINNET_GENESIS_BYTES.as_ref()),

            // BeforeOverwinter
            (1, BLOCK_KMDMAINNET_1_BYTES.as_ref()),
            (2, BLOCK_KMDMAINNET_2_BYTES.as_ref()),
            (3, BLOCK_KMDMAINNET_3_BYTES.as_ref()),
            (4, BLOCK_KMDMAINNET_4_BYTES.as_ref()),
            (5, BLOCK_KMDMAINNET_5_BYTES.as_ref()),
            (6, BLOCK_KMDMAINNET_6_BYTES.as_ref()),
            (7, BLOCK_KMDMAINNET_7_BYTES.as_ref()),
            (8, BLOCK_KMDMAINNET_8_BYTES.as_ref()),
            (9, BLOCK_KMDMAINNET_9_BYTES.as_ref()),
            (10, BLOCK_KMDMAINNET_10_BYTES.as_ref()),

            (899_012, BLOCK_KMDMAINNET_899012_BYTES.as_ref()),  // pow block tx v1

            // do not add easy mined blocks here as difficulty tests would fail
            // (1_140_500, BLOCK_KMDMAINNET_1140500_BYTES.as_ref()),  // easy-mining block tx v4

            (1_140_507, BLOCK_KMDMAINNET_1140507_BYTES.as_ref()),  // pow block tx v4, coibase only


        ].iter().cloned().collect();

    // KMD Mainnet

    // Genesis/BeforeOverwinter
    // for i in `seq 0 9`; do
    //     komodo-cli getblock $i 0 > block-kmdmain--000-00$i.txt
    // done
    pub static ref BLOCK_KMDMAINNET_GENESIS_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-000.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_1_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-001.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_2_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-002.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_3_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-003.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_4_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-004.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_5_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-005.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_6_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-006.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_7_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-007.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_8_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-008.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDMAINNET_9_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-009.txt").trim())
        .expect("Block bytes are in valid hex representation");
    // komodo-cli getblock 10 0 > block-kmdmain-0-000-010.txt
    pub static ref BLOCK_KMDMAINNET_10_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-000-010.txt").trim())
        .expect("Block bytes are in valid hex representation");
   
    // Some block before overwinter:
    pub static ref BLOCK_KMDMAINNET_899012_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-0-899-012.txt").trim())
        .expect("Block bytes are in valid hex representation");

    // Block just before overwinter/sapling
    pub static ref BLOCK_KMDMAINNET_1140408_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-1-140-408.txt").trim())
        .expect("Block bytes are in valid hex representation");

    // Overwinter, Sapling

    // easy-mined block
    pub static ref BLOCK_KMDMAINNET_1140500_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-1-140-500.txt").trim())
        .expect("Block bytes are in valid hex representation");

    // pow block, coinbase only
    pub static ref BLOCK_KMDMAINNET_1140507_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-1-140-507.txt").trim())
        .expect("Block bytes are in valid hex representation");

    // block with nota (asset chain) TODO delete
    pub static ref BLOCK_KMDMAINNET_3140508_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdmain-3-140-508.txt").trim())
        .expect("Block bytes are in valid hex representation");

    // RICK sample blocks:
    pub static ref BLOCK_RICK_899012_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-rick-0-899-012.txt").trim())
        .expect("Block bytes are in valid hex representation");

    /// KMD Testnet sample, generated by Decker
    /// 
    /// Continuous testnet blocks, indexed by height
    pub static ref CONTINUOUS_KMDTESTNET_BLOCKS: BTreeMap<u32, &'static [u8]> = KMDTESTNET_BLOCKS
        .iter()
        .enumerate()
        .take_while(|(i, (height, _block))| *i == **height as usize)
        .map(|(_i, (height, block))| (*height, *block))
        .collect();

    /// KMD Testnet blocks, indexed by height
    ///
    /// This is actually a bijective map, the tests ensure that values are unique.
    pub static ref KMDTESTNET_BLOCKS: BTreeMap<u32, &'static [u8]> = [
            // Genesis
            (0, BLOCK_KMDTESTNET_GENESIS_BYTES.as_ref()),

            // BeforeOverwinter
            (1, BLOCK_KMDTESTNET_1_BYTES.as_ref()),
            (2, BLOCK_KMDTESTNET_2_BYTES.as_ref()),
            (3, BLOCK_KMDTESTNET_3_BYTES.as_ref()),
            (4, BLOCK_KMDTESTNET_4_BYTES.as_ref()),
            (5, BLOCK_KMDTESTNET_5_BYTES.as_ref()),
            (6, BLOCK_KMDTESTNET_6_BYTES.as_ref()),
            (7, BLOCK_KMDTESTNET_7_BYTES.as_ref()),
            (8, BLOCK_KMDTESTNET_8_BYTES.as_ref()),
            (9, BLOCK_KMDTESTNET_9_BYTES.as_ref()),
            (10, BLOCK_KMDTESTNET_10_BYTES.as_ref()),

            // Sapling
            (126, BLOCK_KMDTESTNET_0000126_BYTES.as_ref()),  
            (127, BLOCK_KMDTESTNET_0000127_BYTES.as_ref()),  
        ].iter().cloned().collect();


    pub static ref BLOCK_KMDTESTNET_GENESIS_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-000.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_1_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-001.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_2_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-002.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_3_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-003.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_4_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-004.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_5_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-005.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_6_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-006.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_7_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-007.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_8_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-008.txt").trim())
        .expect("Block bytes are in valid hex representation");
    pub static ref BLOCK_KMDTESTNET_9_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-009.txt").trim())
        .expect("Block bytes are in valid hex representation");
    // komodo-cli getblock 10 0 > block-kmdtest-0-000-010.txt
    pub static ref BLOCK_KMDTESTNET_10_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-010.txt").trim())
        .expect("Block bytes are in valid hex representation");


    pub static ref BLOCK_KMDTESTNET_0000126_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-126.txt").trim())
        .expect("Block bytes are in valid hex representation");

    // block with nota
    pub static ref BLOCK_KMDTESTNET_0000127_BYTES: Vec<u8> =
        <Vec<u8>>::from_hex(include_str!("block-kmdtest-0-000-127.txt").trim())
        .expect("Block bytes are in valid hex representation");
 
}

#[cfg(test)]
mod test {
    use super::*;

    use std::collections::HashSet;

    use crate::init;

    #[test]
    fn block_test_vectors_unique() {
        init();

        let block_count = KMDBLOCKS.len();
        let block_set: HashSet<_> = KMDBLOCKS.iter().collect();

        // putting the same block in two files is an easy mistake to make
        assert_eq!(
            block_count,
            block_set.len(),
            "block test vectors must be unique"
        );

        // final sapling roots can be duplicated if a block has no sapling spends or outputs
    }

    /// Make sure we use all the test vectors in the lists.
    ///
    /// We're using lazy_static! and combinators, so it would be easy to make this mistake.
    #[ignore = "fix for komodo block set"] 
    #[test]
    fn block_test_vectors_count() {
        init();

        assert!(
            KMDMAINNET_BLOCKS.len() > 30,
            "there should be a reasonable number of mainnet block test vectors"
        );
    }
}
