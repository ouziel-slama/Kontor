import { useState } from 'react'
import { request } from '@stacks/connect'
import './App.css'
import type { AddressEntry, GetAddressesResult } from '@stacks/connect/dist/types/methods'


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

function WalletComponent() {
  const [address, setAddress] = useState<ExtendedAddressEntry | undefined>()
  const [utxos, setUtxos] = useState<Utxo[]>([])
  const [composeResult, setComposeResult] = useState<ComposeResult | undefined>()
  const [error, setError] = useState<string>('')


  const handleGetAddresses = async () => {
    try {
      // Fetch addresses
      const response: GetAddressesResult = await request('getAddresses')
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
      {error && <p className="error">{error}</p>}
    </div>
  )
}

function App() {
  return <WalletComponent />
}

export default App
