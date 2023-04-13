//! tests for komodo notarisation code

use std::ops::Range;
use std::sync::Arc;
use hex::FromHex;

use zebra_chain::block::tests::komodo_generate::komodo_create_partial_chain;
use zebra_chain::serialization::ZcashDeserializeInto;
use zebra_chain::block::{self, Block, Height};

use zebra_chain::transparent::OutPoint;
use zebra_chain::amount::Amount;

use crate::request::ContextuallyValidBlock;
use crate::service::non_finalized_state::Chain;
use crate::{Config, CommitBlockError, PreparedBlock};
use crate::service::{StateService, block_iter};
use crate::{ValidateContextError, FinalizedBlock};
use crate::arbitrary::Prepare;

use std::collections::HashMap;

use zebra_test::prelude::*;

use zebra_chain::{
    parameters::{Network, *},
    value_balance::ValueBalance,
};

const CHAIN_A_BLOCK_HASH_WITH_NOTA: &str = "00e5a0b985d58cd3be4c6b580f30de57d041a56589d61e98b85a0fe20f76383f";
const CHAIN_A_BLOCK_HASH_TO_FAIL: &str = "009b7faa4fffb879db787dec6acafe02767297158a867361ad82a699c6c8839c";
const NON_NOTARY_P2PK: &str = "2102c50c23b6578f6a688f9868efca41bddd33b4225583474bb6183ff3ddf593ae01ac";

struct SampleChain<'a> {
    pub genesis: &'a str, 
    pub node_1: &'a str, 
    pub node_2: &'a str, 
    pub fork: usize,    // not including genesis
    pub tip_1: usize,
    pub tip_2: usize,
}

const SAMPLE_CHAIN_A: SampleChain<'static> = SampleChain { 
    genesis: include_str!("./testnet_a_genesis.hex"), 
    node_1: include_str!("./testnet_a_node_1.hex"),
    node_2: include_str!("./testnet_a_node_2.hex"), // branch with the nota
    fork:  127, 
    tip_1: 136,
    tip_2: 140,
};

const SAMPLE_CHAIN_B: SampleChain<'static> = SampleChain { 
    genesis: include_str!("./testnet_b_genesis.hex"), 
    node_1: include_str!("./testnet_b_node_2.hex"), // no nota in this branch
    node_2: include_str!("./testnet_b_node_1.hex"), // branch with the extra nota
    fork:  128,
    tip_1: 139,
    tip_2: 140,
};

/// Contextual validation tests helper to load test blocks and modify them if needed
/// it also calls result checker to assert or continue block loading (for non finalised part from ht=127)
/// For testing blocks transactions can be modified before committing as contextual validation does net check merkle root
fn komodo_load_testnet_both_branches<M, C>(chain_desc: SampleChain, modify_block: M, check_commit_result: C)
    where 
        M: Fn(&mut Block),
        C: Fn(&PreparedBlock, &Result<(), CommitBlockError>)->bool,  // continue if true
{
   
    let (mut state, _, _, _) = StateService::new(Config::ephemeral(), Network::Testnet);

    let genesis_bin = Vec::from_hex(chain_desc.genesis.trim()).expect("invalid genesis hex");
    let genesis = genesis_bin.zcash_deserialize_into::<Arc<Block>>()
        .expect("block should deserialize");
    let genesis_fin = FinalizedBlock::from(genesis);
    state
        .disk
        .commit_finalized_direct(genesis_fin.clone(), "test")
        .expect("unexpected invalid genesis block test vector");

    let blocks_node1_hex = chain_desc.node_1.split("\n").collect::<Vec<_>>();

    
    let finalized = chain_desc.fork - 1; // add some blocks below the fork to finalized 
    let remained_1 = chain_desc.tip_1 - finalized; // how many to add after finalized (minus genesis) 
    let remained_2 = chain_desc.tip_2 - chain_desc.fork; // how many to add after fork (minus genesis)
    for block_hex in blocks_node1_hex.iter().take(finalized)   {   // one off is genesis
        if block_hex.trim().is_empty() { break; }
        let block_bin = Vec::from_hex(block_hex.trim()).expect("invalid block hex");
        let block = block_bin.zcash_deserialize_into::<Block>().expect("could not deserialise block");

        let block_fin = FinalizedBlock::from(Arc::new(block));
        let commit_result = state.disk.commit_finalized_direct(block_fin.clone(), "test");
        assert!(commit_result.is_ok());
    }

    // fork is othe last common block for both branches
    // load remaining part of the second branch with the nota from the block after finalized to 136 to memory
    for block_hex in blocks_node1_hex.iter().skip(finalized).take(remained_1)   {     // load 136 blocks, less than blocks num in the second branch
        if block_hex.trim().is_empty() { break; }
        let block_bin = Vec::from_hex(block_hex.trim()).expect("invalid block hex");
        let mut block = block_bin.zcash_deserialize_into::<Block>().expect("could not deserialise block");

        modify_block(&mut block);   // allow to change block for different test cases
        let block = Arc::new(block);
        let block_prepared = block.prepare();

        let commit_result = state.validate_and_commit(block_prepared.clone());
        if !check_commit_result(&block_prepared, &commit_result) { break; }  // check results
    }

    let blocks_node2_hex = chain_desc.node_2.split("\n").collect::<Vec<_>>();

    // load second branch from the block after fork to tip_2
    for block_hex in blocks_node2_hex.iter().skip(chain_desc.fork).take(remained_2)   {     // start a branch from block 128
        if block_hex.trim().is_empty() { break; }
        let block_bin = Vec::from_hex(block_hex.trim()).expect("invalid block hex");
        let mut block = block_bin.zcash_deserialize_into::<Block>().expect("could not deserialise block");

        modify_block(&mut block);
        let block = Arc::new(block);
        let block_prepared = block.prepare();

        let commit_result = state.validate_and_commit(block_prepared.clone());
        if !check_commit_result(&block_prepared, &commit_result) { break; }  // check results
    }
}

/// a helper to replace pubkeys in fake nota's inputs to a non-notary pubkey
fn replace_nota_input_pubkey(block: &mut Block, r: Range<usize>)   {
    let replace_script = <Vec<u8>>::from_hex(NON_NOTARY_P2PK).expect("valid hex");

    // clone inputs:
    let nota_inputs = block.transactions[ 2 ].inputs().into_iter().map(|i| i.clone()).collect::<Vec<_>>();

    let prev_tx = Arc::get_mut(&mut block.transactions[ 1 ]).unwrap();  // notaries' funding outputs
    let prev_tx_hash = prev_tx.hash();
    let prev_tx_outputs = prev_tx.outputs_mut();
    for i in r {
        assert!(nota_inputs[i].outpoint().unwrap().hash == prev_tx_hash, "bad nota prev tx");
        prev_tx_outputs[nota_inputs[i].outpoint().unwrap().index as usize].lock_script = zebra_chain::transparent::Script::new(&replace_script);
    }

    let prev_tx = Arc::get_mut(&mut block.transactions[ 1 ]).unwrap();  // notaries' funding outputs
    let new_prev_tx_hash = prev_tx.hash();
    let nota_inputs = Arc::get_mut(&mut block.transactions[ 2 ]).unwrap().inputs_mut();

    // Note for rust learners: do not use 'prev_tx' after 'nota_inputs' as it would be the second mutable borrow for 'block':
    // let nota_inputs = Arc::get_mut(&mut block.transactions[ 2 ]).unwrap().inputs_mut();
    // let new_prev_tx_hash = prev_tx.hash();

    for i in 0..nota_inputs.len() {
        // for rust learners:
        // we need a extra step to do immutable borrow inside this statement:
        let outpoint = nota_inputs[i].outpoint().unwrap();  
        nota_inputs[i].set_outpoint(OutPoint{hash: new_prev_tx_hash, index: outpoint.index});   // mutable borrow inside this statement 
        // So we cannot do this looking more compact: 
        // inputs[i].set_outpoint(OutPoint{hash: new_prev_tx_hash, index: inputs[i].outpoint().unwrap().index}); 
        // as it would be both mutable and immutable borrows
    }
}

/// komodo test contextual validation to reject blocks forked below the last notarised height stored in a notarisation transaction
/// use komodo testnet blocks
#[test]
fn komodo_reject_fork_from_below_last_notarised_height() {
    let _init_guard = zebra_test::init();

    // test invalid fork from ht < ntz_ht 
    komodo_load_testnet_both_branches(
        SAMPLE_CHAIN_A,
        |_| {}, 
        |prepared, commit_result| {
            // forked chain must become invalid at height 137 (where it becomes the best chain)
            let bad_block_hash = block::Hash::from_hex(CHAIN_A_BLOCK_HASH_TO_FAIL).expect("valid hex");
            if prepared.hash == bad_block_hash {
                assert_eq!( 
                    *commit_result, 
                    Err(
                        ValidateContextError::KomodoInvalidNotarisedChain(
                            bad_block_hash, Height(127), Height(128)
                        )
                        .into()
                    ) 
                );
                return false; // end of test
            } 
            return true;
        }
    );
}

/// komodo test nota validation
#[test]
fn komodo_valid_fork_from_below_last_notarised_height() {
    let _init_guard = zebra_test::init();

    // test invalid fork from ht < ntz_ht 
    komodo_load_testnet_both_branches(
        SAMPLE_CHAIN_B,
        |_| {}, 
        |_prepared, commit_result| {
            if commit_result.is_err()   {
                println!("commit_result1={}", commit_result.as_ref().unwrap_err());
                //println!("commit_result2={:?}", commit_result.as_ref().unwrap_err());
            }
            assert_eq!( 
                *commit_result, 
                Ok(()) 
            );
            return true;
        }
    );
}

/// test low notary signed inputs number for the notarised transaction  
#[test]
fn komodo_too_few_notary_inputs() {
    let _init_guard = zebra_test::init();
    komodo_load_testnet_both_branches(
        SAMPLE_CHAIN_A,
        |block|{ 
            if block.hash() == block::Hash::from_hex(CHAIN_A_BLOCK_HASH_WITH_NOTA).expect("valid hex") {
                let prev_outputs = block.transactions[1].outputs().to_owned();  // funding tx
                let inputs = Arc::get_mut(&mut block.transactions[2]).unwrap().inputs_mut();    // notarisation tx
                let mut removed_amount = Amount::zero();
                // remove inputs to make them fewer than min ratify (actually this makes no nota in the chain):
                while inputs.len() > 6 { 
                    let popped = inputs.pop().unwrap(); 
                    removed_amount = (removed_amount + prev_outputs[popped.outpoint().unwrap().index as usize].value).unwrap(); 
                } 
                let outputs = Arc::get_mut(&mut block.transactions[2]).unwrap().outputs_mut();
                outputs[0].value = (outputs[0].value() - removed_amount).unwrap(); // decrease output value for the removed inputs total
            }
        }, 
        |_prepared, commit_result| {
            // all forked chain becomes valid if no valid nota
            assert_eq!( 
                *commit_result, 
                Ok(()) 
            );
            return true;
        }
    );
}

/// test if signed notaries number reduced for the min ratify number 
#[test]
fn komodo_min_notary_inputs() {
    let _init_guard = zebra_test::init();
    komodo_load_testnet_both_branches(
        SAMPLE_CHAIN_A,
        |block|{ 
            if block.hash() == block::Hash::from_hex(CHAIN_A_BLOCK_HASH_WITH_NOTA).expect("valid hex") {
                let prev_outputs = block.transactions[1].outputs().to_owned();  // funding tx
                let inputs = Arc::get_mut(&mut block.transactions[2]).unwrap().inputs_mut();    // notarisation tx
                let mut removed_amount = Amount::zero();
                // remove inputs to make their number equal to min ratify:
                while inputs.len() > 7 { 
                    let popped = inputs.pop().unwrap(); 
                    removed_amount = (removed_amount + prev_outputs[popped.outpoint().unwrap().index as usize].value).unwrap(); 
                } 
                let outputs = Arc::get_mut(&mut block.transactions[2]).unwrap().outputs_mut();
                outputs[0].value = (outputs[0].value() - removed_amount).unwrap(); // decrease output value for the removed inputs total
            }
        }, 
        |prepared, commit_result| {
            // still have nota in the chain
            // forked chain must become invalid at height 137
            let bad_block_hash = block::Hash::from_hex(CHAIN_A_BLOCK_HASH_TO_FAIL).expect("valid hex");
            if prepared.hash == bad_block_hash {
                assert_eq!( 
                    *commit_result, 
                    Err(
                        ValidateContextError::KomodoInvalidNotarisedChain(
                            bad_block_hash, Height(127), Height(128)
                        )
                        .into()
                    ) 
                );
                return false; // end of test
            } else {
                assert_eq!( 
                    *commit_result, 
                    Ok(()) 
                );
            }
            return true;
        }
    );
}

/// test non-notary signers for the notarised transaction  
#[test]
fn komodo_not_notary_inputs() {
    let _init_guard = zebra_test::init();
    //let lock_script = <Vec<u8>>::from_hex(NON_NOTARY_P2PK).expect("valid hex");
    komodo_load_testnet_both_branches(
        SAMPLE_CHAIN_A,
        |block|{ 
            if block.hash() == block::Hash::from_hex(CHAIN_A_BLOCK_HASH_WITH_NOTA).expect("valid hex") {
                // replace pubkey in 6+ inputs to a non-notary pubkey
                replace_nota_input_pubkey(block, 6..block.transactions[2].inputs().len());
            }
        }, 
        |_prepared, commit_result| {
            // all forked blocks become valid if nota does not contain enough notary inputs
            assert_eq!( 
                *commit_result, 
                Ok(()) 
            );
            return true;
        }
    );
}

// Test chain description mini-language
/// defines operations: advance chain for n ordinary blocks, create a fork, create a block with a nota
#[allow(unused)]
enum TCD {
    /// advance to n ordinary blocks
    A(u32),

    /// make a fork from the height n blocks down from the tip. Note that operation 'fork' does not add new blocks
    F(i32, &'static str),

    /// make a block with a nota pointing to ht-2 as the last notarised height
    N,
}

/// komodo test forks in a chain with notas
/// cannot create a new fork below the last notarised height
#[test]
fn komodo_forked_notarised_chains_1() {
    zebra_test::init();

    // chain description: (<branch_num>, TCD-op)
    let chain_desc = [ 
        ("0", TCD::A(5)), // advance 5 blocks (including 0 genesis) to ht 4
        ("0", TCD::N),  // create a block with a nota (pointing to ht-2 as the last notarised height)
        ("0", TCD::F(-3, "1")), // create fork at height 2, make branch with id "1"
        ("0", TCD::A(2)),   // advance branch "0" by 3 blocks
        ("1", TCD::A(3)),   // advance branch "1" by 3 blocks
    ];

    // create a chain for chain_desc and execute result checkers
    komodo_run_forked_nn_chain_test(&chain_desc, |_, _, branch_id, prepared_block, result| {        
        if branch_id == "1" && prepared_block.height == Height(3) {
            assert_eq!( 
                *result, 
                Err(
                    ValidateContextError::KomodoInvalidNotarisedChain(
                        prepared_block.hash, Height(2), Height(3)   // fork height and last notarised height 
                    )
                    .into()
                ) 
            );
            // println!("{:?} received for fork height < last notarised height", result.as_ref().err().unwrap());
            return false;
        } else {
            assert_eq!(*result, Ok(()));
        }
        return true;
    },
    |_,_|{});   // empty checker
}

/// komodo test forks in a chain with notas
/// valid fork
#[test]
fn komodo_forked_notarised_chains_2() {
        zebra_test::init();

    let chain_desc = [ 
        ("0", TCD::A(5)),  // to ht 4 (including 0 genesis)
        ("0", TCD::N), // create block with nota at ht 5 (last notarised ht = 3)
        ("0", TCD::F(-2, "1")), // fork at height 3
        ("0", TCD::A(2)),
        ("1", TCD::A(3)),
        ("0", TCD::N),

    ];
    komodo_run_forked_nn_chain_test(&chain_desc, |_, _, _branch_id, _prepared_block, result| {
        assert_eq!(*result, Ok(()));
        return true;
    },
    |_,_|{});
}

/// komodo test forks in a chain with notas
/// try to grow an already forked chain if the nota is added into another branch
#[test]
fn komodo_forked_notarised_chains_3() {
    zebra_test::init();

    let chain_desc = [ 
        ("0", TCD::A(5)),   // advance to ht 4 (0 is genesis)
        ("0", TCD::N),  // create block with nota at ht 5 (last notarised ht = 3)
        ("0", TCD::F(0, "1")), // fork at ht 5
        ("1", TCD::A(3)), // advance to ht 8
        ("0", TCD::A(3)), // advance to ht 8
        ("0", TCD::N),  // create block with nota at ht 9 (last notarised ht = 7)
        ("1", TCD::A(2)), // try add to ht 9
    ];
    komodo_run_forked_nn_chain_test(&chain_desc, 
        |state, tips, branch_id, prepared_block, result| {        
            // println!("branch_id {:?} prepared_block.height {:?}", branch_id, prepared_block.height);
            if branch_id == "1" && prepared_block.height >= Height(9) { // chain "1" may have more work since ht 9
                let mut non_fin_state = state.mem.clone();
                let chain_1 = non_fin_state.find_chain(|chain| chain.height_by_hash(tips.get("1").expect("branch tip exists").1).is_some() ).expect("chain not empty");
                let mut non_fin_state = state.mem.clone();
                let chain_0 = non_fin_state.find_chain(|chain| chain.height_by_hash(tips.get("0").expect("branch tip exists").1).is_some() ).expect("chain not empty");

                if &komodo_test_chain_add_block(chain_1.clone(), prepared_block) > chain_0 {  // check if chain 1 has more work than best_chain
                    assert_eq!( 
                        *result, 
                        Err(
                            ValidateContextError::KomodoInvalidNotarisedChain(
                                prepared_block.hash, Height(5), Height(7)
                            )
                            .into()
                        ) 
                    );
                    // println!("result {:?} received for fork height < last notarised height", result.as_ref().err().unwrap());
                    return false;
                }
            } else {
                assert_eq!(*result, Ok(()));
            }
            return true;
        },
    |_,_|{});
}

/// komodo test forks in a chain with notas
/// try to grow an already forked chain if the nota is added into another branch, use more branches
#[test]
fn komodo_forked_notarised_chains_4() {
    zebra_test::init();

    let chain_desc = [ 
        ("0", TCD::A(5)), // advance to ht 4 (inluding genesis 0)
        ("0", TCD::N), // nota at ht 5 (last notarised ht = 3)
        ("0", TCD::F(0, "1")), // fork at ht 5
        ("1", TCD::A(3)),
        ("1", TCD::F(0, "2")), // another fork at ht 8
        ("1", TCD::A(2)), // advance to ht 10
        ("2", TCD::A(3)), // advance to ht 11
        ("0", TCD::A(3)), // advance to ht 8
        ("0", TCD::N),    // add block with nota at ht 9 (last notarised ht = 7)
        ("1", TCD::A(1)), // try add at ht 11
    ];
    komodo_run_forked_nn_chain_test(&chain_desc, 
        |state, tips, branch_id, prepared_block, result| {        
            // println!("branch_id {:?} prepared_block.height {:?}", branch_id, prepared_block.height);
            if branch_id == "1" && prepared_block.height >= Height(11) { 
                let mut non_fin_state = state.mem.clone();
                let chain_1 = non_fin_state.find_chain(|chain| chain.height_by_hash(tips.get("1").expect("branch tip exists").1).is_some() ).expect("chain not empty");
                let mut non_fin_state = state.mem.clone();
                let chain_0 = non_fin_state.find_chain(|chain| chain.height_by_hash(tips.get("0").expect("branch tip exists").1).is_some() ).expect("chain not empty");
                if &komodo_test_chain_add_block(chain_1.clone(), prepared_block) > chain_0 {  // check if chain 1 has more work than best_chain
                    assert_eq!( 
                        *result, 
                        Err(
                            ValidateContextError::KomodoInvalidNotarisedChain(
                                prepared_block.hash, Height(5), Height(7)
                            )
                            .into()
                        ) 
                    );
                    // println!("result {:?} received for fork height < last notarised height", result.as_ref().err().unwrap());
                    return false;
                }
            } else {
                assert_eq!(*result, Ok(()));
            }
            return true;
        },
    |_,_|{});
}

/// komodo test notarised best chain
/// should return best chain with nota
#[test]
fn komodo_best_notarised_chain_1() {
    zebra_test::init();

    let chain_desc_1 = [ 
        ("0", TCD::A(5)),
        ("0", TCD::N),
        ("0", TCD::F(0, "1")), // fork at ht 5, creates branch with id "1"
        ("0", TCD::F(0, "2")), // fork at ht 5, creates branch with id "2"
        ("2", TCD::A(3)),
        ("1", TCD::A(3)),
        ("0", TCD::A(3)),
        ("1", TCD::A(3)),   // make "1" longer than "0" 
        ("2", TCD::A(4)),   // make "2" longer than "1" 
        ("0", TCD::N),      // added nota in "0"
        ("0", TCD::A(1)),   // add a block after nota in "0"
    ];
    komodo_run_forked_nn_chain_test(&chain_desc_1, |_, _, _branch_id, _prepared_block, result| {        
        // println!("branch {} hash {:?} height {:?}", _branch_id, _prepared_block.hash, _prepared_block.height);
        assert_eq!(*result, Ok(()));
        return true;
    }, 
    |state, tips|{
        // branch "0" with nota must be the best
        assert_eq!(state.mem.best_tip().expect("valid best tip").1, tips.get("0").expect("valid tip for branch").1);
    });
}

/// komodo test notarised best chain
/// with no nota should return longest best chain 
#[test]
fn komodo_best_notarised_chain_2() {
    zebra_test::init();

    let chain_desc_2 = [ 
        ("0", TCD::A(5)),
        ("0", TCD::N),
        ("0", TCD::F(0, "1")), // fork at ht 5, creates branch with id "1"
        ("0", TCD::F(0, "2")), // fork at ht 5, creates branch with id "2"
        ("0", TCD::A(3)),
        ("1", TCD::A(3)),
        ("2", TCD::A(3)),
        ("0", TCD::A(1)),   // advance ht+1
        ("1", TCD::A(2)),   // make "1" longer than "0"
        ("2", TCD::A(3)),   // make "2" longer than "1"
    ];
    komodo_run_forked_nn_chain_test(&chain_desc_2, |_, _, _branch_id, _prepared_block, result| {        
        // println!("branch {} hash {:?} height {:?}", _branch_id, _prepared_block.hash, _prepared_block.height);
        assert_eq!(*result, Ok(()));
        return true;
    }, 
    |state, tips|{
        // longest branch "2" must be the best
        assert_eq!(state.mem.best_tip().expect("valid best tip").1, tips.get("2").expect("valid tip for branch").1);
    });
}

/// this test helper allows to create a configurable test chain (blocks, forks and notas) and run checker functions.
/// Params: 
/// chain_desc chain description, what blocks, notas and forks to create
/// check_result - block commit result checker
/// check_state - checker is executed when the chain is created to validate its whole consistence
fn komodo_run_forked_nn_chain_test<C1, C2>(chain_desc: &[(&str, TCD)], check_result: C1, check_state: C2)
    where
        C1: Fn(&mut StateService, &HashMap<&str, (Height, block::Hash)>, &str, &PreparedBlock, &Result<(), CommitBlockError>)->bool,  // return true to continue test
        C2: Fn(&StateService, &HashMap<&str, (Height, block::Hash)>),  
{
    // create with inital branch 0
    let mut branch_tips = HashMap::new();
    branch_tips.insert("0", (Height(0), GENESIS_PREVIOUS_BLOCK_HASH));  // (next height, prev block hash)

    let (mut state_service, _read_state, _latest_chain_tip, _chain_tip_change) =
        StateService::new(Config::ephemeral(), Network::Testnet);

    for tcd_step in chain_desc.iter() {

        let new_chunk;
        let branch_next_tip = branch_tips.get(tcd_step.0).expect("branch id must exist");

        let relevant_chain = block_iter::any_ancestor_blocks(&state_service.mem, &state_service.disk.db(), branch_next_tip.1);
        let mut prev_blocks = relevant_chain.collect::<Vec<_>>();
        prev_blocks.reverse();

        let this_chain = state_service.mem.find_chain(|chain| chain.non_finalized_tip_hash() == branch_next_tip.1);
        let (value_pools, utxos) = if let Some(this_chain) = this_chain {
            (this_chain.chain_value_pools, this_chain.unspent_utxos())
        } else {
            (ValueBalance::zero(), HashMap::new())
        };
        match tcd_step.1 {
            TCD::A(n) => {
                new_chunk = komodo_create_partial_chain(
                    tcd_step.0,
                    &prev_blocks,
                    value_pools,
                    utxos,
                    branch_next_tip.0, 
                    n, 
                    false, 
                    crate::service::check::utxo::transparent_coinbase_spend, // allow_all_transparent_coinbase_spends,
                );
            },
            TCD::N => {
                new_chunk = komodo_create_partial_chain(
                    tcd_step.0,
                    &prev_blocks,
                    value_pools,
                    utxos,
                    branch_next_tip.0, 
                    1, 
                    true, 
                    crate::service::check::utxo::transparent_coinbase_spend, // allow_all_transparent_coinbase_spends,
                );            
            },
            TCD::F(n, new_branch_id) => {
                assert!(n <= 0);
                assert!(branch_tips.get(new_branch_id).is_none(), "new branch already exists");
                    
                let fork_ht = (branch_next_tip.0 + (-1 + n)).expect("fork offset valid"); // substract 1 more as this is the next height, not tip height
                let this_chain = this_chain
                    .expect("non-finalized chain must exist for branch tip");
                let fork_hash = this_chain.hash_by_height(fork_ht).expect("fork tip found");
                this_chain
                    .fork(
                        fork_hash,
                        Default::default(),
                        Default::default(),
                        Default::default(),
                        Default::default(),
                    )
                    .expect("fork works")
                    .expect("hash is present");
                branch_tips.insert(new_branch_id, ((fork_ht+1).unwrap(),fork_hash)); // add new branch
                // println!("fork created at ht={:?} block_hash {:?}", fork_ht, fork_hash);
                continue;
            },
        }

        let mut to_skip = 0_usize;
        if new_chunk[0].coinbase_height().expect("valid coinbase height") == Height(0) {
            // println!("to finalize ht={:?}", new_chunk[0].coinbase_height());
            let result = state_service.disk.commit_finalized_direct(new_chunk[0].clone().into(), "test");
            assert!(
                result.is_ok(),
                "komodo_create_partial_chain should generate a valid genesis block"
            );
            to_skip = 1;
        }

        for block in new_chunk.iter().skip(to_skip) {

            let prepared = block.clone().prepare();
            // println!("to commit branch '{}' ht={:?} hash={:?}", tcd_step.0, prepared.clone().height, prepared.clone().hash);
            let result = state_service.validate_and_commit(prepared.clone());
            // println!("result {:?} last_nota {:?}", result, state_service.mem.last_nota);
            if !check_result(&mut state_service, &branch_tips, tcd_step.0, &prepared, &result) {
                return;
            }
            branch_tips.insert(tcd_step.0, ((prepared.clone().height + 1).unwrap(), prepared.clone().hash));
        }
    } 
    check_state(&state_service, &branch_tips);
}

fn komodo_test_chain_add_block(chain: Arc<Chain>, prepared_block: &PreparedBlock) -> Arc<Chain> {
    if chain.blocks.values().last().expect("non empty chain").hash == prepared_block.hash {
        chain.clone()
    } else {
        let contextual_block = ContextuallyValidBlock::with_block_and_spent_utxos(
            chain.network(),
            prepared_block.clone(),
            None,
            chain.unspent_utxos()        
        ).expect("ContextuallyValidBlock created");

        let new_chain = (*chain).clone();
        let new_chain = new_chain.push(contextual_block).expect("add test block okay");

        Arc::new(new_chain)
    }
}