//! Test vectors and randomised property tests for UTXO contextual validation

use std::{env, sync::Arc /*, intrinsics::unreachable*/};

use chrono::{DateTime, Utc};
use proptest::prelude::*;

use zebra_chain::{
    amount::{Amount, NonNegative},
    block::{Block, Height},
    fmt::TypeNameToDebug,
    serialization::ZcashDeserializeInto,
    transaction::{self, LockTime, Transaction},
    transparent, parameters::Network,
};

use crate::{
    arbitrary::Prepare,
    constants::MIN_TRANSPARENT_COINBASE_MATURITY,
    service::{
        check, finalized_state::FinalizedState, non_finalized_state::NonFinalizedState, read,
        write::validate_and_commit_non_finalized,
    },
    tests::setup::{komodo_new_state_with_mainnet_genesis, komodo_new_state_with_testnet_genesis, transaction_v4_from_coinbase},
    FinalizedBlock,
    ValidateContextError::{
        DuplicateTransparentSpend, EarlyTransparentSpend, ImmatureTransparentCoinbaseSpend,
        MissingTransparentOutput, UnshieldedTransparentCoinbaseSpend,
    },
};

/// Check that shielded, mature spends of coinbase transparent outputs succeed.
///
/// This test makes sure there are no spurious rejections that might hide bugs in the other tests.
/// (And that the test infrastructure generally works.)
#[test]
fn accept_shielded_mature_coinbase_utxo_spend() {
    let _init_guard = zebra_test::init();

    let created_height = Height(1);
    let outpoint = transparent::OutPoint {
        hash: transaction::Hash([0u8; 32]),
        index: 0,
    };
    let output = transparent::Output {
        value: Amount::zero(),
        lock_script: transparent::Script::new(&[]),
    };
    let ordered_utxo = transparent::OrderedUtxo::new(output, created_height, 0, LockTime::unlocked());

    let min_spend_height = Height(created_height.0 + MIN_TRANSPARENT_COINBASE_MATURITY);
    let spend_restriction = transparent::CoinbaseSpendRestriction::OnlyShieldedOutputs {
        spend_height: min_spend_height,
    };

    let result =
        check::utxo::transparent_coinbase_spend(Network::Mainnet, outpoint, spend_restriction, ordered_utxo.clone());
    assert_eq!(result, Ok(ordered_utxo));
}

/// Check that non-shielded spends of coinbase transparent outputs fail.
#[ignore = "this is okay in komodo"] // this is legal in Komodo
#[test]
fn reject_unshielded_coinbase_utxo_spend() {
    let _init_guard = zebra_test::init();

    let created_height = Height(1);
    let outpoint = transparent::OutPoint {
        hash: transaction::Hash([0u8; 32]),
        index: 0,
    };
    let output = transparent::Output {
        value: Amount::zero(),
        lock_script: transparent::Script::new(&[]),
    };
    let ordered_utxo = transparent::OrderedUtxo::new(output, created_height, 0, LockTime::unlocked());

    let min_spend_height = Height(created_height.0 + MIN_TRANSPARENT_COINBASE_MATURITY); // added by komodo
    let spend_restriction = transparent::CoinbaseSpendRestriction::SomeTransparentOutputs{ spend_height: min_spend_height };

    let result = check::utxo::transparent_coinbase_spend(Network::Mainnet, outpoint, spend_restriction, ordered_utxo);
    assert_eq!(result, Err(UnshieldedTransparentCoinbaseSpend { outpoint }));
}

/// Check that early spends of coinbase transparent outputs fail.
#[test]
#[ignore = "enable when komodo zebra updated"] // Enable after PR#26 merged in Komodo Zebra repo
fn reject_immature_coinbase_utxo_spend() {
    let _init_guard = zebra_test::init();

    let created_height = Height(1);
    let outpoint = transparent::OutPoint {
        hash: transaction::Hash([0u8; 32]),
        index: 0,
    };
    let output = transparent::Output {
        value: Amount::zero(),
        lock_script: transparent::Script::new(&[]),
    };
    let ordered_utxo = transparent::OrderedUtxo::new(output, created_height, 0, LockTime::unlocked());

    let min_spend_height = Height(created_height.0 + MIN_TRANSPARENT_COINBASE_MATURITY);
    let spend_height = Height(min_spend_height.0 - 1);
    let spend_restriction =
        transparent::CoinbaseSpendRestriction::OnlyShieldedOutputs { spend_height };

    let result = check::utxo::transparent_coinbase_spend(Network::Mainnet, outpoint, spend_restriction, ordered_utxo);
    assert_eq!(
        result,
        Err(ImmatureTransparentCoinbaseSpend {
            outpoint,
            spend_height,
            min_spend_height,
            created_height
        })
    );
}

// These tests use the `Arbitrary` trait to easily generate complex types,
// then modify those types to cause an error (or to ensure success).
//
// We could use mainnet or testnet blocks in these tests,
// but the differences shouldn't matter,
// because we're only interested in spend validation,
// (and passing various other state checks).

const DEFAULT_UTXO_PROPTEST_CASES: u32 = 16;

proptest! {
    #![proptest_config(
        proptest::test_runner::Config::with_cases(env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_UTXO_PROPTEST_CASES))
    )]

    /// Make sure an arbitrary transparent spend from a previous transaction in this block
    /// is accepted by state contextual validation.
    ///
    /// This test makes sure there are no spurious rejections that might hide bugs in the other tests.
    /// (And that the test infrastructure generally works.)
    ///
    /// It also covers a potential edge case where later transactions can spend outputs
    /// of previous transactions in a block, but earlier transactions can not spend later outputs.
    #[test]
    fn accept_later_transparent_spend_from_this_block(
        utxo in TypeNameToDebug::<transparent::Utxo>::arbitrary(),
        mut prevout_input in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        use_finalized_state in any::<bool>(),
    ) {
        let _init_guard = zebra_test::init();

        let mut block1 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_1_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        // create an output
        let output_transaction = komodo_transaction_v4_with_transparent_data([], [], [utxo.0.output.clone()]);

        // create a spend
        let expected_outpoint = transparent::OutPoint {
            hash: output_transaction.hash(),
            index: 0,
        };
        prevout_input.set_outpoint(expected_outpoint);
        let spend_transaction = komodo_transaction_v4_with_transparent_data(
            [prevout_input.0],
            [(expected_outpoint, utxo.0)],
            []
        );

        // convert the coinbase transaction to a version that the non-finalized state will accept
        block1.transactions[0] = transaction_v4_from_coinbase(&block1.transactions[0]).into();

        block1
            .transactions
            .extend([output_transaction.into(), spend_transaction.into()]);

        let (mut finalized_state, mut non_finalized_state, _genesis) = komodo_new_state_with_mainnet_genesis();
        let previous_non_finalized_state = non_finalized_state.clone();

        // randomly choose to commit the block to the finalized or non-finalized state
        if use_finalized_state {
            let block1 = FinalizedBlock::from(Arc::new(block1));
            let commit_result = finalized_state.commit_finalized_direct(block1.clone().into(), "test");

            // the block was committed
            prop_assert_eq!(Some((Height(1), block1.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));
            prop_assert!(commit_result.is_ok());

            // the non-finalized state didn't change
            prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

            // the finalized state added then spent the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
            // the non-finalized state does not have the UTXO
            prop_assert!(non_finalized_state.any_utxo(&expected_outpoint).is_none());
        } else {
            let block1 = Arc::new(block1).prepare();
            let commit_result = validate_and_commit_non_finalized(
                &finalized_state.db,
                &mut non_finalized_state,
                block1.clone()
            );

            // the block was committed
            prop_assert_eq!(commit_result, Ok(()));
            prop_assert_eq!(Some((Height(1), block1.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));

            // the block data is in the non-finalized state
            prop_assert!(!non_finalized_state.eq_internal_state(&previous_non_finalized_state));

            // the non-finalized state has created and spent the UTXO
            prop_assert_eq!(non_finalized_state.chain_set.len(), 1);
            let chain = non_finalized_state
                .chain_set
                .iter()
                .next()
                .unwrap();
            prop_assert!(!chain.unspent_utxos().contains_key(&expected_outpoint));
            prop_assert!(chain.created_utxos.contains_key(&expected_outpoint));
            prop_assert!(chain.spent_utxos.contains(&expected_outpoint));

            // the finalized state does not have the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
        }
    }

    /// Make sure an arbitrary transparent spend from a previous block
    /// is accepted by state contextual validation.
    #[test]
    fn accept_arbitrary_transparent_spend_from_previous_block(
        utxo in TypeNameToDebug::<transparent::Utxo>::arbitrary(),
        mut prevout_input in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        use_finalized_state_output in any::<bool>(),
        mut use_finalized_state_spend in any::<bool>(),
    ) {
        let _init_guard = zebra_test::init();

        // if we use the non-finalized state for the first block,
        // we have to use it for the second as well
        if !use_finalized_state_output {
            use_finalized_state_spend = false;
        }

        let mut block2 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_2_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        let TestState {
            mut finalized_state, mut non_finalized_state, block1, ..
        } = komodo_new_state_with_mainnet_transparent_data([], [], [utxo.0.output.clone()], use_finalized_state_output);
        let previous_non_finalized_state = non_finalized_state.clone();

        let expected_outpoint = transparent::OutPoint {
            hash: block1.transactions[1].hash(),
            index: 0,
        };
        prevout_input.set_outpoint(expected_outpoint);

        let spend_transaction = komodo_transaction_v4_with_transparent_data(
            [prevout_input.0],
            [(expected_outpoint, utxo.0)],
            []
        );

        // convert the coinbase transaction to a version that the non-finalized state will accept
        block2.transactions[0] = transaction_v4_from_coinbase(&block2.transactions[0]).into();

        block2.transactions.push(spend_transaction.into());

        if use_finalized_state_spend {
            let block2 = FinalizedBlock::from(Arc::new(block2));
            let commit_result = finalized_state.commit_finalized_direct(block2.clone().into(), "test");

            // the block was committed
            prop_assert_eq!(Some((Height(2), block2.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));
            prop_assert!(commit_result.is_ok());

            // the non-finalized state didn't change
            prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

            // the finalized state has spent the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
        } else {
            let block2 = Arc::new(block2).prepare();
            let commit_result = validate_and_commit_non_finalized(
                &finalized_state.db,
                &mut non_finalized_state,
                block2.clone()
            );

            // the block was committed
            prop_assert_eq!(commit_result, Ok(()));
            prop_assert_eq!(Some((Height(2), block2.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));

            // the block data is in the non-finalized state
            prop_assert!(!non_finalized_state.eq_internal_state(&previous_non_finalized_state));

            // the UTXO is spent
            prop_assert_eq!(non_finalized_state.chain_set.len(), 1);
            let chain = non_finalized_state
                .chain_set
                .iter()
                .next()
                .unwrap();
            prop_assert!(!chain.unspent_utxos().contains_key(&expected_outpoint));

            if use_finalized_state_output {
                // the chain has spent the UTXO from the finalized state
                prop_assert!(!chain.created_utxos.contains_key(&expected_outpoint));
                prop_assert!(chain.spent_utxos.contains(&expected_outpoint));
                // the finalized state has the UTXO, but it will get deleted on commit
                prop_assert!(finalized_state.utxo(&expected_outpoint).is_some());
            } else {
                // the chain has spent its own UTXO
                prop_assert!(!chain.unspent_utxos().contains_key(&expected_outpoint));
                prop_assert!(chain.created_utxos.contains_key(&expected_outpoint));
                prop_assert!(chain.spent_utxos.contains(&expected_outpoint));
                // the finalized state does not have the UTXO
                prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
            }
        }
    }

    /// Make sure a duplicate transparent spend, by two inputs in the same transaction,
    /// using an output from a previous transaction in this block,
    /// is rejected by state contextual validation.
    #[test]
    fn reject_duplicate_transparent_spend_in_same_transaction_from_same_block(
        utxo in TypeNameToDebug::<transparent::Utxo>::arbitrary(),
        mut prevout_input1 in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        mut prevout_input2 in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
    ) {
        let _init_guard = zebra_test::init();

        let mut block1 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_1_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        let output_transaction = komodo_transaction_v4_with_transparent_data([], [], [utxo.0.output.clone()]);

        let expected_outpoint = transparent::OutPoint {
            hash: output_transaction.hash(),
            index: 0,
        };
        prevout_input1.set_outpoint(expected_outpoint);
        prevout_input2.set_outpoint(expected_outpoint);

        let spend_transaction = komodo_transaction_v4_with_transparent_data(
            [prevout_input1.0, prevout_input2.0],
            [(expected_outpoint, utxo.0)],
            []
        );

        // convert the coinbase transaction to a version that the non-finalized state will accept
        block1.transactions[0] = transaction_v4_from_coinbase(&block1.transactions[0]).into();

        block1
            .transactions
            .extend([output_transaction.into(), spend_transaction.into()]);

            let (finalized_state, mut non_finalized_state, genesis) = komodo_new_state_with_mainnet_genesis();
            let previous_non_finalized_state = non_finalized_state.clone();

        let block1 = Arc::new(block1).prepare();
        let commit_result = validate_and_commit_non_finalized(
            &finalized_state.db,
            &mut non_finalized_state,
            block1
        );

        // the block was rejected
        prop_assert_eq!(
            commit_result,
            Err(DuplicateTransparentSpend {
                outpoint: expected_outpoint,
                location: "the same block",
            }
            .into())
        );
        prop_assert_eq!(Some((Height(0), genesis.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));

        // the non-finalized state did not change
        prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

        // the finalized state does not have the UTXO
        prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
    }

    /// Make sure a duplicate transparent spend, by two inputs in the same transaction,
    /// using an output from a previous block in this chain,
    /// is rejected by state contextual validation.
    #[test]
    fn reject_duplicate_transparent_spend_in_same_transaction_from_previous_block(
        utxo in TypeNameToDebug::<transparent::Utxo>::arbitrary(),
        mut prevout_input1 in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        mut prevout_input2 in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        use_finalized_state_output in any::<bool>(),
    ) {
        let _init_guard = zebra_test::init();

        let mut block2 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_2_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        let TestState {
            finalized_state, mut non_finalized_state, block1, ..
        } = komodo_new_state_with_mainnet_transparent_data([], [], [utxo.0.output.clone()], use_finalized_state_output);
        let previous_non_finalized_state = non_finalized_state.clone();

        let expected_outpoint = transparent::OutPoint {
            hash: block1.transactions[1].hash(),
            index: 0,
        };
        prevout_input1.set_outpoint(expected_outpoint);
        prevout_input2.set_outpoint(expected_outpoint);

        let spend_transaction = komodo_transaction_v4_with_transparent_data(
            [prevout_input1.0, prevout_input2.0],
            [(expected_outpoint, utxo.0)],
            []
        );

        // convert the coinbase transaction to a version that the non-finalized state will accept
        block2.transactions[0] = transaction_v4_from_coinbase(&block2.transactions[0]).into();

        block2.transactions.push(spend_transaction.into());

        let block2 = Arc::new(block2).prepare();
        let commit_result = validate_and_commit_non_finalized(
            &finalized_state.db,
            &mut non_finalized_state,
            block2
        );

        // the block was rejected
        prop_assert_eq!(
            commit_result,
            Err(DuplicateTransparentSpend {
                outpoint: expected_outpoint,
                location: "the same block",
            }
            .into())
        );
        prop_assert_eq!(Some((Height(1), block1.hash())), read::best_tip(&non_finalized_state, &finalized_state.db));

        // the non-finalized state did not change
        prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

        if use_finalized_state_output {
            // the finalized state has the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_some());
            // the non-finalized state has no chains (so it can't have the UTXO)
            prop_assert!(non_finalized_state.chain_set.iter().next().is_none());
        } else {
            let chain = non_finalized_state
                .chain_set
                .iter()
                .next()
                .unwrap();
            // the non-finalized state has the UTXO
            prop_assert!(chain.unspent_utxos().contains_key(&expected_outpoint));
            // the finalized state does not have the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
        }
    }

    /// Make sure a duplicate transparent spend,
    /// by two inputs in different transactions in the same block,
    /// using an output from a previous block in this chain,
    /// is rejected by state contextual validation.
    #[test]
    fn reject_duplicate_transparent_spend_in_same_block_from_previous_block(
        utxo in TypeNameToDebug::<transparent::Utxo>::arbitrary(),
        mut prevout_input1 in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        mut prevout_input2 in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        use_finalized_state_output in any::<bool>(),
    ) {
        let _init_guard = zebra_test::init();

        let mut block2 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_2_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        let TestState {
            finalized_state, mut non_finalized_state, block1, ..
        } = komodo_new_state_with_mainnet_transparent_data([], [], [utxo.0.output.clone()], use_finalized_state_output);
        let previous_non_finalized_state = non_finalized_state.clone();

        let expected_outpoint = transparent::OutPoint {
            hash: block1.transactions[1].hash(),
            index: 0,
        };
        prevout_input1.set_outpoint(expected_outpoint);
        prevout_input2.set_outpoint(expected_outpoint);

        let spend_transaction1 = komodo_transaction_v4_with_transparent_data(
            [prevout_input1.0],
            [(expected_outpoint, utxo.0.clone())],
            []
        );
        let spend_transaction2 = komodo_transaction_v4_with_transparent_data(
            [prevout_input2.0],
            [(expected_outpoint, utxo.0)],
            []
        );

        // convert the coinbase transaction to a version that the non-finalized state will accept
        block2.transactions[0] = transaction_v4_from_coinbase(&block2.transactions[0]).into();

        block2
            .transactions
            .extend([spend_transaction1.into(), spend_transaction2.into()]);

        let block2 = Arc::new(block2).prepare();
        let commit_result = validate_and_commit_non_finalized(
            &finalized_state.db,
            &mut non_finalized_state,
            block2
        );

        // the block was rejected
        prop_assert_eq!(
            commit_result,
            Err(DuplicateTransparentSpend {
                outpoint: expected_outpoint,
                location: "the same block",
            }
            .into())
        );
        prop_assert_eq!(Some((Height(1), block1.hash())), read::best_tip(&non_finalized_state, &finalized_state.db));

        // the non-finalized state did not change
        prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

        if use_finalized_state_output {
            // the finalized state has the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_some());
            // the non-finalized state has no chains (so it can't have the UTXO)
            prop_assert!(non_finalized_state.chain_set.iter().next().is_none());
        } else {
            let chain = non_finalized_state
                .chain_set
                .iter()
                .next()
                .unwrap();
            // the non-finalized state has the UTXO
            prop_assert!(chain.unspent_utxos().contains_key(&expected_outpoint));
            // the finalized state does not have the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
        }
    }

    /// Make sure a duplicate transparent spend,
    /// by two inputs in different blocks in the same chain,
    /// using an output from a previous block in this chain,
    /// is rejected by state contextual validation.
    #[test]
    fn reject_duplicate_transparent_spend_in_same_chain_from_previous_block(
        utxo in TypeNameToDebug::<transparent::Utxo>::arbitrary(),
        mut prevout_input1 in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        mut prevout_input2 in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
        use_finalized_state_output in any::<bool>(),
        mut use_finalized_state_spend in any::<bool>(),
    ) {
        let _init_guard = zebra_test::init();

        // if we use the non-finalized state for the first block,
        // we have to use it for the second as well
        if !use_finalized_state_output {
            use_finalized_state_spend = false;
        }

        let mut block2 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_2_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");
        let mut block3 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_3_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        let TestState {
            mut finalized_state, mut non_finalized_state, block1, ..
        } = komodo_new_state_with_mainnet_transparent_data([], [], [utxo.0.output.clone()], use_finalized_state_output);
        let mut previous_non_finalized_state = non_finalized_state.clone();

        let expected_outpoint = transparent::OutPoint {
            hash: block1.transactions[1].hash(),
            index: 0,
        };
        prevout_input1.set_outpoint(expected_outpoint);
        prevout_input2.set_outpoint(expected_outpoint);

        let spend_transaction1 = komodo_transaction_v4_with_transparent_data(
            [prevout_input1.0],
            [(expected_outpoint, utxo.0.clone())],
            []
        );
        let spend_transaction2 = komodo_transaction_v4_with_transparent_data(
            [prevout_input2.0],
            [(expected_outpoint, utxo.0)],
            []
        );

        // convert the coinbase transactions to a version that the non-finalized state will accept
        block2.transactions[0] = transaction_v4_from_coinbase(&block2.transactions[0]).into();
        block3.transactions[0] = transaction_v4_from_coinbase(&block3.transactions[0]).into();

        block2.transactions.push(spend_transaction1.into());
        block3.transactions.push(spend_transaction2.into());

        let block2 = Arc::new(block2);

        if use_finalized_state_spend {
            let block2 = FinalizedBlock::from(block2.clone());
            let commit_result = finalized_state.commit_finalized_direct(block2.clone().into(), "test");

            // the block was committed
            prop_assert_eq!(Some((Height(2), block2.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));
            prop_assert!(commit_result.is_ok());

            // the non-finalized state didn't change
            prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

            // the finalized state has spent the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
            // the non-finalized state does not have the UTXO
            prop_assert!(non_finalized_state.any_utxo(&expected_outpoint).is_none());
        } else {
            let block2 = block2.clone().prepare();
            let commit_result = validate_and_commit_non_finalized(
                &finalized_state.db,
                &mut non_finalized_state,
                block2.clone()
            );

            // the block was committed
            prop_assert_eq!(commit_result, Ok(()));
            prop_assert_eq!(Some((Height(2), block2.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));

            // the block data is in the non-finalized state
            prop_assert!(!non_finalized_state.eq_internal_state(&previous_non_finalized_state));

            prop_assert_eq!(non_finalized_state.chain_set.len(), 1);
            let chain = non_finalized_state
                .chain_set
                .iter()
                .next()
                .unwrap();

            if use_finalized_state_output {
                // the finalized state has the unspent UTXO
                prop_assert!(finalized_state.utxo(&expected_outpoint).is_some());
                // the non-finalized state has spent the UTXO
                prop_assert!(chain.spent_utxos.contains(&expected_outpoint));
            } else {
                // the non-finalized state has created and spent the UTXO
                prop_assert!(!chain.unspent_utxos().contains_key(&expected_outpoint));
                prop_assert!(chain.created_utxos.contains_key(&expected_outpoint));
                prop_assert!(chain.spent_utxos.contains(&expected_outpoint));
                // the finalized state does not have the UTXO
                prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
            }

            previous_non_finalized_state = non_finalized_state.clone();
        }

        let block3 = Arc::new(block3).prepare();
        let commit_result = validate_and_commit_non_finalized(
            &finalized_state.db,
            &mut non_finalized_state,
            block3
        );

        // the block was rejected
        if use_finalized_state_spend {
            prop_assert_eq!(
                commit_result,
                Err(MissingTransparentOutput {
                    outpoint: expected_outpoint,
                    location: "the non-finalized and finalized chain",
                }
                .into())
            );
        } else {
            prop_assert_eq!(
                commit_result,
                Err(DuplicateTransparentSpend {
                    outpoint: expected_outpoint,
                    location: "the non-finalized chain",
                }
                .into())
            );
        }
        prop_assert_eq!(Some((Height(2), block2.hash())), read::best_tip(&non_finalized_state, &finalized_state.db));

        // the non-finalized state did not change
        prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

        // Since the non-finalized state has not changed, we don't need to check it again
        if use_finalized_state_spend {
            // the finalized state has spent the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
        } else if use_finalized_state_output {
            // the finalized state has the unspent UTXO
            // but the non-finalized state has spent it
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_some());
        } else {
            // the non-finalized state has created and spent the UTXO
            // and the finalized state does not have the UTXO
            prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
        }
    }

    /// Make sure a transparent spend with a missing UTXO
    /// is rejected by state contextual validation.
    #[test]
    fn reject_missing_transparent_spend(
        unused_utxo in TypeNameToDebug::<transparent::Utxo>::arbitrary(),
        prevout_input in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
    ) {
        let _init_guard = zebra_test::init();

        let mut block1 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_1_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        let expected_outpoint = prevout_input.outpoint().unwrap();
        let spend_transaction = komodo_transaction_v4_with_transparent_data(
            [prevout_input.0],
            // provide an fake spent output for value fixups
            [(expected_outpoint, unused_utxo.0)],
            []
        );

        // convert the coinbase transaction to a version that the non-finalized state will accept
        block1.transactions[0] = transaction_v4_from_coinbase(&block1.transactions[0]).into();

        block1.transactions.push(spend_transaction.into());

        let (finalized_state, mut non_finalized_state, genesis) = komodo_new_state_with_mainnet_genesis();
        let previous_non_finalized_state = non_finalized_state.clone();

        let block1 = Arc::new(block1).prepare();
        let commit_result = validate_and_commit_non_finalized(
            &finalized_state.db,
            &mut non_finalized_state,
            block1
        );

        // the block was rejected
        prop_assert_eq!(
            commit_result,
            Err(MissingTransparentOutput {
                outpoint: expected_outpoint,
                location: "the non-finalized and finalized chain",
            }
            .into())
        );
        prop_assert_eq!(Some((Height(0), genesis.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));

        // the non-finalized state did not change
        prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

        // the finalized state does not have the UTXO
        prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
    }

    /// Make sure transparent output spends are rejected by state contextual validation,
    /// if they spend an output in the same or later transaction in the block.
    ///
    /// This test covers a potential edge case where later transactions can spend outputs
    /// of previous transactions in a block, but earlier transactions can not spend later outputs.
    #[test]
    fn reject_earlier_transparent_spend_from_this_block(
        utxo in TypeNameToDebug::<transparent::Utxo>::arbitrary(),
        mut prevout_input in TypeNameToDebug::<transparent::Input>::arbitrary_with(None),
    ) {
        let _init_guard = zebra_test::init();

        let mut block1 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_1_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        // create an output
        let output_transaction = komodo_transaction_v4_with_transparent_data([], [], [utxo.0.output.clone()]);

        // create a spend
        let expected_outpoint = transparent::OutPoint {
            hash: output_transaction.hash(),
            index: 0,
        };
        prevout_input.set_outpoint(expected_outpoint);
        let spend_transaction = komodo_transaction_v4_with_transparent_data(
            [prevout_input.0],
            [(expected_outpoint, utxo.0)],
            []
        );

        // convert the coinbase transaction to a version that the non-finalized state will accept
        block1.transactions[0] = transaction_v4_from_coinbase(&block1.transactions[0]).into();

        // put the spend transaction before the output transaction in the block
        block1
            .transactions
            .extend([spend_transaction.into(), output_transaction.into()]);

            let (finalized_state, mut non_finalized_state, genesis) = komodo_new_state_with_mainnet_genesis();
            let previous_non_finalized_state = non_finalized_state.clone();

        let block1 = Arc::new(block1).prepare();
        let commit_result = validate_and_commit_non_finalized(
            &finalized_state.db,
            &mut non_finalized_state,
            block1
        );

        // the block was rejected
        prop_assert_eq!(
            commit_result,
            Err(EarlyTransparentSpend {
                outpoint: expected_outpoint,
            }
            .into())
        );
        prop_assert_eq!(Some((Height(0), genesis.hash)), read::best_tip(&non_finalized_state, &finalized_state.db));

        // the non-finalized state did not change
        prop_assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

        // the finalized state does not have the UTXO
        prop_assert!(finalized_state.utxo(&expected_outpoint).is_none());
    }

    /// Komodo:
    /// Ensure komodo interest is calulated and added to transaction input pool so it can be spent
    /// Also ensure that the chain pool value includes the interest if it is added to the spending tx output
    /// Try this both for finalized and non-finalized state
    #[test]
    fn komodo_value_pool_with_interest(
        use_finalized_state_output in any::<bool>(),    // controls block1 placement
        mut use_finalized_state_spend in any::<bool>(), // controls block3 placement 
    ) {
        let _init_guard = zebra_test::init();
        const TEST_BLOCK_INTERVAL_MIN: u64 = 60; // for minimum interest age to have it non-null
    
        // if we use the non-finalized state for the first block,
        // we have to use it for the second as well
        if !use_finalized_state_output {
            use_finalized_state_spend = false;
        }

        let mut block2 = zebra_test::komodo_vectors::BLOCK_KMDTESTNET_2_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");
        let mut block3 = zebra_test::komodo_vectors::BLOCK_KMDTESTNET_3_BYTES
            .zcash_deserialize_into::<Block>()
            .expect("block should deserialize");

        let TestState {
            mut finalized_state, mut non_finalized_state, block1, ..
        } = komodo_new_state_with_testnet_block1(use_finalized_state_output);
        let previous_non_finalized_state = non_finalized_state.clone();

        let nominator: u64 = (block1.transactions[0].outputs()[0].value() / 20u64).unwrap().try_into().unwrap();
        let denominator = 365 * 24 * 60 / TEST_BLOCK_INTERVAL_MIN;
        let testnet_interest = nominator / denominator;

        let header_2 = Arc::get_mut(&mut block2.header).unwrap();
        header_2.time = block1.header.time + chrono::Duration::seconds((TEST_BLOCK_INTERVAL_MIN*60).try_into().unwrap());

        let header_3 = Arc::get_mut(&mut block3.header).unwrap();
        header_3.time = block2.header.time + chrono::Duration::seconds((TEST_BLOCK_INTERVAL_MIN*60).try_into().unwrap());
        header_3.previous_block_hash = block2.hash(); // update prev block hash

        // convert the coinbase transactions to a version that the non-finalized state will accept
        block2.transactions[0] = transaction_v4_from_coinbase(&block2.transactions[0]).into();
        block3.transactions[0] = transaction_v4_from_coinbase(&block3.transactions[0]).into();

        let expected_outpoint = transparent::OutPoint {
            hash: block1.transactions[0].hash(),
            index: 0,
        };
        let prevout_input = transparent::Input::PrevOut {
            outpoint: expected_outpoint, 
            unlock_script: transparent::Script::new(&[0]), 
            sequence: std::u32::MAX, 
        };

        let utxo = transparent::Utxo { 
            output: block1.transactions[0].outputs()[0].clone(), 
            height: Height(1), 
            from_coinbase: true, 
            lock_time: LockTime::Time(block1.header.time) 
        };

        let mut next_utxo = transparent::Utxo { 
            output: utxo.clone().output, 
            height: Height(3), 
            from_coinbase: false, 
            lock_time: LockTime::Time(block3.header.time) 
        };
        // add the interest to the new tx output
        next_utxo.output.value = (next_utxo.output.value + Amount::<NonNegative>::try_from(testnet_interest).unwrap()).unwrap();

        // Note: remember that komodo_transaction_v4_with_transparent_data_testnet calls transaction.fix_remaining_value() which may zero tx outputs if it exceeds the input pool. This may affect tests
        let spend_transaction = komodo_transaction_v4_with_transparent_data_testnet(
            [prevout_input],
            [(expected_outpoint, utxo.clone())],
            [next_utxo.clone().output],
            Height(3),
            Some(block2.header.time)
        );

        block3.transactions.push(spend_transaction.into()); // add new tx to block3

        let block2 = Arc::new(block2);
        let block3 = Arc::new(block3);

        if use_finalized_state_spend {
            let block2 = FinalizedBlock::from(block2.clone());
            let commit_result = finalized_state.commit_finalized_direct(block2.clone().into(), "test");
            prop_assert!(commit_result.is_ok());
        } else {
            let block2 = block2.clone().prepare();
            let commit_result = validate_and_commit_non_finalized(
                &finalized_state.db,
                &mut non_finalized_state,
                block2.clone()
            );
        }

        if use_finalized_state_spend {
            let block3 = FinalizedBlock::from(block3.clone());
            let commit_result = finalized_state.commit_finalized_direct(block3.clone().into(), "test");
            prop_assert!(commit_result.is_ok());
        } else {
            let block3 = block3.clone().prepare();
            let commit_result = validate_and_commit_non_finalized(
                &finalized_state.db,
                &mut non_finalized_state,
                block3
            );
            prop_assert_eq!(commit_result, Ok(()));
        }

        let transparent_pool = if use_finalized_state_spend {
            finalized_state.finalized_value_pool().transparent_amount()
        } else {
            let chain = non_finalized_state
                .chain_set
                .iter()
                .next()
                .unwrap();
            chain.chain_value_pools.transparent_amount()
        };

        // Expected chain pool 2 blocks with 3 coin reward each plus spent block1 100 mln utxo with interest
        let expected_pool = Amount::<NonNegative>::try_from(2 * 3_0000_0000 + 100_000_000_0000_0000 + testnet_interest).expect("expected transparent pool amount valid");
        // println!("transparent_pool={:?} expected_pool={:?}", transparent_pool, expected_pool);
        prop_assert!(transparent_pool == expected_pool);
    }
}

/// State associated with transparent UTXO tests.
struct TestState {
    /// The pre-populated finalized state.
    finalized_state: FinalizedState,

    /// The pre-populated non-finalized state.
    non_finalized_state: NonFinalizedState,

    /// The genesis block that has already been committed to the `state` service's
    /// finalized state.
    #[allow(dead_code)]
    genesis: FinalizedBlock,

    /// A block at height 1, that has already been committed to the `state` service.
    block1: Arc<Block>,
}

/// Return a new `StateService` containing the mainnet genesis block.
/// Also returns the finalized genesis block itself.
fn komodo_new_state_with_mainnet_transparent_data(
    inputs: impl IntoIterator<Item = transparent::Input>,
    spent_utxos: impl IntoIterator<Item = (transparent::OutPoint, transparent::Utxo)>,
    outputs: impl IntoIterator<Item = transparent::Output>,
    use_finalized_state: bool,
) -> TestState {
    let (mut finalized_state, mut non_finalized_state, genesis) = komodo_new_state_with_mainnet_genesis();
    let previous_non_finalized_state = non_finalized_state.clone();

    let mut block1 = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_1_BYTES
        .zcash_deserialize_into::<Block>()
        .expect("block should deserialize");

    let outputs: Vec<_> = outputs.into_iter().collect();
    let outputs_len: u32 = outputs
        .len()
        .try_into()
        .expect("unexpectedly large output iterator");

    let transaction = komodo_transaction_v4_with_transparent_data(inputs, spent_utxos, outputs);
    let transaction_hash = transaction.hash();

    let expected_outpoints = (0..outputs_len).map(|index| transparent::OutPoint {
        hash: transaction_hash,
        index,
    });

    block1.transactions[0] = transaction_v4_from_coinbase(&block1.transactions[0]).into();
    block1.transactions.push(transaction.into());

    let block1 = Arc::new(block1);

    if use_finalized_state {
        let block1 = FinalizedBlock::from(block1.clone());
        let commit_result = finalized_state.commit_finalized_direct(block1.clone().into(), "test");

        // the block was committed
        assert_eq!(
            Some((Height(1), block1.hash)),
            read::best_tip(&non_finalized_state, &finalized_state.db)
        );
        assert!(commit_result.is_ok());

        // the non-finalized state didn't change
        assert!(non_finalized_state.eq_internal_state(&previous_non_finalized_state));

        for expected_outpoint in expected_outpoints {
            // the finalized state has the UTXOs
            assert!(finalized_state.utxo(&expected_outpoint).is_some());
            // the non-finalized state does not have the UTXOs
            assert!(non_finalized_state.any_utxo(&expected_outpoint).is_none());
        }
    } else {
        let block1 = block1.clone().prepare();
        let commit_result = validate_and_commit_non_finalized(
            &finalized_state.db,
            &mut non_finalized_state,
            block1.clone(),
        );

        // the block was committed
        assert_eq!(
            commit_result,
            Ok(()),
            "unexpected invalid block 1, modified with generated transactions: \n\
             converted coinbase: {:?} \n\
             generated non-coinbase: {:?}",
            block1.block.transactions[0],
            block1.block.transactions[1],
        );
        assert_eq!(
            Some((Height(1), block1.hash)),
            read::best_tip(&non_finalized_state, &finalized_state.db)
        );

        // the block data is in the non-finalized state
        assert!(!non_finalized_state.eq_internal_state(&previous_non_finalized_state));

        assert_eq!(non_finalized_state.chain_set.len(), 1);

        for expected_outpoint in expected_outpoints {
            // the non-finalized state has the unspent UTXOs
            assert!(non_finalized_state
                .chain_set
                .iter()
                .next()
                .unwrap()
                .unspent_utxos()
                .contains_key(&expected_outpoint));
            // the finalized state does not have the UTXOs
            assert!(finalized_state.utxo(&expected_outpoint).is_none());
        }
    }

    TestState {
        finalized_state,
        non_finalized_state,
        genesis,
        block1,
    }
}

/// Return a new `StateService` containing the komodo testnet genesis block.
/// Adds block1 into the finalized or non-finalized state
/// Fixes locktime in the block1 coinbase
fn komodo_new_state_with_testnet_block1(
    use_finalized_state: bool,
) -> TestState {
    let (mut finalized_state, mut non_finalized_state, genesis) = komodo_new_state_with_testnet_genesis();

    let mut block1 = zebra_test::komodo_vectors::BLOCK_KMDTESTNET_1_BYTES
        .zcash_deserialize_into::<Block>()
        .expect("block should deserialize");

    let coinbase: Transaction = transaction_v4_from_coinbase(&block1.transactions[0]).into();
    // set locktime in coinbase output to block.time:
    let coinbase = Transaction::V4 {
        inputs: coinbase.inputs().to_vec(),
        outputs: coinbase.outputs().to_vec(),
        lock_time: LockTime::Time(block1.header.time),
        expiry_height: coinbase.expiry_height().unwrap_or(Height(0)),
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    block1.transactions[0] = coinbase.into();
    let block1 = Arc::new(block1);

    if use_finalized_state {
        let block1 = FinalizedBlock::from(block1.clone());
        let commit_result = finalized_state.commit_finalized_direct(block1.clone().into(), "test");

        // the block was committed
        assert_eq!(
            Some((Height(1), block1.hash)),
            read::best_tip(&non_finalized_state, &finalized_state.db)
        );
        assert!(commit_result.is_ok());
    } else {
        let block1 = block1.clone().prepare();
        let commit_result = validate_and_commit_non_finalized(
            &finalized_state.db,
            &mut non_finalized_state,
            block1.clone(),
        );

        // the block was committed
        assert_eq!(
            commit_result,
            Ok(()),
            "unexpected invalid block 1, modified with generated transactions: \n\
             converted coinbase: {:?} \n\
             generated non-coinbase: {:?}",
            block1.block.transactions[0],
            block1.block.transactions[1],
        );
        assert_eq!(
            Some((Height(1), block1.hash)),
            read::best_tip(&non_finalized_state, &finalized_state.db)
        );
    }

    TestState {
        finalized_state,
        non_finalized_state,
        genesis,
        block1,
    }
}


/// Return a `Transaction::V4`, using transparent `inputs` and their `spent_outputs`,
/// and newly created `outputs`.
///
/// Other fields have empty or default values.
fn komodo_transaction_v4_with_transparent_data(
    inputs: impl IntoIterator<Item = transparent::Input>,
    spent_utxos: impl IntoIterator<Item = (transparent::OutPoint, transparent::Utxo)>,
    outputs: impl IntoIterator<Item = transparent::Output>,
) -> Transaction {
    let inputs: Vec<_> = inputs.into_iter().collect();
    let outputs: Vec<_> = outputs.into_iter().collect();

    let mut transaction = Transaction::V4 {
        inputs,
        outputs,
        lock_time: LockTime::min_lock_time_timestamp(),
        expiry_height: Height(0),
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    // do required fixups, but ignore any errors,
    // because we're not checking all the consensus rules here
    // Komodo: there is no interest on low blocks
    let _ = transaction.fix_remaining_value(Network::Mainnet, &spent_utxos.into_iter().collect(), Height(0), None);

    transaction
}

fn komodo_transaction_v4_with_transparent_data_testnet(
    inputs: impl IntoIterator<Item = transparent::Input>,
    spent_utxos: impl IntoIterator<Item = (transparent::OutPoint, transparent::Utxo)>,
    outputs: impl IntoIterator<Item = transparent::Output>,
    height: Height,
    last_block_time: Option<DateTime<Utc>>,
) -> Transaction {
    let inputs: Vec<_> = inputs.into_iter().collect();
    let outputs: Vec<_> = outputs.into_iter().collect();

    let mut transaction = Transaction::V4 {
        inputs,
        outputs,
        lock_time: LockTime::min_lock_time_timestamp(),
        expiry_height: Height(0),
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    // do required fixups, but ignore any errors,
    // because we're not checking all the consensus rules here
    let _ = transaction.fix_remaining_value(Network::Testnet, &spent_utxos.into_iter().collect(), height, last_block_time);

    transaction
}