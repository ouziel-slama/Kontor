import { useState, useEffect } from 'react'
import './App.css'
import {
  AddressPurpose,
  getProviders,
  request as satsConnectRequest,
  RpcErrorCode,
} from "sats-connect";

import * as bitcoin from 'bitcoinjs-lib'

import './App.css'

import * as ecc from 'tiny-secp256k1';
import { ECPairFactory } from 'ecpair';

// --- Constants ---
const SATS_PER_BTC = 100_000_000;
const PROVIDER_ID_XVERSE = 'XverseProviders.BitcoinProvider';
const PROVIDER_ID_LEATHER = 'LeatherProvider';
const PROVIDER_ID_UNISAT = 'unisat';
const ADDRESS_TYPE_P2TR = 'p2tr';

const electrsUrl = import.meta.env.VITE_ELECTRS_URL;
const kontorUrl = import.meta.env.VITE_KONTOR_URL;

// --- Types ---
interface UnisatProvider {
  requestAccounts(): Promise<string[]>;
  getAccounts(): Promise<string[]>;
  getPublicKey(): Promise<string>;
  getBalance(): Promise<{ confirmed: number; unconfirmed: number; total: number }>;
  signPsbt(psbtHex: string, options?: any): Promise<string>;
  getNetwork(): Promise<string>;
}

declare global {
  interface Window {
    unisat?: UnisatProvider;
  }
}

interface Provider {
  id: string;
  name: string;
  icon: string;
}

interface XverseAddressEntry {
  address: string
  publicKey: string
  purpose: AddressPurpose
  addressType: string
}

interface LeatherAddressEntry {
  address: string
  publicKey: string
  type: string
  tweakedPublicKey: string
}

interface ComposeAddressEntry {
  address: string
  publicKey: string
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

interface TapLeafScript {
  leafVersion: number
  script: string
  controlBlock: string
}

interface ComposeResult {
  commit_transaction: Transaction
  commit_transaction_hex: string
  reveal_transaction: Transaction
  reveal_transaction_hex: string
  commit_psbt_hex: string
  reveal_psbt_hex: string
  tap_script: string
  tap_leaf_script: TapLeafScript
  chained_tap_script: string | null
}

interface TestMempoolAcceptResult {
  txid: string;
  wtxid: string;
  allowed: boolean;
  reject_reason: string | null;
  vsize: number | null;
  fee: number | null;
}

interface TestMempoolAcceptResultWrapper {
  result: TestMempoolAcceptResult[]
}

// --- Helper Functions ---
const convertKebabToSnake = (obj: Record<string, any>): Record<string, any> => {
  return Object.entries(obj).reduce((acc, [key, value]) => {
    const snakeKey = key.replace(/-([a-z])/g, (_, letter) => `_${letter}`);
    acc[snakeKey] = value;
    return acc;
  }, {} as Record<string, any>);
};

async function fetchUtxosFromApi(address: string): Promise<Utxo[]> {
  const response = await fetch(`${electrsUrl}/address/${address}/utxo`);
  if (!response.ok) {
    throw new Error('Failed to fetch UTXOs');
  }
  return response.json();
}

async function composeTransactionOnApi(address: ComposeAddressEntry, utxos: Utxo[], inputData: string): Promise<ComposeResult> {
  const base64EncodedData = btoa(inputData || '');
  const fundingUtxoIds = utxos.map(utxo => `${utxo.txid}:${utxo.vout}`).join(',');
  const url = `${kontorUrl}/compose?address=${address.address}&x_only_public_key=${address.publicKey}&funding_utxo_ids=${fundingUtxoIds}&sat_per_vbyte=2&script_data=${base64EncodedData}`;

  const response = await fetch(url);
  const data = await response.json();
  if (data.error) {
    throw new Error(data.error);
  }
  return data.result;
}

async function broadcastTransactionOnApi(signedTx: string): Promise<TestMempoolAcceptResult[]> {
  const response = await fetch(`${kontorUrl}/api/test_mempool_accept?txs=${signedTx}`);
  const rawData = await response.json();
  const convertedData: TestMempoolAcceptResultWrapper = {
    result: rawData.result.map((item: any) => convertKebabToSnake(item))
  };
  return convertedData.result;
}

async function signPsbt(psbtHex: string, sourceAddress: string, provider: string, scriptLeafData?: TapLeafScript): Promise<string> {
  const psbt = bitcoin.Psbt.fromHex(psbtHex);

  if (scriptLeafData) {
    psbt.updateInput(
      0,
      {
        tapLeafScript: [{
          leafVersion: scriptLeafData.leafVersion,
          script: Buffer.from(scriptLeafData.script, 'hex'),
          controlBlock: Buffer.from(scriptLeafData.controlBlock, 'hex')
        }]
      }
    )
  }

  const commonSignOptions = {
    broadcast: false,
    signInputs: { [sourceAddress]: Array.from({ length: psbt.txInputs.length }, (_, i) => i) },
  };

  const payload = provider === PROVIDER_ID_LEATHER
    ? { ...commonSignOptions, hex: psbt.toHex() }
    : { ...commonSignOptions, psbt: psbt.toBase64() };

  const res = await satsConnectRequest(
    'signPsbt',
    payload as any,
    provider
  );

  if (res.status === 'error') {
    throw new Error(`Signing failed: ${res.error?.message || 'Unknown error'}`);
  }

  const signedPsbt = provider === PROVIDER_ID_LEATHER
    ? bitcoin.Psbt.fromHex((res.result as any).hex)
    : bitcoin.Psbt.fromBase64(res.result.psbt);

  signedPsbt.finalizeAllInputs();
  const tx = signedPsbt.extractTransaction();
  return tx.toHex();
}

// --- UI Components ---

const ErrorMessage: React.FC<{ error: string }> = ({ error }) => {
  if (!error) return null;
  return <p className="error">{error}</p>;
};

const ProviderSelector: React.FC<{ provider: string; setProvider: (p: string) => void; availableProviders: Provider[] }> = ({ provider, setProvider, availableProviders }) => {
  if (availableProviders.length === 0) return null;
  return (
    <div>
      <label htmlFor="provider-select">Choose a wallet provider: </label>
      <select id="provider-select" value={provider} onChange={(e) => setProvider(e.target.value)}>
        {availableProviders.map((p) => (
          <option key={p.id} value={p.id}>{p.name}</option>
        ))}
      </select>
    </div>
  );
};

const AddressInfo: React.FC<{ address: ComposeAddressEntry; utxos: Utxo[] }> = ({ address, utxos }) => (
  <div className="addresses">
    <h2>Your Taproot Address:</h2>
    <ul><li>{address.address}</li></ul>
    {utxos.length > 0 && (
      <div className="utxos">
        <h3>UTXOs:</h3>
        <ul>
          {utxos.map((utxo, index) => (
            <li key={index}>
              <strong>TXID:</strong> {utxo.txid}<br />
              <strong>Vout:</strong> {utxo.vout}<br />
              <strong>Value:</strong> {utxo.value / SATS_PER_BTC} BTC<br />
              <strong>Status:</strong> {utxo.status.confirmed ? 'Confirmed' : 'Unconfirmed'}
            </li>
          ))}
        </ul>
      </div>
    )}
  </div>
);

const TransactionDetails: React.FC<{ tx: Transaction, title: string }> = ({ tx, title }) => (
  <>
    <h3>{title}:</h3>
    <div className="transaction-details">
      <p><strong>Version:</strong> {tx.version}</p>
      <p><strong>Lock Time:</strong> {tx.lock_time}</p>
      <h4>Inputs:</h4>
      <ul>
        {tx.input.map((input, index) => (
          <li key={index}>
            <strong>Previous Output:</strong> {input.previous_output}<br />
            <strong>Sequence:</strong> {input.sequence}
          </li>
        ))}
      </ul>
      <h4>Outputs:</h4>
      <ul>
        {tx.output.map((output, index) => (
          <li key={index}>
            <strong>Script Pubkey:</strong> {output.script_pubkey}<br />
            <strong>Value:</strong> {output.value / SATS_PER_BTC} BTC
          </li>
        ))}
      </ul>
    </div>
  </>
);

const ComposeResultDisplay: React.FC<{ composeResult: ComposeResult }> = ({ composeResult }) => (
  <div className="transactions">
    <TransactionDetails tx={composeResult.commit_transaction} title="Commit Transaction" />
    <TransactionDetails tx={composeResult.reveal_transaction} title="Reveal Transaction" />
    <h3>Tap Script:</h3>
    <p className="tap-script">{composeResult.tap_script}</p>
  </div>
);

const Composer: React.FC<{ address: ComposeAddressEntry, utxos: Utxo[], inputData: string; setInputData: (d: string) => void; onCompose: () => void; }> =
  ({ address, utxos, inputData, setInputData, onCompose }) => (
    <div className="compose-section">
      <div className="input-container">
        <input
          type="text"
          value={inputData}
          onChange={(e) => setInputData(e.target.value)}
          placeholder="Enter data to encode"
          className="data-input"
          style={{ width: '100%', padding: '12px', marginBottom: '16px', fontSize: '16px', borderRadius: '4px', border: '1px solid #ccc' }}
        />
      </div>
      <button onClick={onCompose}>Compose Commit/Reveal Transactions</button>
    </div>
  );

const Signer: React.FC<{ signedTx: string, onSign: () => void, onBroadcast: () => void }> = ({ signedTx, onSign, onBroadcast }) => (
  <div className="sign-transaction">
    <button onClick={onSign}>Sign Transactions</button>
    {signedTx && (
      <>
        <div className="signed-transaction">
          <h3>Signed Transactions (Commit, Reveal):</h3>
          <p className="tx-hex">{signedTx}</p>
        </div>
        <button onClick={onBroadcast}>Broadcast Transactions</button>
      </>
    )}
  </div>
);

const BroadcastResultDisplay: React.FC<{ broadcastedTx: TestMempoolAcceptResult[] }> = ({ broadcastedTx }) => {
  if (broadcastedTx.length === 0) return null;
  return (
    <div className="broadcasted-transaction">
      <h3>Broadcasted Transaction Result:</h3>
      <ul>
        {broadcastedTx.map((tx, index) => (
          <li key={index}>
            <strong>TXID:</strong> {tx.txid}
            <p>Allowed: {tx.allowed ? 'Yes' : 'No'}</p>
            {tx.reject_reason && <p>Reject Reason: {tx.reject_reason}</p>}
            {tx.vsize && <p>Vsize: {tx.vsize}</p>}
            {tx.fee && <p>Fee: {tx.fee}</p>}
          </li>
        ))}
      </ul>
    </div>
  );
};


// --- Main Wallet Component ---
function WalletComponent() {
  const [address, setAddress] = useState<ComposeAddressEntry | undefined>()
  const [utxos, setUtxos] = useState<Utxo[]>([])
  const [composeResult, setComposeResult] = useState<ComposeResult | undefined>()
  const [error, setError] = useState<string>('')
  const [signedTx, setSignedTx] = useState<string>('');
  const [broadcastedTx, setBroadcastedTx] = useState<TestMempoolAcceptResult[]>([])
  const [inputData, setInputData] = useState<string>('')
  const [provider, setProvider] = useState<string>('')
  const [availableProviders, setAvailableProviders] = useState<Provider[]>([]);

  useEffect(() => {
    const providers = getProviders();
    if (window.unisat && !providers.find(p => p.id === PROVIDER_ID_UNISAT)) {
      providers.push({ id: PROVIDER_ID_UNISAT, name: 'UniSat', icon: '' });
    }

    if (providers?.length > 0) {
      setAvailableProviders(providers);
      if (!provider) {
        setProvider(providers[0].id);
      }
    }
  }, [provider]);

  const handleGetAddresses = async () => {
    setError('');
    if (provider === PROVIDER_ID_UNISAT && window.unisat) {
      try {
        const accounts = await window.unisat.requestAccounts();
        const publicKey = await window.unisat.getPublicKey();
        if (accounts.length > 0) {
          const paymentAddress: ComposeAddressEntry = { address: accounts[0], publicKey };
          setAddress(paymentAddress);
          handleFetchUtxos(paymentAddress.address);
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : 'An unknown error occurred with UniSat.');
      }
      return;
    }

    try {
      const getAddresses = () => satsConnectRequest('getAddresses', { purposes: [AddressPurpose.Payment, AddressPurpose.Ordinals, AddressPurpose.Stacks] }, provider);
      let response = await getAddresses();

      if (response.status === 'error') {
        if (response.error.code === RpcErrorCode.ACCESS_DENIED) {
          await satsConnectRequest('wallet_requestPermissions', undefined, provider);
          response = await getAddresses();
        } else {
          throw new Error(response.error.message || 'Failed to get addresses.');
        }
      }

      if (response.status === 'success') {
        const paymentAddress = (response.result.addresses as (XverseAddressEntry | LeatherAddressEntry)[]).find(
          addr => (addr as XverseAddressEntry).addressType === ADDRESS_TYPE_P2TR || (addr as LeatherAddressEntry).type === ADDRESS_TYPE_P2TR
        );

        if (paymentAddress) {
          const composeAddress: ComposeAddressEntry = {
            address: paymentAddress.address,
            publicKey: (paymentAddress as LeatherAddressEntry).tweakedPublicKey || paymentAddress.publicKey,
          };
          setAddress(composeAddress);
          handleFetchUtxos(composeAddress.address);
        } else {
          setError('Could not find a P2TR (Taproot) payment address.');
        }
      } else {
        setError(response.error?.message || 'Failed to get addresses.');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to get addresses or UTXOs');
    }
  }

  const handleFetchUtxos = async (addr: string) => {
    try {
      const utxoData = await fetchUtxosFromApi(addr);
      setUtxos(utxoData);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'An unknown error occurred while fetching UTXOs.');
    }
  };

  const handleCompose = async () => {
    if (!address || utxos.length === 0) return;
    setError('');
    try {
      const kontorData = await composeTransactionOnApi(address, utxos, inputData);
      setComposeResult(kontorData);
    } catch (err) {
      setError(err instanceof Error ? `Failed to compose transaction: ${err.message}` : 'An unknown error occurred during composition.');
    }
  }

  const handleSignTransaction = async () => {
    if (!address || !composeResult) return;
    setError('');
    try {
      bitcoin.initEccLib(ecc);
      ECPairFactory(ecc);

      const commitSignResult = await signPsbt(composeResult.commit_psbt_hex, address.address, provider);
      const revealSignResult = await signPsbt(composeResult.reveal_psbt_hex, address.address, provider, composeResult.tap_leaf_script);

      setSignedTx([commitSignResult, revealSignResult].join(','));
    } catch (err) {
      console.log('err', err)
      setError(err instanceof Error ? `Failed to sign transaction: ${err.message}` : 'An unknown error occurred during signing.');
    }
  };

  const handleBroadcastTransaction = async () => {
    if (!signedTx) return;
    setError('');
    try {
      const result = await broadcastTransactionOnApi(signedTx);
      setBroadcastedTx(result);
    } catch (err) {
      setError(err instanceof Error ? `Failed to broadcast: ${err.message}` : 'An unknown error occurred while broadcasting.');
    }
  }

  return (
    <div className="wallet-container">
      <h1>COMPOSE</h1>
      <ProviderSelector provider={provider} setProvider={setProvider} availableProviders={availableProviders} />
      <button onClick={handleGetAddresses}>Get Wallet Addresses</button>

      <ErrorMessage error={error} />

      {address && <AddressInfo address={address} utxos={utxos} />}

      {composeResult && <ComposeResultDisplay composeResult={composeResult} />}

      {!composeResult && address && utxos.length > 0 && (
        <Composer
          address={address}
          utxos={utxos}
          inputData={inputData}
          setInputData={setInputData}
          onCompose={handleCompose}
        />
      )}

      {composeResult && (
        <Signer
          signedTx={signedTx}
          onSign={handleSignTransaction}
          onBroadcast={handleBroadcastTransaction}
        />
      )}

      <BroadcastResultDisplay broadcastedTx={broadcastedTx} />
    </div>
  )
}

function App() {
  return <WalletComponent />
}

export default App


