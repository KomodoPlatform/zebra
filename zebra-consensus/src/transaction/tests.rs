//! Tests for Zcash transaction consensus checks.

use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
    sync::Arc,
};

use chrono::{DateTime, Utc, NaiveDateTime, Duration};
use halo2::pasta::{group::ff::PrimeField, pallas};
use proptest::{
    arbitrary::any,
    prelude::*, strategy::ValueTree,
};
use proptest::test_runner::TestRunner;

use tower::{service_fn, ServiceExt};

use zebra_chain::{
    amount::{Amount, NonNegative},
    block::{self, Block, Height, merkle::Root},
    orchard::AuthorizedAction,
    parameters::{Network, NetworkUpgrade},
    primitives::{ed25519, x25519, Groth16Proof},
    sapling,
    serialization::{ZcashDeserialize, ZcashDeserializeInto, arbitrary::datetime_full},
    sprout,
    transaction::{
        arbitrary::{
            fake_v5_transactions_for_network, insert_fake_orchard_shielded_data, test_transactions, komodo_transactions_from_blocks,
        },
        Hash, HashType, JoinSplitData, LockTime, Transaction, self, UnminedTx,
    },
    transparent::{self, CoinbaseData, Input, Output},
};

use zebra_state::HashOrHeight;
use zebra_test::mock_service::MockService;

use super::{check, Request, Verifier};

use crate::error::TransactionError;
use color_eyre::eyre::Report;

#[cfg(test)]
mod prop;

const FAKE_PREV_BLOCK_HASH: block::Hash = block::Hash([0x1f; 32]);

/// fake state service to return prev block time and mtp 
async fn fake_state_handler(request: zebra_state::Request) -> Result<zebra_state::Response, zebra_state::BoxError>     {
    
    let mut runner = TestRunner::default();
    match request {
        zebra_state::Request::GetMedianTimePast(block_hash) => {
            let rsp = {
                if let Some(block_hash) = block_hash {
                    if block_hash == FAKE_PREV_BLOCK_HASH    {
                        let fake_mtp = datetime_full().new_tree(&mut runner).unwrap().current(); //Utc::now();
                        return Ok(zebra_state::Response::MedianTimePast(Some(fake_mtp)));
                    }
                } 
                Ok(zebra_state::Response::MedianTimePast(None))
            };
            return async move { rsp }.await;
        },
        zebra_state::Request::AwaitBlock(block_hash) => {
            let rsp = {
                if block_hash == FAKE_PREV_BLOCK_HASH    {
                    let fake_header = block::Header { 
                        version: 4,
                        time: datetime_full().new_tree(&mut runner).unwrap().current(), // Utc::now(),  
                        previous_block_hash: FAKE_PREV_BLOCK_HASH, 
                        merkle_root: any::<block::merkle::Root>().new_tree(&mut runner).unwrap().current(), //Root([0x3f; 32]),
                        commitment_bytes: any::<[u8; 32]>().new_tree(&mut runner).unwrap().current(), //[0x4f; 32],
                        difficulty_threshold: zebra_chain::work::difficulty::ExpandedDifficulty::from(zebra_chain::work::difficulty::U256::MAX).into(),
                        nonce: any::<[u8; 32]>().new_tree(&mut runner).unwrap().current(), // [0x6f; 32],
                        solution: any::<zebra_chain::work::equihash::Solution>().new_tree(&mut runner).unwrap().current() // ([0x11u8; 1344])
                    };
                    let fake_block = Arc::new( Block { header: fake_header.into(), transactions: vec![] } );
                    return Ok(zebra_state::Response::Block(Some(fake_block)));
                }
                Ok(zebra_state::Response::Block(None))
            };
            return async move { rsp }.await;
        },

        zebra_state::Request::Block(h_or_ht) => {
            let rsp = {
                if let HashOrHeight::Hash(block_hash) = h_or_ht {
                    if block_hash == FAKE_PREV_BLOCK_HASH    {
                        let fake_header = block::Header { 
                            version: 4,
                            time: datetime_full().new_tree(&mut runner).unwrap().current(), // Utc::now(),  
                            previous_block_hash: FAKE_PREV_BLOCK_HASH, 
                            merkle_root: any::<block::merkle::Root>().new_tree(&mut runner).unwrap().current(), //Root([0x3f; 32]),
                            commitment_bytes: any::<[u8; 32]>().new_tree(&mut runner).unwrap().current(), //[0x4f; 32],
                            difficulty_threshold: zebra_chain::work::difficulty::ExpandedDifficulty::from(zebra_chain::work::difficulty::U256::MAX).into(),
                            nonce: any::<[u8; 32]>().new_tree(&mut runner).unwrap().current(), // [0x6f; 32],
                            solution: any::<zebra_chain::work::equihash::Solution>().new_tree(&mut runner).unwrap().current() // ([0x11u8; 1344])
                        };
                        let fake_block = Arc::new( Block { header: fake_header.into(), transactions: vec![] } );
                        return Ok(zebra_state::Response::Block(Some(fake_block)));
                    }
                }
                Ok(zebra_state::Response::Block(None))
            };
            return async move { rsp }.await;
        },
        _ => unreachable!("no other request is allowed"),
    }
}


#[test]
fn v5_fake_transactions() -> Result<(), Report> {
    zebra_test::init();

    let networks = vec![
        (Network::Mainnet, zebra_test::vectors::MAINNET_BLOCKS.iter()),
        (Network::Testnet, zebra_test::vectors::TESTNET_BLOCKS.iter()),
    ];

    for (network, blocks) in networks {
        for transaction in fake_v5_transactions_for_network(network, blocks) {
            match check::has_inputs_and_outputs(&transaction) {
                Ok(()) => (),
                Err(TransactionError::NoInputs) | Err(TransactionError::NoOutputs) => (),
                Err(_) => panic!("error must be NoInputs or NoOutputs"),
            };

            // make sure there are no joinsplits nor spends in coinbase
            check::coinbase_tx_no_prevout_joinsplit_spend(&transaction)?;
        }
    }

    Ok(())
}

#[test]
fn fake_v5_transaction_with_orchard_actions_has_inputs_and_outputs() {
    // Find a transaction with no inputs or outputs to use as base
    let mut transaction = fake_v5_transactions_for_network(
        Network::Mainnet,
        zebra_test::vectors::MAINNET_BLOCKS.iter(),
    )
    .rev()
    .find(|transaction| {
        transaction.inputs().is_empty()
            && transaction.outputs().is_empty()
            && transaction.sapling_spends_per_anchor().next().is_none()
            && transaction.sapling_outputs().next().is_none()
            && transaction.joinsplit_count() == 0
    })
    .expect("At least one fake V5 transaction with no inputs and no outputs");

    // Insert fake Orchard shielded data to the transaction, which has at least one action (this is
    // guaranteed structurally by `orchard::ShieldedData`)
    insert_fake_orchard_shielded_data(&mut transaction);

    // The check will fail if the transaction has no flags
    assert_eq!(
        check::has_inputs_and_outputs(&transaction),
        Err(TransactionError::NoInputs)
    );

    // If we add ENABLE_SPENDS flag it will pass the inputs check but fails with the outputs
    // TODO: Avoid new calls to `insert_fake_orchard_shielded_data` for each check #2409.
    let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);
    shielded_data.flags = zebra_chain::orchard::Flags::ENABLE_SPENDS;

    assert_eq!(
        check::has_inputs_and_outputs(&transaction),
        Err(TransactionError::NoOutputs)
    );

    // If we add ENABLE_OUTPUTS flag it will pass the outputs check but fails with the inputs
    let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);
    shielded_data.flags = zebra_chain::orchard::Flags::ENABLE_OUTPUTS;

    assert_eq!(
        check::has_inputs_and_outputs(&transaction),
        Err(TransactionError::NoInputs)
    );

    // Finally make it valid by adding both required flags
    let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);
    shielded_data.flags =
        zebra_chain::orchard::Flags::ENABLE_SPENDS | zebra_chain::orchard::Flags::ENABLE_OUTPUTS;

    assert!(check::has_inputs_and_outputs(&transaction).is_ok());
}

#[test]
fn fake_v5_transaction_with_orchard_actions_has_flags() {
    // Find a transaction with no inputs or outputs to use as base
    let mut transaction = fake_v5_transactions_for_network(
        Network::Mainnet,
        zebra_test::vectors::MAINNET_BLOCKS.iter(),
    )
    .rev()
    .find(|transaction| {
        transaction.inputs().is_empty()
            && transaction.outputs().is_empty()
            && transaction.sapling_spends_per_anchor().next().is_none()
            && transaction.sapling_outputs().next().is_none()
            && transaction.joinsplit_count() == 0
    })
    .expect("At least one fake V5 transaction with no inputs and no outputs");

    // Insert fake Orchard shielded data to the transaction, which has at least one action (this is
    // guaranteed structurally by `orchard::ShieldedData`)
    insert_fake_orchard_shielded_data(&mut transaction);

    // The check will fail if the transaction has no flags
    assert_eq!(
        check::has_enough_orchard_flags(&transaction),
        Err(TransactionError::NotEnoughFlags)
    );

    // If we add ENABLE_SPENDS flag it will pass.
    let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);
    shielded_data.flags = zebra_chain::orchard::Flags::ENABLE_SPENDS;
    assert!(check::has_enough_orchard_flags(&transaction).is_ok());

    // If we add ENABLE_OUTPUTS flag instead, it will pass.
    let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);
    shielded_data.flags = zebra_chain::orchard::Flags::ENABLE_OUTPUTS;
    assert!(check::has_enough_orchard_flags(&transaction).is_ok());

    // If we add BOTH ENABLE_SPENDS and ENABLE_OUTPUTS flags it will pass.
    let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);
    shielded_data.flags =
        zebra_chain::orchard::Flags::ENABLE_SPENDS | zebra_chain::orchard::Flags::ENABLE_OUTPUTS;
    assert!(check::has_enough_orchard_flags(&transaction).is_ok());
}

#[test]
fn v5_transaction_with_no_inputs_fails_validation() {
    let transaction = fake_v5_transactions_for_network(
        Network::Mainnet,
        zebra_test::vectors::MAINNET_BLOCKS.iter(),
    )
    .rev()
    .find(|transaction| {
        transaction.inputs().is_empty()
            && transaction.sapling_spends_per_anchor().next().is_none()
            && transaction.orchard_actions().next().is_none()
            && transaction.joinsplit_count() == 0
            && (!transaction.outputs().is_empty() || transaction.sapling_outputs().next().is_some())
    })
    .expect("At least one fake v5 transaction with no inputs in the test vectors");

    assert_eq!(
        check::has_inputs_and_outputs(&transaction),
        Err(TransactionError::NoInputs)
    );
}

#[test]
fn v5_transaction_with_no_outputs_fails_validation() {
    let transaction = fake_v5_transactions_for_network(
        Network::Mainnet,
        zebra_test::vectors::MAINNET_BLOCKS.iter(),
    )
    .rev()
    .find(|transaction| {
        transaction.outputs().is_empty()
            && transaction.sapling_outputs().next().is_none()
            && transaction.orchard_actions().next().is_none()
            && transaction.joinsplit_count() == 0
            && (!transaction.inputs().is_empty()
                || transaction.sapling_spends_per_anchor().next().is_some())
    })
    .expect("At least one fake v5 transaction with no outputs in the test vectors");

    assert_eq!(
        check::has_inputs_and_outputs(&transaction),
        Err(TransactionError::NoOutputs)
    );
}

#[test]
fn v5_coinbase_transaction_without_enable_spends_flag_passes_validation() {
    let mut transaction = fake_v5_transactions_for_network(
        Network::Mainnet,
        zebra_test::vectors::MAINNET_BLOCKS.iter(),
    )
    .rev()
    .find(|transaction| transaction.is_coinbase())
    .expect("At least one fake V5 coinbase transaction in the test vectors");

    insert_fake_orchard_shielded_data(&mut transaction);

    assert!(check::coinbase_tx_no_prevout_joinsplit_spend(&transaction).is_ok());
}

#[test]
fn v5_coinbase_transaction_with_enable_spends_flag_fails_validation() {
    let mut transaction = fake_v5_transactions_for_network(
        Network::Mainnet,
        zebra_test::vectors::MAINNET_BLOCKS.iter(),
    )
    .rev()
    .find(|transaction| transaction.is_coinbase())
    .expect("At least one fake V5 coinbase transaction in the test vectors");

    let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);

    shielded_data.flags = zebra_chain::orchard::Flags::ENABLE_SPENDS;

    assert_eq!(
        check::coinbase_tx_no_prevout_joinsplit_spend(&transaction),
        Err(TransactionError::CoinbaseHasEnableSpendsOrchard)
    );
}

#[ignore = "V5 not supported in komodo"] // Not supported in Komodo due to different network upgrades and blocks
#[tokio::test]
async fn v5_transaction_is_rejected_before_nu5_activation() {
    const V5_TRANSACTION_VERSION: u32 = 5;

    let canopy = NetworkUpgrade::Canopy;
    let networks = vec![
        (Network::Mainnet, zebra_test::vectors::MAINNET_BLOCKS.iter()),
        (Network::Testnet, zebra_test::vectors::TESTNET_BLOCKS.iter()),
    ];

    for (network, blocks) in networks {
        let state_service = service_fn(|_| async { unreachable!("Service should not be called") });
        let verifier = Verifier::new(network, state_service);

        let transaction = fake_v5_transactions_for_network(network, blocks)
            .rev()
            .next()
            .expect("At least one fake V5 transaction in the test vectors");

        let result = verifier
            .oneshot(Request::Block {
                transaction: Arc::new(transaction),
                known_utxos: Arc::new(HashMap::new()),
                height: canopy
                    .activation_height(network)
                    .expect("Canopy activation height is specified"),
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result,
            Err(TransactionError::UnsupportedByNetworkUpgrade(
                V5_TRANSACTION_VERSION,
                canopy
            ))
        );
    }
}

#[ignore = "V5 not supported in komodo"]  // Not supported in Komodo due to different network upgrades and blocks
#[test]
fn v5_transaction_is_accepted_after_nu5_activation_mainnet() {
    v5_transaction_is_accepted_after_nu5_activation_for_network(Network::Mainnet)
}

#[ignore = "V5 not supported in komodo"]  // Not supported in Komodo due to different network upgrades and blocks
#[test]
fn v5_transaction_is_accepted_after_nu5_activation_testnet() {
    v5_transaction_is_accepted_after_nu5_activation_for_network(Network::Testnet)
}

fn v5_transaction_is_accepted_after_nu5_activation_for_network(network: Network) {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let nu5 = NetworkUpgrade::Nu5;
        let nu5_activation_height = nu5
            .activation_height(network)
            .expect("NU5 activation height is specified");

        let blocks = match network {
            Network::Mainnet => zebra_test::vectors::MAINNET_BLOCKS.iter(),
            Network::Testnet => zebra_test::vectors::TESTNET_BLOCKS.iter(),
        };

        let state_service = service_fn(|_| async { unreachable!("Service should not be called") });
        let verifier = Verifier::new(network, state_service);

        let mut transaction = fake_v5_transactions_for_network(network, blocks)
            .rev()
            .next()
            .expect("At least one fake V5 transaction in the test vectors");
        if transaction
            .expiry_height()
            .expect("V5 must have expiry_height")
            < nu5_activation_height
        {
            let expiry_height = transaction.expiry_height_mut();
            *expiry_height = nu5_activation_height;
        }

        let expected_hash = transaction.unmined_id();
        let expiry_height = transaction
            .expiry_height()
            .expect("V5 must have expiry_height");

        let result = verifier
            .oneshot(Request::Block {
                transaction: Arc::new(transaction),
                known_utxos: Arc::new(HashMap::new()),
                height: expiry_height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result.expect("unexpected error response").tx_id(),
            expected_hash
        );
    })
}

/// Test if V4 transaction with transparent funds is accepted.
#[tokio::test]
async fn komodo_v4_transaction_with_transparent_transfer_is_accepted() {
    let network = Network::Mainnet;

    let sapling_activation_height = NetworkUpgrade::Sapling
        .activation_height(network)
        .expect("Sapling activation height is specified");

    let transaction_block_height =
        (sapling_activation_height + 10).expect("transaction block height is too large");

    let fake_source_fund_height =
        (transaction_block_height - 1).expect("fake source fund block height is too small");

    // Create a fake transparent transfer that should succeed
    let (input, output, known_utxos) = mock_transparent_transfer(fake_source_fund_height, true, 0);

    // Create a V4 transaction
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::Height(block::Height(0)),
        expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let transaction_hash = transaction.unmined_id();

    let state_service =
        service_fn(/*|_| async { unreachable!("State service should not be called") }*/ fake_state_handler);
    let verifier = Verifier::new(network, state_service);

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(known_utxos),
            height: transaction_block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        transaction_hash
    );
}

/// Tests if a non-coinbase V4 transaction with the last valid expiry height is
/// accepted.
/// fixed for Komodo with fake_state_handler()
#[tokio::test]
async fn komodo_v4_transaction_with_last_valid_expiry_height() {
    let state_service =
        service_fn(/*|_| async { unreachable!("State service should not be called") }*/ fake_state_handler);
    let verifier = Verifier::new(Network::Mainnet, state_service);

    let block_height = NetworkUpgrade::Sapling
        .activation_height(Network::Mainnet)
        .expect("Sapling activation height is specified");
    let fund_height = (block_height - 1).expect("fake source fund block height is too small");
    let (input, output, known_utxos) = mock_transparent_transfer(fund_height, true, 0);

    // Create a non-coinbase V4 tx with the last valid expiry height.
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height: block_height,
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(known_utxos),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        transaction.unmined_id()
    );
}

/// Tests if a coinbase V4 transaction with an expiry height lower than the
/// block height is accepted.
///
/// Note that an expiry height lower than the block height is considered
/// *expired* for *non-coinbase* transactions.
/// fixed for Komodo with fake_state_handler()
#[tokio::test]
async fn komodo_v4_coinbase_transaction_with_low_expiry_height() {
    let state_service =
        service_fn(/*|_| async { unreachable!("State service should not be called") }*/ fake_state_handler);
    let verifier = Verifier::new(Network::Mainnet, state_service);

    let block_height = NetworkUpgrade::Sapling
        .activation_height(Network::Mainnet)
        .expect("Sapling activation height is specified");

    let (input, output) = mock_coinbase_transparent_output(block_height);

    // This is a correct expiry height for coinbase V4 transactions.
    let expiry_height = (block_height - 1).expect("original block height is too small");

    // Create a coinbase V4 tx.
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height,
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(HashMap::new()),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        transaction.unmined_id()
    );
}

/// Tests if an expired non-coinbase V4 transaction is rejected.
#[tokio::test]
async fn komodo_v4_transaction_with_too_low_expiry_height() {
    let state_service =
        service_fn(|_| async { unreachable!("State service should not be called") });
    let verifier = Verifier::new(Network::Mainnet, state_service);

    let block_height = NetworkUpgrade::Sapling
        .activation_height(Network::Mainnet)
        .expect("Sapling activation height is specified");

    let fund_height = (block_height - 1).expect("fake source fund block height is too small");
    let (input, output, known_utxos) = mock_transparent_transfer(fund_height, true, 0);

    // This expiry height is too low so that the tx should seem expired to the verifier.
    let expiry_height = (block_height - 1).expect("original block height is too small");

    // Create a non-coinbase V4 tx.
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height,
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(known_utxos),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::ExpiredTransaction {
            expiry_height,
            block_height,
            transaction_hash: transaction.hash(),
        })
    );
}

/// Tests if a non-coinbase V4 transaction with an expiry height exceeding the
/// maximum is rejected.
#[tokio::test]
async fn v4_transaction_with_exceeding_expiry_height() {
    let state_service =
        service_fn(|_| async { unreachable!("State service should not be called") });
    let verifier = Verifier::new(Network::Mainnet, state_service);

    let block_height = block::Height::MAX;

    let fund_height = (block_height - 1).expect("fake source fund block height is too small");
    let (input, output, known_utxos) = mock_transparent_transfer(fund_height, true, 0);

    // This expiry height exceeds the maximum defined by the specification.
    let expiry_height = block::Height(500_000_000);

    // Create a non-coinbase V4 tx.
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height,
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(known_utxos),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::MaximumExpiryHeight {
            expiry_height,
            is_coinbase: false,
            block_height,
            transaction_hash: transaction.hash(),
        })
    );
}

/// Tests if a coinbase V4 transaction with an expiry height exceeding the
/// maximum is rejected.
#[tokio::test]
async fn v4_coinbase_transaction_with_exceeding_expiry_height() {
    let state_service =
        service_fn(|_| async { unreachable!("State service should not be called") });
    let verifier = Verifier::new(Network::Mainnet, state_service);

    // Use an arbitrary pre-NU5 block height.
    // It can't be NU5-onward because the expiry height limit is not enforced
    // for coinbase transactions (it needs to match the block height instead),
    // which is what is used in this test.
    let block_height = (NetworkUpgrade::Nu5
        .activation_height(Network::Mainnet)
        .expect("NU5 height must be set")
        - 1)
    .expect("will not underflow");

    let (input, output) = mock_coinbase_transparent_output(block_height);

    // This expiry height exceeds the maximum defined by the specification.
    let expiry_height = block::Height(500_000_000);

    // Create a coinbase V4 tx.
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height,
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(HashMap::new()),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::MaximumExpiryHeight {
            expiry_height,
            is_coinbase: true,
            block_height,
            transaction_hash: transaction.hash(),
        })
    );
}

/// Test if V4 coinbase transaction is accepted.
/// Fixed for Komodo with fake_state_handler
#[tokio::test]
async fn komodo_v4_coinbase_transaction_is_accepted() {
    let network = Network::Mainnet;

    let sapling_activation_height = NetworkUpgrade::Sapling
        .activation_height(network)
        .expect("Sapling activation height is specified");

    let transaction_block_height =
        (sapling_activation_height + 10).expect("transaction block height is too large");

    // Create a fake transparent coinbase that should succeed
    let (input, output) = mock_coinbase_transparent_output(transaction_block_height);

    // Create a V4 coinbase transaction
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::Height(block::Height(0)),
        expiry_height: transaction_block_height,
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let transaction_hash = transaction.unmined_id();

    let state_service =
        service_fn(fake_state_handler);

    let verifier = Verifier::new(network, state_service);

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(HashMap::new()),
            height: transaction_block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, 
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        transaction_hash
    );
}

/// Test if V4 transaction with transparent funds is rejected if the source script prevents it.
///
/// This test simulates the case where the script verifier rejects the transaction because the
/// script prevents spending the source UTXO.
/// Fixed for Komodo with fake_state_handler
#[tokio::test]
async fn komodo_v4_transaction_with_transparent_transfer_is_rejected_by_the_script() {
    let network = Network::Mainnet;

    let sapling_activation_height = NetworkUpgrade::Sapling
        .activation_height(network)
        .expect("Sapling activation height is specified");

    let transaction_block_height =
        (sapling_activation_height + 10).expect("transaction block height is too large");

    let fake_source_fund_height =
        (transaction_block_height - 1).expect("fake source fund block height is too small");

    // Create a fake transparent transfer that should not succeed
    let (input, output, known_utxos) = mock_transparent_transfer(fake_source_fund_height, false, 0);

    // Create a V4 transaction
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::Height(block::Height(0)),
        expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let state_service =
        service_fn(/*|_| async { unreachable!("State service should not be called") }*/ fake_state_handler);
    let verifier = Verifier::new(network, state_service);

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(known_utxos),
            height: transaction_block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::InternalDowncastError(
            "downcast to known transaction error type failed, original error: ScriptInvalid"
                .to_string()
        ))
    );
}

/// Test if V4 transaction with an internal double spend of transparent funds is rejected.
#[tokio::test]
async fn komodo_v4_transaction_with_conflicting_transparent_spend_is_rejected() {
    let network = Network::Mainnet;

    let sapling_activation_height = NetworkUpgrade::Sapling
        .activation_height(network)
        .expect("Sapling activation height is specified");

    let transaction_block_height =
        (sapling_activation_height + 10).expect("transaction block height is too large");

    let fake_source_fund_height =
        (transaction_block_height - 1).expect("fake source fund block height is too small");

    // Create a fake transparent transfer that should succeed
    let (input, output, known_utxos) = mock_transparent_transfer(fake_source_fund_height, true, 0);

    // Create a V4 transaction
    let transaction = Transaction::V4 {
        inputs: vec![input.clone(), input.clone()],
        outputs: vec![output],
        lock_time: LockTime::Height(block::Height(0)),
        expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let state_service =
        service_fn(|_| async { unreachable!("State service should not be called") });
    let verifier = Verifier::new(network, state_service);

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(known_utxos),
            height: transaction_block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    let expected_outpoint = input.outpoint().expect("Input should have an outpoint");

    assert_eq!(
        result,
        Err(TransactionError::DuplicateTransparentSpend(
            expected_outpoint
        ))
    );
}

/// Test if V4 transaction with a joinsplit that has duplicate nullifiers is rejected.
#[test]
fn komodo_v4_transaction_with_conflicting_sprout_nullifier_inside_joinsplit_is_rejected() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;
        let network_upgrade = NetworkUpgrade::Sapling;

        let sapling_activation_height = NetworkUpgrade::Sapling
            .activation_height(network)
            .expect("Sapling activation height is specified");

        let transaction_block_height =
            (sapling_activation_height + 10).expect("transaction block height is too large");

        // Create a fake Sprout join split
        let (mut joinsplit_data, signing_key) = mock_sprout_join_split_data();

        // Make both nullifiers the same inside the joinsplit transaction
        let duplicate_nullifier = joinsplit_data.first.nullifiers[0];
        joinsplit_data.first.nullifiers[1] = duplicate_nullifier;

        // Create a V4 transaction
        let mut transaction = Transaction::V4 {
            inputs: vec![],
            outputs: vec![],
            lock_time: LockTime::Height(block::Height(0)),
            expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
            joinsplit_data: Some(joinsplit_data),
            sapling_shielded_data: None,
        };

        // Sign the transaction
        let sighash = transaction.sighash(network_upgrade, HashType::ALL, &[], None);

        match &mut transaction {
            Transaction::V4 {
                joinsplit_data: Some(joinsplit_data),
                ..
            } => joinsplit_data.sig = signing_key.sign(sighash.as_ref()),
            _ => unreachable!("Mock transaction was created incorrectly"),
        }

        let state_service =
            service_fn(|_| async { unreachable!("State service should not be called") });
        let verifier = Verifier::new(network, state_service);

        let result = verifier
            .oneshot(Request::Block {
                transaction: Arc::new(transaction),
                known_utxos: Arc::new(HashMap::new()),
                height: transaction_block_height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result,
            Err(TransactionError::DuplicateSproutNullifier(
                duplicate_nullifier
            ))
        );
    });
}

/// Test if V4 transaction with duplicate nullifiers across joinsplits is rejected.
#[test]
fn komodo_v4_transaction_with_conflicting_sprout_nullifier_across_joinsplits_is_rejected() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;
        let network_upgrade = NetworkUpgrade::Sapling;

        let sapling_activation_height = NetworkUpgrade::Sapling
            .activation_height(network)
            .expect("Sapling activation height is specified");

        let transaction_block_height =
            (sapling_activation_height + 10).expect("transaction block height is too large");

        // Create a fake Sprout join split
        let (mut joinsplit_data, signing_key) = mock_sprout_join_split_data();

        // Duplicate a nullifier from the created joinsplit
        let duplicate_nullifier = joinsplit_data.first.nullifiers[1];

        // Add a new joinsplit with the duplicate nullifier
        let mut new_joinsplit = joinsplit_data.first.clone();
        new_joinsplit.nullifiers[0] = duplicate_nullifier;
        new_joinsplit.nullifiers[1] = sprout::note::Nullifier([2u8; 32]);

        joinsplit_data.rest.push(new_joinsplit);

        // Create a V4 transaction
        let mut transaction = Transaction::V4 {
            inputs: vec![],
            outputs: vec![],
            lock_time: LockTime::Height(block::Height(0)),
            expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
            joinsplit_data: Some(joinsplit_data),
            sapling_shielded_data: None,
        };

        // Sign the transaction
        let sighash = transaction.sighash(network_upgrade, HashType::ALL, &[], None);

        match &mut transaction {
            Transaction::V4 {
                joinsplit_data: Some(joinsplit_data),
                ..
            } => joinsplit_data.sig = signing_key.sign(sighash.as_ref()),
            _ => unreachable!("Mock transaction was created incorrectly"),
        }

        let state_service =
            service_fn(|_| async { unreachable!("State service should not be called") });
        let verifier = Verifier::new(network, state_service);

        let result = verifier
            .oneshot(Request::Block {
                transaction: Arc::new(transaction),
                known_utxos: Arc::new(HashMap::new()),
                height: transaction_block_height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result,
            Err(TransactionError::DuplicateSproutNullifier(
                duplicate_nullifier
            ))
        );
    });
}

/// Test if V5 transaction with transparent funds is accepted.
#[ignore = "V5 not supported in komodo"] // Nu5 not supported in Komodo
#[tokio::test]
async fn v5_transaction_with_transparent_transfer_is_accepted() {
    let network = Network::Testnet;
    let network_upgrade = NetworkUpgrade::Nu5;

    let nu5_activation_height = network_upgrade
        .activation_height(network)
        .expect("NU5 activation height is specified");

    let transaction_block_height =
        (nu5_activation_height + 10).expect("transaction block height is too large");

    let fake_source_fund_height =
        (transaction_block_height - 1).expect("fake source fund block height is too small");

    // Create a fake transparent transfer that should succeed
    let (input, output, known_utxos) = mock_transparent_transfer(fake_source_fund_height, true, 0);

    // Create a V5 transaction
    let transaction = Transaction::V5 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::Height(block::Height(0)),
        expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
        sapling_shielded_data: None,
        orchard_shielded_data: None,
        network_upgrade,
    };

    let transaction_hash = transaction.unmined_id();

    let state_service =
        service_fn(/*|_| async { unreachable!("State service should not be called") }*/ fake_state_handler);
    let verifier = Verifier::new(network, state_service);

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(known_utxos),
            height: transaction_block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        transaction_hash
    );
}

/// Tests if a non-coinbase V5 transaction with the last valid expiry height is
/// accepted.
#[ignore = "V5 not supported in komodo"] // Nu5 not supported in Komodo
#[tokio::test]
async fn v5_transaction_with_last_valid_expiry_height() {
    let state_service =
        service_fn(fake_state_handler);
    let verifier = Verifier::new(Network::Testnet, state_service);

    let block_height = NetworkUpgrade::Nu5
        .activation_height(Network::Testnet)
        .expect("Nu5 activation height for testnet is specified");
    let fund_height = (block_height - 1).expect("fake source fund block height is too small");
    let (input, output, known_utxos) = mock_transparent_transfer(fund_height, true, 0);

    // Create a non-coinbase V5 tx with the last valid expiry height.
    let transaction = Transaction::V5 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height: block_height,
        sapling_shielded_data: None,
        orchard_shielded_data: None,
        network_upgrade: NetworkUpgrade::Nu5,
    };

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(known_utxos),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        transaction.unmined_id()
    );
}

/// Tests that a coinbase V5 transaction is accepted only if its expiry height
/// is equal to the height of the block the transaction belongs to.
#[ignore = "V5 not supported in komodo"]   // Nu5 not supported in komodo
#[tokio::test]
async fn v5_coinbase_transaction_expiry_height() {
    let state_service =
        service_fn(fake_state_handler);
    let verifier = Verifier::new(Network::Testnet, state_service);

    let block_height = NetworkUpgrade::Nu5
        .activation_height(Network::Testnet)
        .expect("Nu5 activation height for testnet is specified");

    let (input, output) = mock_coinbase_transparent_output(block_height);

    // Create a coinbase V5 tx with an expiry height that matches the height of
    // the block. Note that this is the only valid expiry height for a V5
    // coinbase tx.
    let transaction = Transaction::V5 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height: block_height,
        sapling_shielded_data: None,
        orchard_shielded_data: None,
        network_upgrade: NetworkUpgrade::Nu5,
    };

    let result = verifier
        .clone()
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(HashMap::new()),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        transaction.unmined_id()
    );

    // Increment the expiry height so that it becomes invalid.
    let new_expiry_height = (block_height + 1).expect("transaction block height is too large");
    let mut new_transaction = transaction.clone();

    *new_transaction.expiry_height_mut() = new_expiry_height;

    let result = verifier
        .clone()
        .oneshot(Request::Block {
            transaction: Arc::new(new_transaction.clone()),
            known_utxos: Arc::new(HashMap::new()),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::CoinbaseExpiryBlockHeight {
            expiry_height: Some(new_expiry_height),
            block_height,
            transaction_hash: new_transaction.hash(),
        })
    );

    // Decrement the expiry height so that it becomes invalid.
    let new_expiry_height = (block_height - 1).expect("transaction block height is too low");
    let mut new_transaction = transaction.clone();

    *new_transaction.expiry_height_mut() = new_expiry_height;

    let result = verifier
        .clone()
        .oneshot(Request::Block {
            transaction: Arc::new(new_transaction.clone()),
            known_utxos: Arc::new(HashMap::new()),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::CoinbaseExpiryBlockHeight {
            expiry_height: Some(new_expiry_height),
            block_height,
            transaction_hash: new_transaction.hash(),
        })
    );

    // Test with matching heights again, but using a very high value
    // that is greater than the limit for non-coinbase transactions,
    // to ensure the limit is not being enforced for coinbase transactions.
    let new_expiry_height = Height::MAX;
    let mut new_transaction = transaction.clone();

    *new_transaction.expiry_height_mut() = new_expiry_height;

    let result = verifier
        .clone()
        .oneshot(Request::Block {
            transaction: Arc::new(new_transaction.clone()),
            known_utxos: Arc::new(HashMap::new()),
            height: new_expiry_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        new_transaction.unmined_id()
    );
}

/// Tests if an expired non-coinbase V5 transaction is rejected.
#[ignore = "V5 not supported in komodo"]   // Nu5 not supported in komodo
#[tokio::test]
async fn v5_transaction_with_too_low_expiry_height() {
    let state_service =
        service_fn(|_| async { unreachable!("State service should not be called") });
    let verifier = Verifier::new(Network::Testnet, state_service);

    let block_height = NetworkUpgrade::Nu5
        .activation_height(Network::Testnet)
        .expect("Nu5 activation height for testnet is specified");
    let fund_height = (block_height - 1).expect("fake source fund block height is too small");
    let (input, output, known_utxos) = mock_transparent_transfer(fund_height, true, 0);

    // This expiry height is too low so that the tx should seem expired to the verifier.
    let expiry_height = (block_height - 1).expect("original block height is too small");

    // Create a non-coinbase V5 tx.
    let transaction = Transaction::V5 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height,
        sapling_shielded_data: None,
        orchard_shielded_data: None,
        network_upgrade: NetworkUpgrade::Nu5,
    };

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(known_utxos),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::ExpiredTransaction {
            expiry_height,
            block_height,
            transaction_hash: transaction.hash(),
        })
    );
}

/// Tests if a non-coinbase V5 transaction with an expiry height exceeding the
/// maximum is rejected.
#[ignore = "V5 not supported in komodo"] // Nu5 not supported in komodo
#[tokio::test]
async fn v5_transaction_with_exceeding_expiry_height() {
    let state_service =
        service_fn(|_| async { unreachable!("State service should not be called") });
    let verifier = Verifier::new(Network::Mainnet, state_service);

    let block_height = block::Height::MAX;

    let fund_height = (block_height - 1).expect("fake source fund block height is too small");
    let (input, output, known_utxos) = mock_transparent_transfer(fund_height, true, 0);

    // This expiry height exceeds the maximum defined by the specification.
    let expiry_height = block::Height(500_000_000);

    // Create a non-coinbase V5 tx.
    let transaction = Transaction::V5 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height,
        sapling_shielded_data: None,
        orchard_shielded_data: None,
        network_upgrade: NetworkUpgrade::Nu5,
    };

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction.clone()),
            known_utxos: Arc::new(known_utxos),
            height: block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::MaximumExpiryHeight {
            expiry_height,
            is_coinbase: false,
            block_height,
            transaction_hash: transaction.hash(),
        })
    );
}

/// Test if V5 coinbase transaction is accepted.
/// Fixed for Komodo with fake_state_handler()
#[ignore]   // Nu5 not supported in komodo
#[tokio::test]
async fn v5_coinbase_transaction_is_accepted() {
    let network = Network::Testnet;
    let network_upgrade = NetworkUpgrade::Nu5;

    let nu5_activation_height = network_upgrade
        .activation_height(network)
        .expect("NU5 activation height is specified");

    let transaction_block_height =
        (nu5_activation_height + 10).expect("transaction block height is too large");

    // Create a fake transparent coinbase that should succeed
    let (input, output) = mock_coinbase_transparent_output(transaction_block_height);
    let known_utxos = HashMap::new();

    // Create a V5 coinbase transaction
    let transaction = Transaction::V5 {
        network_upgrade,
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::Height(block::Height(0)),
        expiry_height: transaction_block_height,
        sapling_shielded_data: None,
        orchard_shielded_data: None,
    };

    let transaction_hash = transaction.unmined_id();

    let state_service =
        service_fn(fake_state_handler);
    let verifier = Verifier::new(network, state_service);

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(known_utxos),
            height: transaction_block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result.expect("unexpected error response").tx_id(),
        transaction_hash
    );
}

/// Test if V5 transaction with transparent funds is rejected if the source script prevents it.
///
/// This test simulates the case where the script verifier rejects the transaction because the
/// script prevents spending the source UTXO.
/// Fixed for Komodo with fake_state_handler()
#[ignore]   // Nu5 not supported in komodo
#[tokio::test]
async fn v5_transaction_with_transparent_transfer_is_rejected_by_the_script() {
    let network = Network::Testnet;
    let network_upgrade = NetworkUpgrade::Nu5;

    let nu5_activation_height = network_upgrade
        .activation_height(network)
        .expect("NU5 activation height is specified");

    let transaction_block_height =
        (nu5_activation_height + 10).expect("transaction block height is too large");

    let fake_source_fund_height =
        (transaction_block_height - 1).expect("fake source fund block height is too small");

    // Create a fake transparent transfer that should not succeed
    let (input, output, known_utxos) = mock_transparent_transfer(fake_source_fund_height, false, 0);

    // Create a V5 transaction
    let transaction = Transaction::V5 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::Height(block::Height(0)),
        expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
        sapling_shielded_data: None,
        orchard_shielded_data: None,
        network_upgrade,
    };

    let state_service =
        service_fn(fake_state_handler);
    let verifier = Verifier::new(network, state_service);

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(known_utxos),
            height: transaction_block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(
        result,
        Err(TransactionError::InternalDowncastError(
            "downcast to known transaction error type failed, original error: ScriptInvalid"
                .to_string()
        ))
    );
}

/// Test if V5 transaction with an internal double spend of transparent funds is rejected.
#[ignore]   // Nu5 not supported in komodo
#[tokio::test]
async fn komodo_v5_transaction_with_conflicting_transparent_spend_is_rejected() {
    let network = Network::Mainnet;
    let network_upgrade = NetworkUpgrade::Nu5;

    let sapling_activation_height = NetworkUpgrade::Sapling
        .activation_height(network)
        .expect("Sapling activation height is specified");

    let transaction_block_height =
        (sapling_activation_height + 10).expect("transaction block height is too large");

    let fake_source_fund_height =
        (transaction_block_height - 1).expect("fake source fund block height is too small");

    // Create a fake transparent transfer that should succeed
    let (input, output, known_utxos) = mock_transparent_transfer(fake_source_fund_height, true, 0);

    // Create a V4 transaction
    let transaction = Transaction::V5 {
        inputs: vec![input.clone(), input.clone()],
        outputs: vec![output],
        lock_time: LockTime::Height(block::Height(0)),
        expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
        sapling_shielded_data: None,
        orchard_shielded_data: None,
        network_upgrade,
    };

    let state_service =
        service_fn(|_| async { unreachable!("State service should not be called") });
    let verifier = Verifier::new(network, state_service);

    let result = verifier
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(known_utxos),
            height: transaction_block_height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH, // unused
            last_tx_verify_data: None, // unused
        })
        .await;

    let expected_outpoint = input.outpoint().expect("Input should have an outpoint");

    assert_eq!(
        result,
        Err(TransactionError::DuplicateTransparentSpend(
            expected_outpoint
        ))
    );
}

/// Test if signed V4 transaction with a dummy [`sprout::JoinSplit`] is accepted.
///
/// This test verifies if the transaction verifier correctly accepts a signed transaction.
#[ignore]  // Not supported in Komodo due to different network upgrades and blocks
#[test]
fn v4_with_signed_sprout_transfer_is_accepted() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;

        let (height, transaction) = test_transactions(network)
            .rev()
            .filter(|(_, transaction)| {
                !transaction.is_coinbase() && transaction.inputs().is_empty()
            })
            .find(|(_, transaction)| transaction.sprout_groth16_joinsplits().next().is_some())
            .expect("No transaction found with Groth16 JoinSplits");

        let expected_hash = transaction.unmined_id();

        // Initialize the verifier
        let state_service =
            service_fn(|_| async { unreachable!("State service should not be called") });
        let verifier = Verifier::new(network, state_service);

        // Test the transaction verifier
        let result = verifier
            .clone()
            .oneshot(Request::Block {
                transaction,
                known_utxos: Arc::new(HashMap::new()),
                height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH,
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result.expect("unexpected error response").tx_id(),
            expected_hash
        );
    })
}

/// Test if an V4 transaction with a modified [`sprout::JoinSplit`] is rejected.
///
/// This test verifies if the transaction verifier correctly rejects the transaction because of the
/// invalid JoinSplit.
#[ignore]  // Not supported in Komodo due to different network upgrades and blocks
#[test]
fn v4_with_modified_joinsplit_is_rejected() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        v4_with_joinsplit_is_rejected_for_modification(
            JoinSplitModification::CorruptSignature,
            // TODO: Fix error downcast
            // Err(TransactionError::Ed25519(ed25519::Error::InvalidSignature))
            TransactionError::InternalDowncastError(
                "downcast to known transaction error type failed, original error: InvalidSignature"
                    .to_string(),
            ),
        )
        .await;

        v4_with_joinsplit_is_rejected_for_modification(
            JoinSplitModification::CorruptProof,
            TransactionError::Groth16("proof verification failed".to_string()),
        )
        .await;

        v4_with_joinsplit_is_rejected_for_modification(
            JoinSplitModification::ZeroProof,
            TransactionError::MalformedGroth16("invalid G1".to_string()),
        )
        .await;
    })
}

async fn v4_with_joinsplit_is_rejected_for_modification(
    modification: JoinSplitModification,
    expected_error: TransactionError,
) {
    let network = Network::Mainnet;

    let (height, mut transaction) = test_transactions(network)
        .rev()
        .filter(|(_, transaction)| !transaction.is_coinbase() && transaction.inputs().is_empty())
        .find(|(_, transaction)| transaction.sprout_groth16_joinsplits().next().is_some())
        .expect("No transaction found with Groth16 JoinSplits");

    modify_joinsplit(
        Arc::get_mut(&mut transaction).expect("Transaction only has one active reference"),
        modification,
    );

    // Initialize the verifier
    let state_service =
        service_fn(/*|_| async { unreachable!("State service should not be called") }*/ fake_state_handler);
    let verifier = Verifier::new(network, state_service);

    // Test the transaction verifier
    let result = verifier
        .clone()
        .oneshot(Request::Block {
            transaction: transaction.clone(),
            known_utxos: Arc::new(HashMap::new()),
            height,
            time: chrono::MAX_DATETIME,
            previous_hash: FAKE_PREV_BLOCK_HASH,
            last_tx_verify_data: None, // unused
        })
        .await;

    assert_eq!(result, Err(expected_error));
}

/// Test if a V4 transaction with Sapling spends is accepted by the verifier.
#[ignore]  // Not supported in Komodo due to different network upgrades and blocks
#[test]
fn v4_with_sapling_spends() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;

        let (height, transaction) = test_transactions(network)
            .rev()
            .filter(|(_, transaction)| {
                !transaction.is_coinbase() && transaction.inputs().is_empty()
            })
            .find(|(_, transaction)| transaction.sapling_spends_per_anchor().next().is_some())
            .expect("No transaction found with Sapling spends");

        let expected_hash = transaction.unmined_id();

        // Initialize the verifier
        let state_service =
            service_fn(|_| async { unreachable!("State service should not be called") });
        let verifier = Verifier::new(network, state_service);

        // Test the transaction verifier
        let result = verifier
            .clone()
            .oneshot(Request::Block {
                transaction,
                known_utxos: Arc::new(HashMap::new()),
                height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result.expect("unexpected error response").tx_id(),
            expected_hash
        );
    });
}

/// Test if a V4 transaction with a duplicate Sapling spend is rejected by the verifier.
#[test]
fn v4_with_duplicate_sapling_spends() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;

        let (height, mut transaction) = test_transactions(network)
            .rev()
            .filter(|(_, transaction)| {
                !transaction.is_coinbase() && transaction.inputs().is_empty()
            })
            .find(|(_, transaction)| transaction.sapling_spends_per_anchor().next().is_some())
            .expect("No transaction found with Sapling spends");

        // Duplicate one of the spends
        let duplicate_nullifier = duplicate_sapling_spend(
            Arc::get_mut(&mut transaction).expect("Transaction only has one active reference"),
        );

        // Initialize the verifier
        let state_service =
            service_fn(|_| async { unreachable!("State service should not be called") });
        let verifier = Verifier::new(network, state_service);

        // Test the transaction verifier
        let result = verifier
            .clone()
            .oneshot(Request::Block {
                transaction,
                known_utxos: Arc::new(HashMap::new()),
                height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result,
            Err(TransactionError::DuplicateSaplingNullifier(
                duplicate_nullifier
            ))
        );
    });
}

/// Test if a V4 transaction with Sapling outputs but no spends is accepted by the verifier.
#[ignore]  // Not supported in Komodo due to different network upgrades and blocks
#[test]
fn v4_with_sapling_outputs_and_no_spends() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;

        let (height, transaction) = test_transactions(network)
            .rev()
            .filter(|(_, transaction)| {
                !transaction.is_coinbase() && transaction.inputs().is_empty()
            })
            .find(|(_, transaction)| {
                transaction.sapling_spends_per_anchor().next().is_none()
                    && transaction.sapling_outputs().next().is_some()
            })
            .expect("No transaction found with Sapling outputs and no Sapling spends");

        let expected_hash = transaction.unmined_id();

        // Initialize the verifier
        let state_service =
            service_fn(/*|_| async { unreachable!("State service should not be called") }*/ fake_state_handler);
        let verifier = Verifier::new(network, state_service);

        // Test the transaction verifier
        let result = verifier
            .clone()
            .oneshot(Request::Block {
                transaction,
                known_utxos: Arc::new(HashMap::new()),
                height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH,
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result.expect("unexpected error response").tx_id(),
            expected_hash
        );
    })
}

/// Test if a V5 transaction with Sapling spends is accepted by the verifier.
#[test]
// TODO: add NU5 mainnet test vectors with Sapling spends, then remove should_panic
#[should_panic]
fn v5_with_sapling_spends() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;
        let nu5_activation = NetworkUpgrade::Nu5.activation_height(network);

        let transaction =
            fake_v5_transactions_for_network(network, zebra_test::vectors::MAINNET_BLOCKS.iter())
                .rev()
                .filter(|transaction| {
                    !transaction.is_coinbase()
                        && transaction.inputs().is_empty()
                        && transaction.expiry_height() >= nu5_activation
                })
                .find(|transaction| transaction.sapling_spends_per_anchor().next().is_some())
                .expect("No transaction found with Sapling spends");

        let expected_hash = transaction.unmined_id();
        let height = transaction
            .expiry_height()
            .expect("Transaction is missing expiry height");

        // Initialize the verifier
        let state_service =
            service_fn(|_| async { unreachable!("State service should not be called") });
        let verifier = Verifier::new(network, state_service);

        // Test the transaction verifier
        let result = verifier
            .clone()
            .oneshot(Request::Block {
                transaction: Arc::new(transaction),
                known_utxos: Arc::new(HashMap::new()),
                height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result.expect("unexpected error response").tx_id(),
            expected_hash
        );
    });
}

/// Test if a V5 transaction with a duplicate Sapling spend is rejected by the verifier.
#[ignore]  // Not supported in Komodo due to different network upgrades and blocks 
#[test]
fn v5_with_duplicate_sapling_spends() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;

        let mut transaction =
            fake_v5_transactions_for_network(network, zebra_test::vectors::MAINNET_BLOCKS.iter())
                .rev()
                .filter(|transaction| !transaction.is_coinbase() && transaction.inputs().is_empty())
                .find(|transaction| transaction.sapling_spends_per_anchor().next().is_some())
                .expect("No transaction found with Sapling spends");

        let height = transaction
            .expiry_height()
            .expect("Transaction is missing expiry height");

        // Duplicate one of the spends
        let duplicate_nullifier = duplicate_sapling_spend(&mut transaction);

        // Initialize the verifier
        let state_service =
            service_fn(|_| async { unreachable!("State service should not be called") });
        let verifier = Verifier::new(network, state_service);

        // Test the transaction verifier
        let result = verifier
            .clone()
            .oneshot(Request::Block {
                transaction: Arc::new(transaction),
                known_utxos: Arc::new(HashMap::new()),
                height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result,
            Err(TransactionError::DuplicateSaplingNullifier(
                duplicate_nullifier
            ))
        );
    });
}

/// Test if a V5 transaction with a duplicate Orchard action is rejected by the verifier.
#[ignore] // Not supported in Komodo due to different network upgrades and blocks
#[test]
fn v5_with_duplicate_orchard_action() {
    zebra_test::init();
    zebra_test::MULTI_THREADED_RUNTIME.block_on(async {
        let network = Network::Mainnet;

        // Find a transaction with no inputs or outputs to use as base
        let mut transaction =
            fake_v5_transactions_for_network(network, zebra_test::vectors::MAINNET_BLOCKS.iter())
                .rev()
                .find(|transaction| {
                    transaction.inputs().is_empty()
                        && transaction.outputs().is_empty()
                        && transaction.sapling_spends_per_anchor().next().is_none()
                        && transaction.sapling_outputs().next().is_none()
                        && transaction.joinsplit_count() == 0
                })
                .expect("At least one fake V5 transaction with no inputs and no outputs");

        let height = transaction
            .expiry_height()
            .expect("Transaction is missing expiry height");

        // Insert fake Orchard shielded data to the transaction, which has at least one action (this is
        // guaranteed structurally by `orchard::ShieldedData`)
        let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);

        // Enable spends
        shielded_data.flags = zebra_chain::orchard::Flags::ENABLE_SPENDS
            | zebra_chain::orchard::Flags::ENABLE_OUTPUTS;

        // Duplicate the first action
        let duplicate_action = shielded_data.actions.first().clone();
        let duplicate_nullifier = duplicate_action.action.nullifier;

        shielded_data.actions.push(duplicate_action);

        // Initialize the verifier
        let state_service =
            service_fn(|_| async { unreachable!("State service should not be called") });
        let verifier = Verifier::new(network, state_service);

        // Test the transaction verifier
        let result = verifier
            .clone()
            .oneshot(Request::Block {
                transaction: Arc::new(transaction),
                known_utxos: Arc::new(HashMap::new()),
                height,
                time: chrono::MAX_DATETIME,
                previous_hash: FAKE_PREV_BLOCK_HASH, // unused
                last_tx_verify_data: None, // unused
            })
            .await;

        assert_eq!(
            result,
            Err(TransactionError::DuplicateOrchardNullifier(
                duplicate_nullifier
            ))
        );
    });
}

// Utility functions

/// Create a mock transparent transfer to be included in a transaction.
///
/// First, this creates a fake unspent transaction output from a fake transaction included in the
/// specified `previous_utxo_height` block height. This fake [`Utxo`] also contains a simple script
/// that can either accept or reject any spend attempt, depending on if `script_should_succeed` is
/// `true` or `false`. Since the `tx_index_in_block` is irrelevant for blocks that have already
/// been verified, it is set to `1`.
///
/// Then, a [`transparent::Input::PrevOut`] is created that attempts to spend the previously created fake
/// UTXO to a new [`transparent::Output`].
///
/// Finally, the initial fake UTXO is placed in a `known_utxos` [`HashMap`] so that it can be
/// retrieved during verification.
///
/// The function then returns the generated transparent input and output, as well as the
/// `known_utxos` map.
///
/// Note: `known_utxos` is only intended to be used for UTXOs within the same block,
/// so future verification changes might break this mocking function.
fn mock_transparent_transfer(
    previous_utxo_height: block::Height,
    script_should_succeed: bool,
    outpoint_index: u32,
) -> (
    transparent::Input,
    transparent::Output,
    HashMap<transparent::OutPoint, transparent::OrderedUtxo>,
) {
    // A script with a single opcode that accepts the transaction (pushes true on the stack)
    let accepting_script = transparent::Script::new(&[1, 1]);
    // A script with a single opcode that rejects the transaction (OP_FALSE)
    let rejecting_script = transparent::Script::new(&[0]);

    // Mock an unspent transaction output
    let previous_outpoint = transparent::OutPoint {
        hash: Hash([1u8; 32]),
        index: outpoint_index,
    };

    let lock_script = if script_should_succeed {
        accepting_script.clone()
    } else {
        rejecting_script.clone()
    };

    let previous_output = transparent::Output {
        value: Amount::try_from(1).expect("1 is an invalid amount"),
        lock_script,
    };

    let previous_utxo = transparent::OrderedUtxo::new(previous_output, previous_utxo_height, 1, LockTime::unlocked());

    // Use the `previous_outpoint` as input
    let input = transparent::Input::PrevOut {
        outpoint: previous_outpoint,
        unlock_script: accepting_script,
        sequence: 0,
    };

    // The output resulting from the transfer
    // Using the rejecting script pretends the amount is burned because it can't be spent again
    let output = transparent::Output {
        value: Amount::try_from(1).expect("1 is an invalid amount"),
        lock_script: rejecting_script,
    };

    // Cache the source of the fund so that it can be used during verification
    let mut known_utxos = HashMap::new();
    known_utxos.insert(previous_outpoint, previous_utxo);

    (input, output, known_utxos)
}

/// Create a mock coinbase input with a transparent output.
///
/// Create a [`transparent::Input::Coinbase`] at `coinbase_height`.
/// Then create UTXO with a [`transparent::Output`] spending some coinbase funds.
///
/// Returns the generated coinbase input and transparent output.
fn mock_coinbase_transparent_output(
    coinbase_height: block::Height,
) -> (transparent::Input, transparent::Output) {
    // A script with a single opcode that rejects the transaction (OP_FALSE)
    let rejecting_script = transparent::Script::new(&[0]);

    let input = transparent::Input::Coinbase {
        height: coinbase_height,
        data: CoinbaseData::new(Vec::new()),
        sequence: u32::MAX,
    };

    // The output resulting from the transfer
    // Using the rejecting script pretends the amount is burned because it can't be spent again
    let output = transparent::Output {
        value: Amount::try_from(1).expect("1 is an invalid amount"),
        lock_script: rejecting_script,
    };

    (input, output)
}

/// Create a mock [`sprout::JoinSplit`] and include it in a [`transaction::JoinSplitData`].
///
/// This creates a dummy join split. By itself it is invalid, but it is useful for including in a
/// transaction to check the signatures.
///
/// The [`transaction::JoinSplitData`] with the dummy [`sprout::JoinSplit`] is returned together
/// with the [`ed25519::SigningKey`] that can be used to create a signature to later add to the
/// returned join split data.
fn mock_sprout_join_split_data() -> (JoinSplitData<Groth16Proof>, ed25519::SigningKey) {
    // Prepare dummy inputs for the join split
    let zero_amount = 0_i32
        .try_into()
        .expect("Invalid JoinSplit transparent input");
    let anchor = sprout::tree::Root::default();
    let first_nullifier = sprout::note::Nullifier([0u8; 32]);
    let second_nullifier = sprout::note::Nullifier([1u8; 32]);
    let commitment = sprout::commitment::NoteCommitment::from([0u8; 32]);
    let ephemeral_key =
        x25519::PublicKey::from(&x25519::EphemeralSecret::new(rand07::thread_rng()));
    let random_seed = sprout::RandomSeed::from([0u8; 32]);
    let mac = sprout::note::Mac::zcash_deserialize(&[0u8; 32][..])
        .expect("Failure to deserialize dummy MAC");
    let zkproof = Groth16Proof([0u8; 192]);
    let encrypted_note = sprout::note::EncryptedNote([0u8; 601]);

    // Create an dummy join split
    let joinsplit = sprout::JoinSplit {
        vpub_old: zero_amount,
        vpub_new: zero_amount,
        anchor,
        nullifiers: [first_nullifier, second_nullifier],
        commitments: [commitment; 2],
        ephemeral_key,
        random_seed,
        vmacs: [mac.clone(), mac],
        zkproof,
        enc_ciphertexts: [encrypted_note; 2],
    };

    // Create a usable signing key
    let signing_key = ed25519::SigningKey::new(rand::thread_rng());
    let verification_key = ed25519::VerificationKey::from(&signing_key);

    // Populate join split data with the dummy join split.
    let joinsplit_data = JoinSplitData {
        first: joinsplit,
        rest: vec![],
        pub_key: verification_key.into(),
        sig: [0u8; 64].into(),
    };

    (joinsplit_data, signing_key)
}

/// A type of JoinSplit modification to test.
#[derive(Clone, Copy)]
enum JoinSplitModification {
    // Corrupt a signature, making it invalid.
    CorruptSignature,
    // Corrupt a proof, making it invalid, but still well-formed.
    CorruptProof,
    // Make a proof all-zeroes, making it malformed.
    ZeroProof,
}

/// Modify a JoinSplit in the transaction following the given modification type.
fn modify_joinsplit(transaction: &mut Transaction, modification: JoinSplitModification) {
    match transaction {
        Transaction::V4 {
            joinsplit_data: Some(ref mut joinsplit_data),
            ..
        } => modify_joinsplit_data(joinsplit_data, modification),
        _ => unreachable!("Transaction has no JoinSplit shielded data"),
    }
}

/// Modify a [`JoinSplitData`] following the given modification type.
fn modify_joinsplit_data(
    joinsplit_data: &mut JoinSplitData<Groth16Proof>,
    modification: JoinSplitModification,
) {
    match modification {
        JoinSplitModification::CorruptSignature => {
            let mut sig_bytes: [u8; 64] = joinsplit_data.sig.into();
            // Flip a bit from an arbitrary byte of the signature.
            sig_bytes[10] ^= 0x01;
            joinsplit_data.sig = sig_bytes.into();
        }
        JoinSplitModification::CorruptProof => {
            let joinsplit = joinsplit_data
                .joinsplits_mut()
                .next()
                .expect("must have a JoinSplit");
            {
                // A proof is composed of three field elements, the first and last having 48 bytes.
                // (The middle one has 96 bytes.) To corrupt the proof without making it malformed,
                // simply swap those first and last elements.
                let (first, rest) = joinsplit.zkproof.0.split_at_mut(48);
                first.swap_with_slice(&mut rest[96..144]);
            }
        }
        JoinSplitModification::ZeroProof => {
            let joinsplit = joinsplit_data
                .joinsplits_mut()
                .next()
                .expect("must have a JoinSplit");
            joinsplit.zkproof.0 = [0; 192];
        }
    }
}

/// Duplicate a Sapling spend inside a `transaction`.
///
/// Returns the nullifier of the duplicate spend.
///
/// # Panics
///
/// Will panic if the `transaction` does not have Sapling spends.
fn duplicate_sapling_spend(transaction: &mut Transaction) -> sapling::Nullifier {
    match transaction {
        Transaction::V4 {
            sapling_shielded_data: Some(ref mut shielded_data),
            ..
        } => duplicate_sapling_spend_in_shielded_data(shielded_data),
        Transaction::V5 {
            sapling_shielded_data: Some(ref mut shielded_data),
            ..
        } => duplicate_sapling_spend_in_shielded_data(shielded_data),
        _ => unreachable!("Transaction has no Sapling shielded data"),
    }
}

/// Duplicates the first spend of the `shielded_data`.
///
/// Returns the nullifier of the duplicate spend.
///
/// # Panics
///
/// Will panic if `shielded_data` has no spends.
fn duplicate_sapling_spend_in_shielded_data<A: sapling::AnchorVariant + Clone>(
    shielded_data: &mut sapling::ShieldedData<A>,
) -> sapling::Nullifier {
    match shielded_data.transfers {
        sapling::TransferData::SpendsAndMaybeOutputs { ref mut spends, .. } => {
            let duplicate_spend = spends.first().clone();
            let duplicate_nullifier = duplicate_spend.nullifier;

            spends.push(duplicate_spend);

            duplicate_nullifier
        }
        sapling::TransferData::JustOutputs { .. } => {
            unreachable!("Sapling shielded data has no spends")
        }
    }
}

#[test]
fn add_to_sprout_pool_after_nu() {
    zebra_test::init();

    // get a block that we know it haves a transaction with `vpub_old` field greater than 0.
    let block: Arc<_> = zebra_chain::block::Block::zcash_deserialize(
        &zebra_test::vectors::BLOCK_MAINNET_419199_BYTES[..],
    )
    .unwrap()
    .into();

    // create a block height at canopy activation.
    let network = Network::Mainnet;
    let block_height = NetworkUpgrade::Canopy.activation_height(network).unwrap();

    // create a zero amount.
    let zero = Amount::<NonNegative>::try_from(0).expect("an amount of 0 is always valid");

    // the coinbase transaction should pass the check.
    assert_eq!(
        check::disabled_add_to_sprout_pool(&block.transactions[0], block_height, network),
        Ok(())
    );

    // the 2nd transaction has no joinsplits, should pass the check.
    assert_eq!(block.transactions[1].joinsplit_count(), 0);
    assert_eq!(
        check::disabled_add_to_sprout_pool(&block.transactions[1], block_height, network),
        Ok(())
    );

    // the 5th transaction has joinsplits and the `vpub_old` cumulative is greater than 0,
    // should fail the check.
    assert!(block.transactions[4].joinsplit_count() > 0);
    let vpub_old: Amount<NonNegative> = block.transactions[4]
        .output_values_to_sprout()
        .fold(zero, |acc, &x| (acc + x).unwrap());
    assert!(vpub_old > zero);

    assert_eq!(
        check::disabled_add_to_sprout_pool(&block.transactions[3], block_height, network),
        Err(TransactionError::DisabledAddToSproutPool)
    );

    // the 8th transaction has joinsplits and the `vpub_old` cumulative is 0,
    // should pass the check.
    assert!(block.transactions[7].joinsplit_count() > 0);
    let vpub_old: Amount<NonNegative> = block.transactions[7]
        .output_values_to_sprout()
        .fold(zero, |acc, &x| (acc + x).unwrap());
    assert_eq!(vpub_old, zero);

    assert_eq!(
        check::disabled_add_to_sprout_pool(&block.transactions[7], block_height, network),
        Ok(())
    );
}

#[ignore]  // Not supported in Komodo due to different network upgrades and blocks
#[test]
fn coinbase_outputs_are_decryptable_for_historical_blocks() -> Result<(), Report> {
    zebra_test::init();

    coinbase_outputs_are_decryptable_for_historical_blocks_for_network(Network::Mainnet)?;
    coinbase_outputs_are_decryptable_for_historical_blocks_for_network(Network::Testnet)?;

    Ok(())
}

fn coinbase_outputs_are_decryptable_for_historical_blocks_for_network(
    network: Network,
) -> Result<(), Report> {
    let block_iter = match network {
        Network::Mainnet => zebra_test::vectors::MAINNET_BLOCKS.iter(),
        Network::Testnet => zebra_test::vectors::TESTNET_BLOCKS.iter(),
    };

    let mut tested_coinbase_txs = 0;
    let mut tested_non_coinbase_txs = 0;

    for (height, block) in block_iter {
        let block = block
            .zcash_deserialize_into::<Block>()
            .expect("block is structurally valid");
        let height = Height(*height);
        let heartwood_onward = height
            >= NetworkUpgrade::Heartwood
                .activation_height(network)
                .unwrap();
        let coinbase_tx = block
            .transactions
            .get(0)
            .expect("must have coinbase transaction");

        // Check if the coinbase outputs are decryptable with an all-zero key.
        if heartwood_onward
            && (coinbase_tx.sapling_outputs().count() > 0
                || coinbase_tx.orchard_actions().count() > 0)
        {
            // We are only truly decrypting something if it's Heartwood-onward
            // and there are relevant outputs.
            tested_coinbase_txs += 1;
        }
        check::coinbase_outputs_are_decryptable(coinbase_tx, network, height)
            .expect("coinbase outputs must be decryptable with an all-zero key");

        // For remaining transactions, check if existing outputs are NOT decryptable
        // with an all-zero key, if applicable.
        for tx in block.transactions.iter().skip(1) {
            let has_outputs = tx.sapling_outputs().count() > 0 || tx.orchard_actions().count() > 0;
            if has_outputs && heartwood_onward {
                tested_non_coinbase_txs += 1;
                check::coinbase_outputs_are_decryptable(tx, network, height).expect_err(
                    "decrypting a non-coinbase output with an all-zero key should fail",
                );
            } else {
                check::coinbase_outputs_are_decryptable(tx, network, height)
                    .expect("a transaction without outputs, or pre-Heartwood, must be considered 'decryptable'");
            }
        }
    }

    assert!(tested_coinbase_txs > 0, "ensure it was actually tested");
    assert!(tested_non_coinbase_txs > 0, "ensure it was actually tested");

    Ok(())
}

/// Given an Orchard action as a base, fill fields related to note encryption
/// from the given test vector and returned the modified action.
fn fill_action_with_note_encryption_test_vector(
    action: &zebra_chain::orchard::Action,
    v: &zebra_test::vectors::TestVector,
) -> zebra_chain::orchard::Action {
    let mut action = action.clone();
    action.cv = v.cv_net.try_into().expect("test vector must be valid");
    action.cm_x = pallas::Base::from_repr(v.cmx).unwrap();
    action.nullifier = v.rho.try_into().expect("test vector must be valid");
    action.ephemeral_key = v
        .ephemeral_key
        .try_into()
        .expect("test vector must be valid");
    action.out_ciphertext = v.c_out.into();
    action.enc_ciphertext = v.c_enc.into();
    action
}

/// Test if shielded coinbase outputs are decryptable with an all-zero outgoing
/// viewing key.
#[test]
fn coinbase_outputs_are_decryptable_for_fake_v5_blocks() {
    let network = Network::Testnet;

    for v in zebra_test::vectors::ORCHARD_NOTE_ENCRYPTION_ZERO_VECTOR.iter() {
        // Find a transaction with no inputs or outputs to use as base
        let mut transaction =
            fake_v5_transactions_for_network(network, zebra_test::vectors::TESTNET_BLOCKS.iter())
                .rev()
                .find(|transaction| {
                    transaction.inputs().is_empty()
                        && transaction.outputs().is_empty()
                        && transaction.sapling_spends_per_anchor().next().is_none()
                        && transaction.sapling_outputs().next().is_none()
                        && transaction.joinsplit_count() == 0
                })
                .expect("At least one fake V5 transaction with no inputs and no outputs");

        let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);
        shielded_data.flags = zebra_chain::orchard::Flags::ENABLE_SPENDS
            | zebra_chain::orchard::Flags::ENABLE_OUTPUTS;

        let action =
            fill_action_with_note_encryption_test_vector(&shielded_data.actions[0].action, v);
        let sig = shielded_data.actions[0].spend_auth_sig;
        shielded_data.actions = vec![AuthorizedAction::from_parts(action, sig)]
            .try_into()
            .unwrap();

        assert_eq!(
            check::coinbase_outputs_are_decryptable(
                &transaction,
                network,
                NetworkUpgrade::Nu5.activation_height(network).unwrap(),
            ),
            Ok(())
        );
    }
}

/// Test if random shielded outputs are NOT decryptable with an all-zero outgoing
/// viewing key.
#[test]
fn shielded_outputs_are_not_decryptable_for_fake_v5_blocks() {
    let network = Network::Testnet;

    for v in zebra_test::vectors::ORCHARD_NOTE_ENCRYPTION_VECTOR.iter() {
        // Find a transaction with no inputs or outputs to use as base
        let mut transaction =
            fake_v5_transactions_for_network(network, zebra_test::vectors::TESTNET_BLOCKS.iter())
                .rev()
                .find(|transaction| {
                    transaction.inputs().is_empty()
                        && transaction.outputs().is_empty()
                        && transaction.sapling_spends_per_anchor().next().is_none()
                        && transaction.sapling_outputs().next().is_none()
                        && transaction.joinsplit_count() == 0
                })
                .expect("At least one fake V5 transaction with no inputs and no outputs");

        let shielded_data = insert_fake_orchard_shielded_data(&mut transaction);
        shielded_data.flags = zebra_chain::orchard::Flags::ENABLE_SPENDS
            | zebra_chain::orchard::Flags::ENABLE_OUTPUTS;

        let action =
            fill_action_with_note_encryption_test_vector(&shielded_data.actions[0].action, v);
        let sig = shielded_data.actions[0].spend_auth_sig;
        shielded_data.actions = vec![AuthorizedAction::from_parts(action, sig)]
            .try_into()
            .unwrap();

        assert_eq!(
            check::coinbase_outputs_are_decryptable(
                &transaction,
                network,
                NetworkUpgrade::Nu5.activation_height(network).unwrap(),
            ),
            Err(TransactionError::CoinbaseOutputsNotDecryptable)
        );
    }
}



#[test]
/// These tests should be fully equal to https://github.com/DeckerSU/KomodoOcean/blob/patch-test-isfinaltx/src/test-komodo/test_isfinaltx.cpp .
fn is_final_tx_komodo_tests() {

    let network = Network::Mainnet;


    // let sapling_activation_height = Height(1140409);
    let sapling_activation_height = NetworkUpgrade::Sapling
        .activation_height(network)
        .expect("Sapling activation height is specified");

    let sapling_block_time = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(1544835390, 0), Utc);

    // TODO: may be best way will be to somehow call NN::NNDataMain::new() and extract needed data from it, but for now we will use constants
    let n_staked_december_hardfork_timestamp = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(1576840000, 0), Utc); //December 2019 hardfork 12/20/2019 @ 11:06am (UTC)
    let n_december_hardfork_height = Height(1670000); //December 2019 hardfork
    let fake_source_fund_height = sapling_activation_height;

    let tbh = n_december_hardfork_height;
    let tbt = n_staked_december_hardfork_timestamp;

    // closure implements BuildTransactionTemplate https://github.com/DeckerSU/KomodoOcean/blob/patch-test-isfinaltx/src/test-komodo/test_isfinaltx.cpp#L82
    let build_transaction_template = | lock_time: LockTime, count_final: u8, count_non_final: u8, default_seq: u32 | -> Transaction {

        let mut fake_inputs: Vec<Input> = vec![];
        let mut fake_outputs: Vec<Output> = vec![];

        let mut vin_number = 0;

        // adding "final" vins
        for _n in 1..=count_final {
            vin_number = vin_number + 1;
            let prev_txid = [vin_number; 32];
            let new_outpoint = transparent::OutPoint {
                hash: Hash(prev_txid),
                index: vin_number as u32,
            };
            let (mut input, _, _) = mock_transparent_transfer(fake_source_fund_height, true, 0);
            input.set_outpoint(new_outpoint);
            input.set_sequence(u32::MAX);
            fake_inputs.push(input);
        }

        // adding "non-final" vins
        for _n in 1..=count_non_final {
            vin_number = vin_number + 1;
            let prev_txid = [vin_number; 32];
            let new_outpoint = transparent::OutPoint {
                hash: Hash(prev_txid),
                index: vin_number as u32,
            };
            let (mut input, _, _) = mock_transparent_transfer(fake_source_fund_height, true, 0);
            input.set_outpoint(new_outpoint);

            // make sure that passed default_seq is "non-final", if "final" is passed - make it "non-final"
            let seq = match default_seq {
                u32::MAX => u32::MAX - 1,
                _ => default_seq
            };

            input.set_sequence(seq);
            fake_inputs.push(input);
        }

        let (_, output, _) = mock_transparent_transfer(fake_source_fund_height, true, 0);
        fake_outputs.push(output);

        let tx = Transaction::V4 {
            inputs: fake_inputs,
            outputs: fake_outputs,
            lock_time,
            expiry_height: (tbh + 1).expect("expiry height is too large"),
            joinsplit_data: None,
            sapling_shielded_data: None,
        };

        tx
    };

    // println!("{:?}", tbh);
    // println!("{:?}", build_transaction_template(LockTime::Height(zebra_chain::block::Height(0)), 1, 2, 777));

    /* common cases, when nLockTime = 0 or nLockTime < nBlockHeight | nBlockTime */
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height(zebra_chain::block::Height(0)), 1, 1, 0), tbh, tbt), Ok(()));
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height((tbh - 1).expect("height is ok")), 1, 1, 0), tbh, tbt), Ok(()));
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt - Duration::seconds(1)), 1, 1, 0), tbh, tbt), Ok(()));

    /* first we will do the test for before December 2019 hardfork values */

    /* before hardfork tx with vin with nSequence == 0xfffffffe treated as final if
        nLockTime > (nBlockTime | nBlockHeight), such vins considered same way as vins with
        Sequence == 0xffffffff. all other sequences in vins should be considered same way as in bitcoin,
        if vin have "non-final" sequence and nLockTime >= (nBlockTime | nBlockHeight) it should be
        considered as non-final.
    */

    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height((tbh + 1).expect("height is ok")), 1, 1, u32::MAX - 1), tbh, tbt), Ok(()));
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt + Duration::seconds(1)), 1, 1, u32::MAX - 1), tbh, tbt), Ok(()));

    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height(tbh), 1, 1, u32::MAX - 1), tbh, tbt), Err(TransactionError::LockedUntilAfterBlockHeight(tbh)));
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt),   1, 1, u32::MAX - 1), tbh, tbt), Err(TransactionError::LockedUntilAfterBlockTime(tbt)));

    // https://stackoverflow.com/questions/51121446/how-do-i-assert-an-enum-is-a-specific-variant-if-i-dont-care-about-its-fields
    assert!(matches!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height((tbh + 1).expect("height is ok")), 1, 1, 777), tbh, tbt), Err(_)));
    assert!(matches!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt + Duration::seconds(1))        , 1, 1, 777), tbh, tbt), Err(_)));

    // all vins have SEQUENCE_FINAL, so it's final
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt + Duration::seconds(1)), 1, 0, u32::MAX - 1), tbh, tbt), Ok(()));

    /* after let's "jump" into hardfork times, we will increase tbh and tbt to match HF times */
    let tbh = (tbh + 2).expect("height is ok");
    let tbt = tbt + Duration::seconds(1);
    // in below tests in komodod we have chainActive.Height() = 1670001, inside komodo_hardfork_active, so HF assumed active,
    // here to get the same effect we should increase tbh by 2.

    /* after hardfork we consider nSequence == 0xfffffffe as final if nLockTime <= (nBlockTime | nBlockHeight) */

    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height((tbh - 1).expect("height is ok")), 1, 1, u32::MAX - 1), tbh, tbt), Ok(()));
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt - Duration::seconds(1)),         1, 1, u32::MAX - 1), tbh, tbt), Ok(()));

    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height(tbh), 1, 1, u32::MAX - 1), tbh, tbt), Ok(()));
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt),   1, 1, u32::MAX - 1), tbh, tbt), Ok(()));

    assert!(matches!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height((tbh + 1).expect("height is ok")), 1, 1, u32::MAX - 1), tbh, tbt), Err(_)));
    assert!(matches!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt + Duration::seconds(1))        , 1, 1, u32::MAX - 1), tbh, tbt), Err(_)));

    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height((tbh - 1).expect("height is ok")), 1, 1, 777), tbh, tbt), Ok(()));
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt - Duration::seconds(1)),         1, 1, 777), tbh, tbt), Ok(()));
    assert!(matches!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height(tbh), 1, 1, 777), tbh, tbt), Err(_)));
    assert!(matches!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt)  , 1, 1, 777), tbh, tbt), Err(_)));
    assert!(matches!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Height((tbh + 1).expect("height is ok")), 1, 1, 777), tbh, tbt), Err(_)));
    assert!(matches!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt + Duration::seconds(1))        , 1, 1, 777), tbh, tbt), Err(_)));

    // all vins have SEQUENCE_FINAL, so it's final
    assert_eq!(check::is_final_tx_komodo(network, &build_transaction_template(LockTime::Time(tbt + Duration::seconds(1)), 1, 0, u32::MAX - 1), tbh, tbt), Ok(()));
}


/// Check the predefined set of transactions containing (or not) banned inputs using [`check::tx_has_banned_inputs`].
#[test]
fn tx_has_banned_inputs_multiple_tests() {
    let network = Network::Mainnet;

    let sapling_activation_height = NetworkUpgrade::Sapling
        .activation_height(network)
        .expect("Sapling activation height is specified");

    let transaction_block_height =
        (sapling_activation_height + 10).expect("transaction block height is too large");

    let fake_source_fund_height =
        (transaction_block_height - 1).expect("fake source fund block height is too small");

    let test_cases: [(&str, u32, Result<(), TransactionError>); 5] = [
        ("c85dcffb16d5a45bd239021ad33443414d60224760f11d535ae2063e5709efee", 1, Err(TransactionError::BannedInputs)),
        ("bbd3a3d9b14730991e1066bd7c626ca270acac4127131afe25f877a5a886eb25", 1, Err(TransactionError::BannedInputs)),
        ("c4ea1462c207547cd6fb6a4155ca6d042b22170d29801a465db5c09fec55b19d", 333, Err(TransactionError::BannedInputs)),
        ("c85dcffb16d5a45bd239021ad33443414d60224760f11d535ae2063e5709efee", 0, Ok(())),
        ("0101010101010101010101010101010101010101010101010101010101010101", 0, Ok(())),
    ];

    for (transaction_id, n, result) in test_cases {
        let txid: Hash = transaction_id.parse().expect("txid should be correct");
        let new_outpoint = transparent::OutPoint {
            hash: txid,
            index: n,
        };

        let (mut input, output, _) = mock_transparent_transfer(fake_source_fund_height, true, 0);
        input.set_outpoint(new_outpoint);

        let tx = Transaction::V4 {
            inputs: vec![input],
            outputs: vec![output],
            lock_time: LockTime::Height(block::Height(0)),
            expiry_height: (transaction_block_height + 1).expect("expiry height is too large"),
            joinsplit_data: None,
            sapling_shielded_data: None,
        };

        assert_eq!(check::tx_has_banned_inputs(&tx), result);
    };

}

/// Block #3263485, https://kmdexplorer.io/block/0d57540a4e2562420b7e6eef7d3b52d486e58e12d6fe115f3ddbc751b30d90f4
/// OP_RETURN calculation check
#[test]
fn merkle_opret_calculation() {

    let hashes = [
        "0db4c219be942611da29836724bd149f5bfd38a34ff7e54bee50b88a348bd486",
        "baad150093bd566ac323af985215fc84dff1fcbfc52c0398a7c7f5af1943ef8b",
    ];

    let calculated_root = hashes.into_iter().map(|hash| hash.parse::<Hash>().expect("hash parse is ok")).collect::<Root>(); 
    let expected_root = Root("08622318f80582f117b8b1421e81eca925afc2743cad610f0b16e0f0740fda82".parse::<Hash>().expect("parse is ok").0
                                    .into_iter().rev().collect::<Vec<_>>().as_slice().try_into().unwrap());

    let hash  = block::Hash::from(expected_root.0);
    // println!("hash = {:?}, expected_root = {:?}", hash, expected_root);

    assert_eq!(calculated_root, expected_root);

}


/// Tests if a transaction has locktime too early possibly trying to collect extra interest
#[tokio::test(flavor = "multi_thread")]
async fn komodo_transaction_locktime_too_early() {

    zebra_test::init();

    // Load komodo sample net blocks
    let blocks: Vec<Arc<Block>> = zebra_state::komodo_test_helpers::komodo_load_testnet_a_node_1();

    // Create a populated state service
    let (state_service, _read_state, _latest_chain_tip, _chain_tip_change) =
        zebra_state::populated_state(blocks.clone(), Network::Testnet).await;
    
    let verifier = Verifier::new(Network::Testnet, state_service);

    let last_block = blocks[blocks.len()-1].clone();
    let last_block_height = last_block.coinbase_height().expect("valid last block height");
    let lock_time = last_block.header.time - chrono::Duration::seconds(6000);

    let fund_height = (last_block_height - 1).expect("fake source fund block height is too small");
    let (input, output, _known_utxos) = mock_transparent_transfer(fund_height, true, 0);

    let expiry_height = (last_block_height + 200).expect("valid last block height");
    let next_height = (last_block_height + 1).expect("valid last block height");

    // Create a non-coinbase V4 tx.
    let transaction = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::Time(lock_time),
        expiry_height,
        joinsplit_data: None,
        sapling_shielded_data: None,
    };
    let unmined_transaction = UnminedTx::from(transaction.clone());

    let result = verifier
        .clone()
        .oneshot(Request::Mempool {
            transaction: unmined_transaction,
            height: next_height,
            check_low_fee: false,
            reject_absurd_fee: false,
        })
        .await;

    assert!( if let Err(TransactionError::KomodoTxLockTimeTooEarly(_, _)) = result { true } else { false } );

    let result = verifier
        .clone()
        .oneshot(Request::Block {
            transaction: Arc::new(transaction),
            known_utxos: Arc::new(HashMap::new()),
            height: next_height,
            time: chrono::MAX_DATETIME,
            previous_hash: last_block.hash(),
            last_tx_verify_data: None,
        })
        .await;

    assert!( if let Err(TransactionError::KomodoTxLockTimeTooEarly(_, _)) = result { true } else { false } );

}

#[tokio::test]
async fn komodo_mempool_request_with_missing_input_is_rejected() {
    let mut state: MockService<_, _, _, _> = MockService::build().for_prop_tests();
    let verifier = Verifier::new(Network::Testnet, state.clone());

    let testnet_blocks: Vec<Arc<Block>> = zebra_state::komodo_test_helpers::komodo_load_testnet_a_node_1();
    let mut testnet_iter = testnet_blocks.iter().flat_map(|block| 
        block
            .transactions
            .iter()
            .map(|transaction| (block.coinbase_height(), transaction))
    );

    let (height, tx) = testnet_iter // cannot use kmd main block as early blocks have various errors
        .find(|(_, tx)| !(tx.is_coinbase() || tx.inputs().is_empty()))
        .expect("At least one non-coinbase transaction with transparent inputs in test vectors");

    let input_outpoint = match tx.inputs()[0] {
        transparent::Input::PrevOut { outpoint, .. } => outpoint,
        transparent::Input::Coinbase { .. } => panic!("requires a non-coinbase transaction"),
    };

    let height = height.expect("valid coinbase height");
    let prev_height = (height - 1).expect("fake prev block height is too small");
    let prev_block = testnet_blocks[(height.0 - 1) as usize].clone();

    tokio::spawn(async move {

        state
            .expect_request(zebra_state::Request::GetMedianTimePast(None))
            .await
            .expect("verifier should call mock state service with correct request")
            .respond(zebra_state::Response::MedianTimePast(Some(prev_block.header.time))); // mock MTP by block time

        state
            .expect_request(zebra_state::Request::Block(zebra_state::HashOrHeight::Height(prev_height)))
            .await
            .expect("verifier should call mock state service with correct request")
            .respond(zebra_state::Response::Block(Some(prev_block)));

        // The first non-coinbase transaction with transparent inputs in our test vectors
        // does not use a lock time, so we don't see Request::BestChainNextMedianTimePast here
        state
            .expect_request(zebra_state::Request::UnspentBestChainUtxo(input_outpoint))
            .await
            .expect("verifier should call mock state service with correct request")
            .respond(zebra_state::Response::UnspentBestChainUtxo(None));
    });

    let verifier_response = verifier
        .oneshot(Request::Mempool {
            transaction: tx.into(),
            height,
            check_low_fee: true,
            reject_absurd_fee: true,
        })
        .await;

    // println!("verifier_response {:?}", verifier_response);
    assert_eq!(
        verifier_response,
        Err(TransactionError::TransparentInputNotFound)
    );
}

#[tokio::test]
async fn komodo_mempool_request_with_present_input_is_accepted() {
    let mut state: MockService<_, _, _, _> = MockService::build().for_prop_tests();
    let verifier = Verifier::new(Network::Mainnet, state.clone());

    let height = NetworkUpgrade::Sapling
        .activation_height(Network::Mainnet)
        .expect("Sapling activation height is specified");
    let fund_height = (height - 1).expect("fake source fund block height is too small");
    let (input, output, known_utxos) = mock_transparent_transfer(fund_height, true, 0);

    // Create a non-coinbase V4 tx with the last valid expiry height.
    let tx = Transaction::V4 {
        inputs: vec![input],
        outputs: vec![output],
        lock_time: LockTime::unlocked(),
        expiry_height: height,
        joinsplit_data: None,
        sapling_shielded_data: None,
    };

    let input_outpoint = match tx.inputs()[0] {
        transparent::Input::PrevOut { outpoint, .. } => outpoint,
        transparent::Input::Coinbase { .. } => panic!("requires a non-coinbase transaction"),
    };

    let prev_height = fund_height;
    let prev_block: Arc<Block> = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_1140408_BYTES.zcash_deserialize_into().expect("valid pre-sapling block to deserialize");

    tokio::spawn(async move {

        state
            .expect_request(zebra_state::Request::GetMedianTimePast(None))
            .await
            .expect("verifier should call mock state service with correct request")
            .respond(zebra_state::Response::MedianTimePast(Some(prev_block.header.time))); // mock MTP by block time

        state
            .expect_request(zebra_state::Request::Block(zebra_state::HashOrHeight::Height(prev_height)))
            .await
            .expect("verifier should call mock state service with correct request")
            .respond(zebra_state::Response::Block(Some(prev_block)));

        state
            .expect_request(zebra_state::Request::UnspentBestChainUtxo(input_outpoint))
            .await
            .expect("verifier should call mock state service with correct request")
            .respond(zebra_state::Response::UnspentBestChainUtxo(
                known_utxos
                    .get(&input_outpoint)
                    .map(|utxo| utxo.utxo.clone()),
            ));
    });


    let verifier_response = verifier
        .oneshot(Request::Mempool {
            transaction: tx.into(),
            height,
            check_low_fee: true, // use rate limiter
            reject_absurd_fee: false,
        })
        .await;

    assert!(
        verifier_response.is_ok(),
        "expected successful verification, got: {verifier_response:?}"
    );
}

/// check komodo mempool low fee rate limiter
#[tokio::test]
async fn komodo_mempool_rate_limiter() {
    let state: MockService<_, _, _, _> = MockService::build().for_prop_tests();
    let verifier = Verifier::new(Network::Mainnet, state.clone());

    let height = NetworkUpgrade::Sapling
        .activation_height(Network::Mainnet)
        .expect("Sapling activation height is specified");
    let fund_height = (height - 1).expect("fake source fund block height is too small");

    let mut rate_err_correct = false;
    for count in 0..100000 {
        let (input, output, known_utxos) = mock_transparent_transfer(fund_height, true, 0);

        // Create a non-coinbase V4 tx with the last valid expiry height.
        let tx = Transaction::V4 {
            inputs: vec![input],
            outputs: vec![output],
            lock_time: LockTime::unlocked(),
            expiry_height: height,
            joinsplit_data: None,
            sapling_shielded_data: None,
        };

        let input_outpoint = match tx.inputs()[0] {
            transparent::Input::PrevOut { outpoint, .. } => outpoint,
            transparent::Input::Coinbase { .. } => panic!("requires a non-coinbase transaction"),
        };

        let prev_height = fund_height;
        let prev_block: Arc<Block> = zebra_test::komodo_vectors::BLOCK_KMDMAINNET_1140408_BYTES.zcash_deserialize_into().expect("valid pre-sapling block to deserialize");

        let mut state = state.clone();
        let verifier = verifier.clone();

        tokio::spawn(async move {

            state
                .expect_request(zebra_state::Request::GetMedianTimePast(None))
                .await
                .expect("verifier should call mock state service with correct request")
                .respond(zebra_state::Response::MedianTimePast(Some(prev_block.header.time))); // mock MTP by block time

            state
                .expect_request(zebra_state::Request::Block(zebra_state::HashOrHeight::Height(prev_height)))
                .await
                .expect("verifier should call mock state service with correct request")
                .respond(zebra_state::Response::Block(Some(prev_block)));

            state
                .expect_request(zebra_state::Request::UnspentBestChainUtxo(input_outpoint))
                .await
                .expect("verifier should call mock state service with correct request")
                .respond(zebra_state::Response::UnspentBestChainUtxo(
                    known_utxos
                        .get(&input_outpoint)
                        .map(|utxo| utxo.utxo.clone()),
                ));
        });

        let verifier_response = verifier
            .oneshot(Request::Mempool {
                transaction: tx.into(),
                height,
                check_low_fee: true, // use rate limiter
                reject_absurd_fee: false,
            })
            .await;

        if let Err(TransactionError::KomodoLowFeeLimit(..)) = verifier_response {
            if count > 1000 {
                rate_err_correct = true;
            }
            break;
        }
    }
    assert!(rate_err_correct, "expected komodo tx rate limiter error after 1000 txns with low fee");
}
