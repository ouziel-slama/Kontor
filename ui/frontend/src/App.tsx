import { useState } from 'react'
import './App.css'
import type { AddressEntry, GetAddressesResult } from '@stacks/connect/dist/types/methods'
import {
  request as satsConnectRequest,

  type RpcResult,
} from "sats-connect";

import { request as connectRequest } from '@stacks/connect'


import * as bitcoin from 'bitcoinjs-lib'

import './App.css'


interface ExtendedAddressEntry extends AddressEntry {
  purpose: string
  addressType: string
}

interface Utxo {
  txid: string
  vout: number
  value: number
  status: {
    confirmed: boolean
    block_height: number
    block_hash: string
    block_time: number
  }
}

interface TransactionInput {
  previous_output: string
  script_sig: string
  sequence: number
  witness: string[]
}

interface TransactionOutput {
  script_pubkey: string
  value: number
}

interface Transaction {
  version: number
  lock_time: number
  input: TransactionInput[]
  output: TransactionOutput[]
}

interface ComposeResult {
  commit_transaction: Transaction
  reveal_transaction: Transaction
  tap_script: string
  chained_tap_script: string | null
}

const handleBroadcastTransaction = async () => {

}

// Add a function to sign the commit transaction using sign_message
const signCommitTransaction = async (composeResult: ComposeResult, address: ExtendedAddressEntry, utxos: Utxo[]) => {
  try {
    // 1. Create a bitcoinjs-lib transaction from your compose result
    const tx = new bitcoin.Transaction();
    tx.version = composeResult.commit_transaction.version;
    tx.locktime = composeResult.commit_transaction.lock_time;

    // Add inputs
    composeResult.commit_transaction.input.forEach(input => {
      // Parse the previous output (txid:vout format)
      const [txid, voutStr] = input.previous_output.split(':');
      const vout = parseInt(voutStr, 10);

      // Add the input to the transaction
      tx.addInput(
        Buffer.from(txid, 'hex').reverse(), // Bitcoin txids are byte-reversed
        vout,
        input.sequence
      );
    });

    // Add outputs
    composeResult.commit_transaction.output.forEach(output => {
      tx.addOutput(
        Buffer.from(output.script_pubkey, 'hex'),
        output.value
      );
    });

    // 2. Create a map of txid:vout to UTXO details for easy lookup
    const utxoMap = utxos.reduce((map, utxo) => {
      map[`${utxo.txid}:${utxo.vout}`] = utxo;
      return map;
    }, {} as Record<string, Utxo>);

    // 3. For each input, create the signature hash and sign it
    const signedTx = tx.clone();

    // Create arrays to hold all the previous output scripts and values
    const prevOutScripts = [];
    const prevOutValues = [];

    // Prepare data for all inputs
    for (let i = 0; i < composeResult.commit_transaction.input.length; i++) {
      const input = composeResult.commit_transaction.input[i];
      const utxo = utxoMap[input.previous_output];

      if (!utxo) {
        throw new Error(`UTXO not found for input ${input.previous_output}`);
      }

      // For Taproot key spend, the script is P2TR with the x-only pubkey
      const p2trScript = Buffer.from('5120' + address.publicKey, 'hex');
      prevOutScripts.push(p2trScript);
      prevOutValues.push(utxo.value);
    }

    // Sign each input
    for (let inputIndex = 0; inputIndex < tx.ins.length; inputIndex++) {
      // For Taproot key spend, create the hash for witness v1
      const hashForSignature = tx.hashForWitnessV1(
        inputIndex,
        prevOutScripts,
        prevOutValues,
        bitcoin.Transaction.SIGHASH_DEFAULT // Taproot default sighash
      );

      // Convert the hash to hex for signing
      const hashHex = hashForSignature.toString('hex');

      // const signResult = signMessage({
      //   message: hashHex,
      // })
      // const sign = signTransaction()

      // const sign = await signMessage
      let signResult: RpcResult<"signMessage">

      try {

        // Sign the hash using Xverse wallet's sign_message
        signResult = await satsConnectRequest('signMessage', {
          message: hashHex,
          address: address.address,
        });
        if (signResult.status === 'error') {
          throw new Error(`Error signing message: ${signResult.error.message}`);
        }
      } catch (error) {
        console.error('Error signing message:', error);
        throw error;
      }

      // The signature from Xverse will be in base64, convert it to buffer
      const signature = Buffer.from(signResult.result.signature, 'base64');

      // Add the signature to the transaction's witness
      signedTx.ins[inputIndex].witness = [signature];
    }

    // Get the signed transaction hex
    const signedTxHex = signedTx.toHex();

    console.log('Signed transaction:', signedTxHex);
    return { tx: signedTxHex };

  } catch (error) {
    console.error('Error signing transaction:', error);
    throw error;
  }
};

function WalletComponent() {
  const [address, setAddress] = useState<ExtendedAddressEntry | undefined>()
  const [utxos, setUtxos] = useState<Utxo[]>([])
  const [composeResult, setComposeResult] = useState<ComposeResult | undefined>()
  const [error, setError] = useState<string>('')
  const [signedTx, setSignedTx] = useState<string>('');



  const handleGetAddresses = async () => {
    try {
      // Fetch addresses
      const response: GetAddressesResult = await connectRequest('getAddresses')
      const paymentAddress = (response.addresses as ExtendedAddressEntry[]).find(
        addr => addr.addressType === 'p2tr'
      )
      setAddress(paymentAddress)

      if (paymentAddress) {
        // Fetch UTXOs for the address
        const electrsUrl = import.meta.env.VITE_ELECTRS_URL
        const utxoResponse = await fetch(`${electrsUrl}/address/${paymentAddress.address}/utxo`)
        if (!utxoResponse.ok) {
          throw new Error('Failed to fetch UTXOs')
        }
        const utxoData = await utxoResponse.json()
        setUtxos(utxoData)
      }
    } catch (err) {
      setError('Failed to get addresses or UTXOs')
      console.error(err)
    }
  }

  const handleCompose = async (address: ExtendedAddressEntry, utxos: Utxo[]) => {
    if (utxos.length > 0) {
      console.log('Composing commit/reveal transactions')
      const kontorUrl = import.meta.env.VITE_KONTOR_URL
      const base64EncodedData = btoa('Hello, world!')
      const kontorResponse = await fetch(`${kontorUrl}/compose?address=${address.address}&x_only_public_key=${address.publicKey}&funding_utxo_ids=${utxos.map(utxo => utxo.txid + ':' + utxo.vout).join(',')}&sat_per_vbyte=2&script_data=${base64EncodedData}`)
      const kontorData = await kontorResponse.json()
      console.log('Kontor data:', kontorData)
      setComposeResult(kontorData.result)
    }
  }

  const handleSignTransaction = async () => {
    if (!address || !composeResult || utxos.length === 0) {
      setError('No address, transaction, or UTXOs to sign');
      return;
    }

    try {
      const result = await signCommitTransaction(composeResult, address, utxos);
      setSignedTx(result.tx);
    } catch (err) {
      setError('Failed to sign transaction');
      console.error(err);
    }
  };

  return (
    <div className="wallet-container">
      <h1>COMPOSE</h1>
      <button onClick={handleGetAddresses}>Get Wallet Addresses</button>
      {address && (
        <div className="addresses">
          <h2>Your Taproot Address:</h2>
          <ul>
            <li>
              <strong>{address.purpose}:</strong> {address.address}
            </li>
          </ul>
          {utxos.length > 0 && (
            <div className="utxos">
              <h3>UTXOs:</h3>
              <ul>
                {utxos.map((utxo, index) => (
                  <li key={index}>
                    <strong>TXID:</strong> {utxo.txid}
                    <br />
                    <strong>Vout:</strong> {utxo.vout}
                    <br />
                    <strong>Value:</strong> {utxo.value / 100000000} BTC
                    <br />
                    <strong>Status:</strong> {utxo.status.confirmed ? 'Confirmed' : 'Unconfirmed'}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {composeResult && (
            <div className="transactions">
              <h3>Commit Transaction:</h3>
              <div className="transaction-details">
                <p><strong>Version:</strong> {composeResult.commit_transaction.version}</p>
                <p><strong>Lock Time:</strong> {composeResult.commit_transaction.lock_time}</p>
                <h4>Inputs:</h4>
                <ul>
                  {composeResult.commit_transaction.input.map((input, index) => (
                    <li key={index}>
                      <strong>Previous Output:</strong> {input.previous_output}
                      <br />
                      <strong>Sequence:</strong> {input.sequence}
                    </li>
                  ))}
                </ul>
                <h4>Outputs:</h4>
                <ul>
                  {composeResult.commit_transaction.output.map((output, index) => (
                    <li key={index}>
                      <strong>Script Pubkey:</strong> {output.script_pubkey}
                      <br />
                      <strong>Value:</strong> {output.value / 100000000} BTC
                    </li>
                  ))}
                </ul>

              </div>

              <h3>Reveal Transaction:</h3>
              <div className="transaction-details">
                <p><strong>Version:</strong> {composeResult.reveal_transaction.version}</p>
                <p><strong>Lock Time:</strong> {composeResult.reveal_transaction.lock_time}</p>
                <h4>Inputs:</h4>
                <ul>
                  {composeResult.reveal_transaction.input.map((input, index) => (
                    <li key={index}>
                      <strong>Previous Output:</strong> {input.previous_output}
                      <br />
                      <strong>Sequence:</strong> {input.sequence}
                    </li>
                  ))}
                </ul>
                <h4>Outputs:</h4>
                <ul>
                  {composeResult.reveal_transaction.output.map((output, index) => (
                    <li key={index}>
                      <strong>Script Pubkey:</strong> {output.script_pubkey}
                      <br />
                      <strong>Value:</strong> {output.value / 100000000} BTC
                    </li>
                  ))}
                </ul>

              </div>

              <h3>Tap Script:</h3>
              <p className="tap-script">{composeResult.tap_script}</p>
            </div>
          )}
        </div>
      )}
      {
        !composeResult && address && utxos.length > 0 && (
          <button onClick={() => handleCompose(address, utxos)}>Compose Commit/Reveal Transactions</button>
        )
      }
      {composeResult && (
        <div className="sign-transaction">

          <button onClick={handleSignTransaction}>Sign Commit Transaction</button>

          {signedTx && (
            <>
              <div className="signed-transaction">
                <h3>Signed Transaction:</h3>
                <p className="tx-hex">{signedTx}</p>
              </div>

              <button onClick={handleBroadcastTransaction}>Broadcast Transaction</button>
            </>
          )}
        </div>
      )}
      {error && <p className="error">{error}</p>}
    </div>
  )
}

function App() {
  return <WalletComponent />
}

export default App
