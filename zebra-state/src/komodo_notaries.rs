//! Komodo notarisation utilities for special block contextual validation
 
use std::borrow::Borrow;
use std::collections::HashMap;

use chrono::{Utc, DateTime, NaiveDateTime, Duration};
use zebra_chain:: {
    komodo_nota::BackNotarisationData,
    parameters::{Network, MAINNET_MAX_FUTURE_BLOCK_TIME, MAINNET_HF22_NOTARIES_PRIORITY_ROTATE_DELTA},
    serialization::ZcashDeserialize,
    transparent,
    block::{self, Block, Height},
};

use zebra_chain::komodo_hardfork::*;
use zebra_chain::komodo_utils::*;

use tracing::error;
use thiserror::Error;

/// max depth to check for duplicate notary miners
pub const NN_DUP_CHECK_DEPTH: usize = 65;

/// Special, notary node generated block errors
#[allow(dead_code, missing_docs)]
#[derive(Error, Debug, PartialEq, Eq)]
pub enum NotaryValidateContextError {

    #[error("special notary block height {0:?}, hash {1:?} not valid ({2:?})")]
    NotaryBlockInvalid(block::Height, block::Hash, String),

    #[error("notary pubkey data error")]
    NotaryPubkeysError(#[from] NotaryDataError),

    #[error("this notary {1:?} already mined recently for this height {0:?}")]
    NotaryAlreadyMinedError(block::Height, NotaryId),

    #[error("notarisation code internal error: {0:?}")]
    NotaryInternalError(String),

    #[error("notarisation rule not implemented: must use checkpont validation")]
    NeedCheckpointValidate(),
}


/// check notary is unique for depth of 'NN_DUP_CHECK_DEPTH' blocks (it's currently the actual rules, since height >= 82000)
fn komodo_check_last_65_blocks_for_dups<C>(network: Network, height: &Height, relevant_chain: &Vec<Block>, notary_id: NotaryId) -> Result<(), NotaryValidateContextError> 
{
    tracing::debug!("komodo_check_last_65_blocks_for_dups enterred for height={:?}", height);

    // if *height >= Height(82000) for mainnet
    if NN::komodo_notaries_new_depth_rule_active(network, height)   {
        if relevant_chain.len() < NN_DUP_CHECK_DEPTH { return Err(NotaryValidateContextError::NotaryInternalError(String::from("relevant chain too small"))); }
        tracing::debug!("komodo_check_last_65_blocks_for_dups relevant_chain heights={:?}", relevant_chain.iter().map(|b| b.hash()).collect::<Vec<_>>());

        let mut has_duplicates = false;
        for block in relevant_chain.into_iter() {
            if let Some(block_pk) = komodo_get_block_pubkey(&block) {
                if let Some(block_notary_id) = NN::komodo_get_notary_id(network, height, &block_pk)? {   
                    if notary_id == block_notary_id {
                        has_duplicates = true;
                        break;
                    }
                }
            }
        }

        // if *height > Height(792000) for mainnet
        if NN::komodo_notaries_check_for_dups_rule_active(network, height)   {
            if has_duplicates {
                return Err(NotaryValidateContextError::NotaryAlreadyMinedError(*height, notary_id));
            }
        }
    }
    else {
        if network == Network::Mainnet {
            error!("komodo_check_last_65_blocks_for_dups {:?} returns error NeedCheckpointValidate", height);
            return Err(NotaryValidateContextError::NeedCheckpointValidate());
        }
    }

    Ok(())
}

/// check the block time rule for easy mined blocks
fn komodo_check_notary_blocktime<C>(network: Network, height: &Height, relevant_chain: &Vec<Block>, block: &Block) -> Result<(), NotaryValidateContextError> 
{
    let tip_block = relevant_chain
    .get(0)
    .expect("state must contain tip block to do contextual validation");

    let blocktime = block.header.time;
    let tip_blocktime = tip_block.borrow().header.time;
    let time_0 = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc);
    let duration_57 = Duration::seconds(57);

    if NN::komodo_notaries_check_blocktime_active(network, height)   {  // if *height > Height(807000)  for mainnet
        if blocktime != time_0 && tip_blocktime != time_0 && blocktime < tip_blocktime + duration_57   {
            error!("komodo_check_notary_blocktime {:?} returns error NotaryBlockInvalid", height);
            return Err(NotaryValidateContextError::NotaryBlockInvalid(*height, block.hash(), String::from("invalid blocktime")));
        }
    }
    Ok(())
}

/// check if a notary_id is in priority list part allowed for second block mining
fn is_second_block_allowed(notary_id: NotaryId, blocktime: DateTime<Utc>, threshold: DateTime<Utc>, delta: i32, v_priority_list: &Vec<u32>) -> Result<bool, NotaryValidateContextError>
{
    if v_priority_list.len() != 64 {
        return Err(NotaryValidateContextError::NotaryInternalError(String::from("invalid priority list")));
    }

    if blocktime >= threshold && delta > 0   {
        if let Ok(pos) = usize::try_from((blocktime - threshold).num_seconds() / delta as i64) {
            if pos < v_priority_list.len()  {
                // if nodeid found in current range of priority -> allow it
                if v_priority_list.iter().take(pos + 1).any(|mid| *mid == notary_id as u32) {
                    return Ok(true);
                }
            }
            else {
                return Ok(true); // if time is bigger than the biggest range -> all nodes allowed
            }
        }
    }
    Ok(false)
}

/// returns true if there is a long gap between mined blocks, to enable to create an extra easy mined block 
fn komodo_check_if_second_block_allowed<C>(network: Network, notary_id: NotaryId, height: &Height, relevant_chain: &Vec<Block>, block: &Block) -> Result<(), NotaryValidateContextError> 
{
    let mut v_priority_list: Vec<u32> = (0..64).collect();
    for block in relevant_chain.iter().rev() {
        if let Some(block_pk) = komodo_get_block_pubkey(&block) {
            if let Some(block_notary_id) = NN::komodo_get_notary_id(network, height, &block_pk)?  {
                if let Some(pos) = v_priority_list.iter().position(|&mid| mid == block_notary_id as u32) {
                    if pos + 1 < v_priority_list.len()  {
                        v_priority_list[pos..].rotate_left(1);
                    }
                }
            }
        }
    }

    let blocktime = block.header.time;
    let tip_blocktime = relevant_chain[0].header.time;
    let max_gap_allowed = MAINNET_MAX_FUTURE_BLOCK_TIME + 1;
    let threshold = tip_blocktime + Duration::seconds(max_gap_allowed);

    if is_second_block_allowed(notary_id, blocktime, threshold, MAINNET_HF22_NOTARIES_PRIORITY_ROTATE_DELTA, &v_priority_list)? {
        tracing::info!("komodo notary hf22 second block allowed for ht={:?}", height);
        return Ok(());
    }
    Err(NotaryValidateContextError::NotaryBlockInvalid(*height, block.hash(), String::from("invalid second block after gap")))
}

/// check if block is notary special and valid for notary new rules for ht >= 84000 for KMD mainnet, 
/// if the block is special and valid then returns true
/// if the block is not special then returns false
/// if the block is special and invalid then throws a error
pub fn komodo_is_special_notary_block<C>(
    block: &Block,
    height: &Height,
    network: Network,
//    finalized_tip_height: Option<Height>,
//    relevant_chain: &Vec<Block>,
    relevant_chain: C,
) -> Result<bool, NotaryValidateContextError>
where
    //C: IntoIterator<Item = Borrow<Block>>,
    C: IntoIterator,
    C::Item: Borrow<Block>,
    C::IntoIter: ExactSizeIterator,
    //<C as IntoIterator>::Item: ExactSizeIterator,
{

    /*if network != Network::Mainnet {
        return Ok(false);
    }*/

    // height >= 34000 for mainnet
    if NN::komodo_notarisation_active(network, height) {

        if let Some(block_pk) = komodo_get_block_pubkey(block) {
        
            if let Some(notary_id) = NN::komodo_get_notary_id(network, height, &block_pk)? {  // low-level error converted to NotaryValidateContextError
                // convert to Vec of block refs for convenience:
                let relevant_chain: Vec<_> = relevant_chain
                .into_iter()
                .map(|b| b.borrow().to_owned())
                .collect::<Vec<Block>>();

                let check_last_65_result = komodo_check_last_65_blocks_for_dups::<C>(network, height, &relevant_chain, notary_id);  // do not return error here

                komodo_check_notary_blocktime::<C>(network, height, &relevant_chain, block)?;  // returns error if blocktime invalid

                if check_last_65_result.is_err() {
                    if NN::komodo_is_gap_after_second_block_allowed(network, height) {
                        komodo_check_if_second_block_allowed::<C>(network, notary_id, height, &relevant_chain, block)?;  // returns error if second block invalid
                    } else {
                        return Err(check_last_65_result.err().unwrap());
                    }
                }
                return Ok(true);
            }
        }
    } else {
        // no need to return error here as notarisation has not begun yet
    }

    Ok(false)  // not a special notary block
}

/// check if block hash a valid notarisation transaction (signed with min number of notaries and with a nota in opreturn)
pub fn komodo_block_has_notarisation_tx(network: Network, block: &Block, spent_outputs: &HashMap<transparent::OutPoint, transparent::Output>, height: &Height) -> Option<BackNotarisationData> 
{
    for tx in &block.transactions {
        
        //println!("looking at {:?} for nota in tx output {:?}", height, tx.clone().outputs().last());

        let mut signedmask: u64 = 0;
        signedmask |= if *height < Height(91400) { 1 } else { 0 };
        for input in tx.inputs() {
            if let transparent::Input::Coinbase{..} = input { continue; } // skip coinbase input

            if let Some(outpoint) = input.outpoint() {
                if let Some(output) = spent_outputs.get(&outpoint) {
                    if let Some(n_id) = NN::komodo_get_notary_id_for_spent_output(network, height, &output) {
                        signedmask |= 1 << n_id;
                    }
                }
            }
        }

        let numbits = {
            let mut n: i32 = 0;
            while signedmask > 0 {
                n += (signedmask & 0x1) as i32;
                signedmask >>= 1;
            }
            n
        };
        //println!("komodo signed notary numbits={} height={:?}", numbits, height);
        trace!("komodo signed notary numbits={} height={:?}", numbits, height);

        if numbits >= komodo_minratify(network, height) {
            // several notas are possible in the same nota tx
            //println!("looking nota in outputs..");
            for output in tx.outputs() {
                if let Ok(nota) = BackNotarisationData::zcash_deserialize(output.lock_script.as_raw_bytes()) {  // ignore parse errors
                    //println!("nota found {:?}", nota);
                    return Some(nota);
                }
            }
        }
    }
    None
}
