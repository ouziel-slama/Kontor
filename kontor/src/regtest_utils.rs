use std::str::FromStr;

use anyhow::Result;
use bitcoin::{Amount, OutPoint, TxOut, Txid};

use crate::bitcoin_client::{Client, types::RawTransactionInput};

pub async fn ensure_wallet_setup(client: &Client) -> Result<()> {
    // Check if wallet exists
    let wallet_name = "regtest_taproot_wallet";
    let wallets = client.list_wallets().await?;
    println!("Available wallets: {:?}", wallets);

    if !wallets.contains(&wallet_name.to_string()) {
        // Create wallet if it doesn't exist
        client.create_wallet(wallet_name).await?;
        println!("Created new wallet: {}", wallet_name);
    } else {
        println!("Wallet {} already exists", wallet_name);
    }

    // Check balances and spendable funds
    let balance: f64 = client.get_balance().await?;
    let unspent = client.list_unspent(1, 9999999, &[]).await?;

    println!("Current wallet balance: {} BTC", balance);
    println!("Found {} spendable UTXOs", unspent.len());

    // If we have no spendable UTXOs, we need to generate and mature coins
    if unspent.is_empty() {
        println!("No spendable UTXOs found. Generating and maturing coins...");

        // Get a mining address
        let mining_address = client.get_new_address().await?;
        println!("Mining to address: {}", mining_address);

        // Generate 101 blocks to get mature coins (coinbase needs 100 confirmations)
        println!("Generating 101 blocks to create mature coins...");
        client.generate_to_address(101, &mining_address).await?;

        // Generate 10 more blocks to another address to ensure we have multiple UTXOs
        let second_address = client.get_new_address().await?;
        println!("Generating 10 more blocks to second address...");
        client.generate_to_address(10, &second_address).await?;

        // Check new balance and UTXOs
        let new_balance: f64 = client.get_balance().await?;
        let new_unspent = client.list_unspent(1, 9999999, &[]).await?;

        println!("New balance after mining: {} BTC", new_balance);
        println!("New spendable UTXOs: {}", new_unspent.len());

        if new_unspent.is_empty() {
            return Err(anyhow::anyhow!("Failed to generate spendable UTXOs"));
        }
    }

    // Print details about available UTXOs
    let spendable = client.list_unspent(1, 9999999, &[]).await?;
    if !spendable.is_empty() {
        println!("Spendable UTXOs:");
        for (i, utxo) in spendable.iter().enumerate().take(3) {
            println!(
                "  UTXO {}: {}:{} - {} BTC (confirmations: {})",
                i, utxo.txid, utxo.vout, utxo.amount, utxo.confirmations
            );
        }
    }

    Ok(())
}

pub async fn get_regtest_utxo(
    client: &Client,
    address: &bitcoin::Address,
) -> Result<(OutPoint, TxOut)> {
    // Amount to send to the taproot address
    let amount = Amount::from_sat(5000);

    // Get list of spendable UTXOs
    let unspent = client.list_unspent(1, 9999999, &[]).await?;
    println!(
        "Found {} spendable UTXOs for creating raw tx",
        unspent.len()
    );

    if unspent.is_empty() {
        return Err(anyhow::anyhow!("No spendable UTXOs available"));
    }

    // Use the first available UTXO
    let input_utxo = &unspent[0];
    println!(
        "Using UTXO: {}:{} with {} BTC (confirmations: {})",
        input_utxo.txid, input_utxo.vout, input_utxo.amount, input_utxo.confirmations
    );

    // Convert BTC to satoshis for precise calculation
    let input_amount_sats = (input_utxo.amount * 100_000_000.0) as u64;
    let output_amount_sats = amount.to_sat();
    let fee_sats = 1000; // 1000 satoshis fee

    // Calculate change amount in satoshis
    let change_amount_sats = input_amount_sats - output_amount_sats - fee_sats;

    // Convert back to BTC for the API
    let change_amount_btc = change_amount_sats as f64 / 100_000_000.0;

    println!(
        "Input: {} sats, Output: {} sats, Fee: {} sats, Change: {} sats",
        input_amount_sats, output_amount_sats, fee_sats, change_amount_sats
    );

    // Create inputs for raw transaction
    let inputs = vec![RawTransactionInput {
        txid: input_utxo.txid.clone(),
        vout: input_utxo.vout,
        sequence: None,
    }];

    // Get a change address
    let change_address = client.get_new_address().await?;
    println!("Change will go to: {}", change_address);

    // Create outputs with precise amounts
    let mut outputs = std::collections::HashMap::new();
    outputs.insert(address.to_string(), amount.to_btc());
    outputs.insert(change_address, change_amount_btc);

    // Print the exact values we're using
    println!(
        "Output amounts: {} BTC to target, {} BTC as change",
        amount.to_btc(),
        change_amount_btc
    );

    // Create the raw transaction
    let raw_tx = client
        .create_raw_transaction(&inputs, &outputs, None, None)
        .await?;
    println!("Created raw transaction");

    // Sign the transaction
    let signed_tx = client.sign_raw_transaction_with_wallet(&raw_tx).await?;

    if !signed_tx.complete {
        return Err(anyhow::anyhow!("Failed to sign transaction"));
    }

    println!("Signed transaction successfully");

    // Send the transaction
    let txid_str = client.send_raw_transaction(&signed_tx.hex).await?;
    println!("Sent raw transaction: {}", txid_str);

    let txid = Txid::from_str(&txid_str).unwrap();

    // Generate a block to confirm the transaction
    let mining_address = client.get_new_address().await?;
    client.generate_to_address(1, &mining_address).await?;
    println!("Generated block to confirm transaction");

    // Since we can't rely on list_unspent for the taproot address,
    // we'll just create the OutPoint and TxOut directly
    let vout = 0; // Assuming the taproot address is the first output

    // Print what we're returning
    println!(
        "Using transaction: {}:{} with {} sats for taproot address",
        txid,
        vout,
        amount.to_sat()
    );

    let out_point = OutPoint { txid, vout };
    let utxo_out = TxOut {
        value: amount,
        script_pubkey: address.script_pubkey(),
    };

    Ok((out_point, utxo_out))
}
