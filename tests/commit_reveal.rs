use anyhow::Result;
use bitcoin::TapLeafHash;
use bitcoin::TapSighash;
use bitcoin::TapSighashType;
use bitcoin::hashes::Hash;
use bitcoin::key::{TapTweak, TweakedKeypair};
use bitcoin::secp256k1::Keypair;
use bitcoin::secp256k1::Message;
use bitcoin::sighash::Prevouts;
use bitcoin::sighash::SighashCache;
use bitcoin::taproot::LeafVersion;
use bitcoin::taproot::TaprootBuilder;
use bitcoin::{
    Amount, OutPoint, Txid, consensus::encode::serialize as serialize_tx, key::Secp256k1,
    transaction::TxOut,
};
use clap::Parser;
use kontor::api::compose::compose;
use kontor::config::TestConfig;
use kontor::test_utils;
use kontor::witness_data::TokenBalance;
use kontor::{bitcoin_client::Client, config::Config};
use std::str::FromStr;

#[tokio::test]
async fn test_taproot_transaction() -> Result<()> {
    let client = Client::new_from_config(Config::try_parse()?)?;
    let config = TestConfig::try_parse()?;

    let secp = Secp256k1::new();

    let (seller_address, seller_child_key, _) =
        test_utils::generate_taproot_address_from_mnemonic(&secp, &config.taproot_key_path, 0)?;

    let keypair = Keypair::from_secret_key(&secp, &seller_child_key.private_key);
    let (internal_key, _parity) = keypair.x_only_public_key();

    // UTXO loaded with 9000 sats
    let out_point = OutPoint {
        txid: Txid::from_str("dd3d962f95741f2f5c3b87d6395c325baa75c4f3f04c7652e258f6005d70f3e8")?,
        vout: 0,
    };

    let utxo_for_output = TxOut {
        value: Amount::from_sat(9000),
        script_pubkey: seller_address.script_pubkey(),
    };

    // Create token balance data
    let token_value = 1000;
    let token_balance = TokenBalance {
        value: token_value,
        name: "token_name".to_string(),
    };

    let mut serialized_token_balance = Vec::new();
    ciborium::into_writer(&token_balance, &mut serialized_token_balance).unwrap();

    let (mut attach_tx, mut spend_tx, tap_script) = compose(
        &seller_address,
        &internal_key,
        vec![(out_point, utxo_for_output.clone())],
        serialized_token_balance.as_slice(),
        2,
    )?;

    let input_index = 0;

    // Sign the attach transaction
    let sighash_type = TapSighashType::Default;
    let prevouts = vec![utxo_for_output];
    let prevouts = Prevouts::All(&prevouts);

    let mut sighasher = SighashCache::new(&attach_tx);
    let sighash = sighasher
        .taproot_key_spend_signature_hash(input_index, &prevouts, sighash_type)
        .expect("failed to construct sighash");

    let tweaked: TweakedKeypair = keypair.tap_tweak(&secp, None);
    let msg = Message::from_digest(sighash.to_byte_array());
    let signature = secp.sign_schnorr(&msg, &tweaked.to_inner());

    let signature = bitcoin::taproot::Signature {
        signature,
        sighash_type,
    };
    attach_tx.input[input_index]
        .witness
        .push(signature.to_vec());

    // Sign the spend transaction
    let mut spend_sighasher = SighashCache::new(&spend_tx);

    // First sign the keyspend input
    let prevout = vec![attach_tx.output[1].clone(), attach_tx.output[0].clone()];
    let prevouts = Prevouts::All(&prevout);

    // Get the sighash for the keyspend
    let key_sighash: TapSighash = spend_sighasher
        .taproot_key_spend_signature_hash(0, &prevouts, sighash_type)
        .expect("failed to construct sighash");

    let tweaked: TweakedKeypair = keypair.tap_tweak(&secp, None);
    let msg = Message::from_digest(key_sighash.to_byte_array());
    let signature = secp.sign_schnorr(&msg, &tweaked.to_inner());

    let signature = bitcoin::taproot::Signature {
        signature,
        sighash_type,
    };

    // Get the sighash for the scriptspend
    let script_sighash: TapSighash = spend_sighasher
        .taproot_script_spend_signature_hash(
            1,
            &prevouts,
            TapLeafHash::from_script(&tap_script, LeafVersion::TapScript),
            sighash_type,
        )
        .expect("Failed to create sighash");

    // push the key spend sig onto the witness
    spend_tx.input[0].witness.push(signature.to_vec());

    // Sign the script spend input
    let msg = Message::from_digest(script_sighash.to_byte_array());
    let signature = secp.sign_schnorr(&msg, &keypair);
    let signature = bitcoin::taproot::Signature {
        signature,
        sighash_type,
    };
    spend_tx.input[1].witness.push(signature.to_vec());
    spend_tx.input[1].witness.push(tap_script.as_bytes());

    let taproot_spend_info = TaprootBuilder::new()
        .add_leaf(0, tap_script.clone())
        .expect("Failed to add leaf")
        .finalize(&secp, internal_key)
        .expect("Failed to finalize Taproot tree");

    let control_block = taproot_spend_info
        .control_block(&(tap_script.clone(), LeafVersion::TapScript))
        .expect("Failed to create control block");
    spend_tx.input[1].witness.push(control_block.serialize());

    let attach_tx_hex = hex::encode(serialize_tx(&attach_tx));
    let spend_tx_hex = hex::encode(serialize_tx(&spend_tx));

    let result = client
        .test_mempool_accept(&[attach_tx_hex, spend_tx_hex])
        .await?;

    assert_eq!(result.len(), 2, "Expected exactly two transaction results");
    assert!(result[0].allowed, "Attach transaction was rejected");
    assert!(result[1].allowed, "Spend transaction was rejected");

    let witness = spend_tx.input[1].witness.clone();
    // 1. Check the total number of witness elements first
    assert_eq!(witness.len(), 3, "Witness should have exactly 3 elements");

    // 2. Check each element individually
    let signature = witness.to_vec()[0].clone();
    assert!(!signature.is_empty(), "Signature should not be empty");

    let script_bytes = witness.to_vec()[1].clone();
    assert_eq!(
        script_bytes,
        tap_script.as_bytes().to_vec(),
        "Script in witness doesn't match expected script"
    );

    let control_block_bytes = witness.to_vec()[2].clone();
    assert_eq!(
        control_block_bytes,
        control_block.serialize(),
        "Control block in witness doesn't match expected control block"
    );

    Ok(())
}
