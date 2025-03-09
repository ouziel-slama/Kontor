# Kontor

## Installation

Install [ZeroMQ](https://zeromq.org/download/)

Create TLS certs for local development with [mkcert](https://github.com/FiloSottile/mkcert). In the project directory run:
```
mkcert -key-file key.pem -cert-file cert.pem localhost 127.0.0.1 ::1
```

Specify options:
```
--bitcoin-rpc-url <BITCOIN_RPC_URL>
    URL of the Bitcoin RPC server (e.g., http://localhost:8332)

    [env: BITCOIN_RPC_URL=https://api.unspendablelabs.com:8332]

--bitcoin-rpc-user <BITCOIN_RPC_USER>
    User for Bitcoin RPC authentication

    [env: BITCOIN_RPC_USER=rpc]

--bitcoin-rpc-password <BITCOIN_RPC_PASSWORD>
    Password for Bitcoin RPC authentication

    [env: BITCOIN_RPC_PASSWORD=rpc]

--zmq-pub-sequence-address <ZMQ_PUB_SEQUENCE_ADDRESS>
    ZMQ address for sequence notifications (e.g., tcp://localhost:28332)

    [env: ZMQ_PUB_SEQUENCE_ADDRESS=tcp://127.0.0.1:28332]

--api-port <API_PORT>
    Port number for the API server (e.g., 8080)

    [env: API_PORT=8443]

--cert-dir <CERT_DIR>
    Directory path for TLS cert.pem and key.pem files (e.g., /var/lib/myapp/certs)

    [env: CERT_DIR=./]

--database-dir <DATABASE_DIR>
    Directory path for the database (e.g., /var/lib/myapp/db)

    [env: DATABASE_DIR=./]

--starting-block-height <STARTING_BLOCK_HEIGHT>
    Block height to begin parsing at (e.g. 850000)

    [env: STARTING_BLOCK_HEIGHT=]
    [default: 850000]
```

# Test

```
cargo test
```

Tests expect options to be set.

# Run

```
cargo run
```
