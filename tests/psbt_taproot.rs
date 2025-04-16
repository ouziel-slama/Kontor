use anyhow::Result;
use bip39;
use bip39::Mnemonic;
use bitcoin::PrivateKey;
use bitcoin::TapSighashType;
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::hashes::{Hash, sha256};
use bitcoin::key::{PublicKey as BitcoinPublicKey, TapTweak, TweakedKeypair};
use bitcoin::opcodes::all::OP_RETURN;
use bitcoin::script::PushBytesBuf;
use bitcoin::secp256k1::Message;
use bitcoin::secp256k1::{All, Keypair};
use bitcoin::sighash::Prevouts;
use bitcoin::sighash::SighashCache;
use bitcoin::taproot::TaprootBuilder;
use bitcoin::{
    Amount, OutPoint, ScriptBuf, Sequence, Txid, Witness,
    absolute::LockTime,
    address::{Address, KnownHrp},
    consensus::encode::serialize as serialize_tx,
    key::{CompressedPublicKey, Secp256k1},
    opcodes::all::{OP_CHECKSIG, OP_EQUALVERIFY, OP_SHA256},
    psbt::{Input, Output, Psbt, PsbtSighashType},
    script::Builder,
    secp256k1::{self},
    transaction::{Transaction, TxIn, TxOut, Version},
};
use bitcoin::{Network, XOnlyPublicKey};
use clap::Parser;
use hex;
use kontor::witness_data::WitnessData;
use kontor::{bitcoin_client::Client, config::Config, op_return::OpReturnData};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::str::FromStr;

#[tokio::test]
async fn test_psbt_with_secret() -> Result<()> {
    let config = Config::try_parse()?;
    let client = Client::new_from_config(config.clone())?;

    // Create secp256k1 context
    let secp = Secp256k1::new();

    // Generate seller's address and keys
    let (seller_address, seller_child_key, seller_compressed_pubkey) =
        generate_address_from_mnemonic(&secp, &config.taproot_key_path, 0)?;
    let (buyer_address, buyer_child_key, buyer_compressed_pubkey) =
        generate_address_from_mnemonic(&secp, &config.taproot_key_path, 1)?;
    println!("Generated seller address: {}", seller_address);
    println!("Generated buyer address: {}", buyer_address);

    // Assuming you have the seller's keypair or public key
    // let seller_keypair = Keypair::from_secret_key(&secp, &seller_child_key.private_key);
    // let seller_pubkey = seller_keypair.public_key();
    let x = seller_child_key.to_keypair(&secp);

    // Get the XOnlyPublicKey directly
    let seller_internal_key = XOnlyPublicKey::from(x.public_key());

    // Or if you already have a PublicKey
    // let seller_internal_key = XOnlyPublicKey::from(existing_public_key);

    let (serialized_token_balance, witness_script) =
        build_serialized_token_and_witness_script(&seller_compressed_pubkey, 1000);

    let attach_tx = build_signed_attach_tx(
        &secp,
        &seller_address,
        seller_internal_key,
        &seller_child_key,
        &witness_script,
    )?;

    let raw_attach_tx_hex = hex::encode(serialize_tx(&attach_tx));

    let result = client.test_mempool_accept(&[raw_attach_tx_hex]).await?;

    // Assert both transactions are allowed
    assert_eq!(result.len(), 1, "Expected exactly one transaction result");
    println!("Result: {:#?}", result);
    assert!(result[0].allowed, "Attach transaction was rejected");

    Ok(())
}

fn generate_address_from_mnemonic(
    secp: &Secp256k1<secp256k1::All>,
    path: &Path,
    index: u32,
) -> Result<(Address, Xpriv, CompressedPublicKey), anyhow::Error> {
    // Read mnemonic from secret file
    let mnemonic = fs::read_to_string(path)
        .expect("Failed to read mnemonic file")
        .trim()
        .to_string();

    // Parse the mnemonic
    let mnemonic = Mnemonic::from_str(&mnemonic).expect("Invalid mnemonic phrase");

    // Generate seed from mnemonic
    let seed = mnemonic.to_seed("");

    // Create master key
    let master_key =
        Xpriv::new_master(Network::Bitcoin, &seed).expect("Failed to create master key");

    // Derive first child key using a proper derivation path
    let path = DerivationPath::from_str(&format!("m/86'/0'/0'/0/{}", index))
        .expect("Invalid derivation path");
    let child_key = master_key
        .derive_priv(&secp, &path)
        .expect("Failed to derive child key");

    // Get the private key
    let private_key = PrivateKey::new(child_key.private_key, Network::Bitcoin);

    // Get the public key
    let public_key = BitcoinPublicKey::from_private_key(&secp, &private_key);
    let compressed_pubkey = bitcoin::CompressedPublicKey(public_key.inner);

    // Create a Taproot address
    let x_only_pubkey = public_key.inner.x_only_public_key().0;
    let address = Address::p2tr(secp, x_only_pubkey, None, KnownHrp::Mainnet);

    Ok((address, child_key, compressed_pubkey))
}

fn build_serialized_token_and_witness_script(
    seller_compressed_pubkey: &CompressedPublicKey,
    token_value: u64,
) -> (Vec<u8>, ScriptBuf) {
    let token_balance = WitnessData::TokenBalance {
        value: token_value,
        name: "token_name".to_string(),
    };

    let serialized_token_balance = rmp_serde::to_vec(&token_balance).unwrap();
    let witness_script = Builder::new()
        .push_slice(b"KNTR")
        .push_opcode(OP_EQUALVERIFY)
        .push_opcode(OP_SHA256)
        .push_slice(sha256::Hash::hash(&serialized_token_balance).as_byte_array())
        .push_opcode(OP_EQUALVERIFY)
        .push_slice(seller_compressed_pubkey.to_bytes())
        .push_opcode(OP_CHECKSIG)
        .into_script();

    (serialized_token_balance, witness_script)
}

fn build_signed_attach_tx(
    secp: &Secp256k1<All>,
    seller_address: &Address,
    seller_internal_key: XOnlyPublicKey,
    seller_child_key: &Xpriv,
    witness_script: &ScriptBuf,
) -> Result<Transaction> {
    // Create a TaprootBuilder with witness script
    let builder = TaprootBuilder::new()
        .add_leaf(0, witness_script.clone())
        .expect("valid leaf");

    // Finalize the Taproot output
    let tap_output = builder
        .finalize(secp, seller_internal_key)
        .expect("valid finalization");

    // Create a Taproot address from this structure -- used for the unspendable utxo
    let script_address = Address::p2tr(
        secp,
        tap_output.internal_key(),
        tap_output.merkle_root(),
        Network::Bitcoin,
    );

    let out_point = OutPoint {
        txid: Txid::from_str("dd3d962f95741f2f5c3b87d6395c325baa75c4f3f04c7652e258f6005d70f3e8")?,
        vout: 0,
    };

    // utxo to spend from
    let spend_from_utxo = TxOut {
        value: Amount::from_sat(9000),
        script_pubkey: seller_address.script_pubkey(), // Use seller_address here, not script_address
    };

    let input = TxIn {
        previous_output: out_point,
        script_sig: ScriptBuf::default(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::default(),
    };

    let change = TxOut {
        value: Amount::from_sat(7700),
        script_pubkey: seller_address.script_pubkey(),
    };

    let mut op_return_script = ScriptBuf::new();
    op_return_script.push_opcode(OP_RETURN);
    op_return_script.push_slice(b"KNTR");

    let op_return_data = OpReturnData::Attach { output_index: 0 };
    let s = rmp_serde::to_vec(&op_return_data).unwrap();
    op_return_script.push_slice(PushBytesBuf::try_from(s)?);

    let op_return = TxOut {
        value: Amount::from_sat(0),
        script_pubkey: op_return_script,
    };

    // Create transaction
    let mut create_tx = Transaction {
        version: Version(2),
        lock_time: LockTime::ZERO,
        input: vec![input],
        output: vec![
            TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: script_address.script_pubkey(), // script address of the unspendable utxo
            },
            change,
            op_return,
        ],
    };

    let input_index = 0;
    let sighash_type = TapSighashType::Default;
    let prevouts = vec![spend_from_utxo];
    let prevouts = Prevouts::All(&prevouts);

    let mut sighasher = SighashCache::new(&mut create_tx);

    let sighash = sighasher
        .taproot_key_spend_signature_hash(input_index, &prevouts, sighash_type)
        .expect("failed to construct sighash");

    // Create keypair from seller's private key
    let keypair = Keypair::from_secret_key(secp, &seller_child_key.private_key);

    // For a Taproot address without scripts, use None for the merkle root
    let tweaked: TweakedKeypair = keypair.tap_tweak(secp, None);

    let msg = Message::from_digest(sighash.to_byte_array());
    let signature = secp.sign_schnorr(&msg, &tweaked.to_inner());

    // Update the witness stack
    let signature = bitcoin::taproot::Signature {
        signature,
        sighash_type,
    };
    create_tx.input[input_index].witness = Witness::p2tr_key_spend(&signature);

    Ok(create_tx)
}
