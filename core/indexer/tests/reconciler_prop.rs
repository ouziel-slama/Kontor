use anyhow::{anyhow, Error, Result};
use rand::prelude::*;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;
use proptest::test_runner::{Config, TestRunner};

use bitcoin::{BlockHash, hashes::Hash};

use proptest::prelude::*;

use indexer::{
    bitcoin_follower::{
        ctrl::CtrlChannel,
        info,
        rpc,
        reconciler,
        events::{Event, ZmqEvent},
    },
    logging,
    block::Block,
    utils::MockTransaction,
};

#[derive(Debug)]
enum Segment {
    RpcSeries(usize),
    AppendBlocks(usize),
    ZmqConnection(bool),
    ZmqSeries(usize),
    Rewind(usize),
}

#[derive(Debug)]
enum Step {
    RpcEvent((u64, Block<MockTransaction>)),
    AppendBlocks(Vec<Block<MockTransaction>>),
    ZmqEvent(ZmqEvent<MockTransaction>),
}

fn gen_block(height: u64, prev_hash: Option<BlockHash>) -> Block<MockTransaction> {
    let mut hash = [0u8; 32];
    rand::rng().fill_bytes(&mut hash);

    let prev = match prev_hash {
        Some(h) => h,
        None => BlockHash::from_byte_array([0x00; 32]),
    };

    Block {
        height,
        hash: BlockHash::from_byte_array(hash),
        prev_hash: prev,
        transactions: vec![],
    }
}

fn gen_blocks(start: u64, end: u64, prev_hash: Option<BlockHash>) -> Vec<Block<MockTransaction>> {
    let mut blocks = vec![];
    let mut prev = prev_hash;

    for _i in start..end {
        let block = gen_block(_i + 1, prev);
        prev = Some(block.hash);
        blocks.push(block.clone());
    }

    blocks
}

fn new_block_chain(n: u64) -> Vec<Block<MockTransaction>> {
    gen_blocks(0, n, None)
}

fn gen_segment() -> impl Strategy<Value = Segment> {
    prop_oneof![
        1 => (1..4usize).prop_map(Segment::RpcSeries),
        1 => (1..4usize).prop_map(Segment::ZmqSeries),
        1 => (1..2usize).prop_map(Segment::AppendBlocks),
        1 => prop::bool::ANY.prop_map(Segment::ZmqConnection),
        1 => (1..2usize).prop_map(Segment::Rewind),
    ]
}

fn gen_segment_vec() -> impl Strategy<Value = Vec<Segment>> {
    prop::collection::vec(gen_segment(), 1..10)
}

fn create_steps(segs: Vec<Segment>) -> (Vec<Step>, MockBlockchain) {
    let mut blocks = new_block_chain(5);
    let mut stream = vec![];
    let mut height = 0;
    for seg in segs.iter() {
        match seg {
            Segment::RpcSeries(n) => {
                for _i in 0..*n {
                    if height < blocks.len() {
                        stream.push(Step::RpcEvent((blocks.len() as u64, blocks[height].clone())));
                        height += 1;
                    }
                }
            }
            Segment::ZmqSeries(n) => {
                for _i in 0..*n {
                    if height < blocks.len() {
                        stream.push(Step::ZmqEvent(ZmqEvent::BlockConnected(blocks[height].clone())));
                        height += 1;
                    }
                }
            }
            Segment::AppendBlocks(n) => {
                let cnt = blocks.len();
                let more_blocks = gen_blocks(cnt as u64, (cnt+n) as u64, Some(blocks[cnt - 1].hash));
                blocks.extend(more_blocks.iter().cloned());
                stream.push(Step::AppendBlocks(more_blocks));
            }
            Segment::ZmqConnection(connected) => {
                if *connected {
                    stream.push(Step::ZmqEvent(ZmqEvent::Connected));
                } else {
                    stream.push(Step::ZmqEvent(ZmqEvent::Disconnected(anyhow!("mock error"))));
                }
            }
            Segment::Rewind(n) => {
                if height < *n {
                    height = 0;
                } else {
                    height -= n;
                }
            }
        }
    }
    (stream, MockBlockchain::new(blocks))
}

#[derive(Clone, Debug)]
struct State {
    start_height: u64,
    running: bool,
    blocks: Vec<Block<MockTransaction>>,
}

#[derive(Clone, Debug)]
struct MockBlockchain {
    state: Arc<Mutex<State>>,
}

impl MockBlockchain {
    fn new(blocks: Vec<Block<MockTransaction>>) -> Self {
        Self {
            state: Mutex::new(State{
                start_height: 0, 
                running: false,
                blocks,
            }).into(),
        }
    }

    fn append_blocks(&mut self, more_blocks: Vec<Block<MockTransaction>>) {
        let mut state = self.state.lock().unwrap();
        state.blocks.extend(more_blocks.iter().cloned());
    }

    async fn await_running(&self) {
        loop {
            if self.state.lock().unwrap().running {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    }

}

impl rpc::BlockFetcher for MockBlockchain {
    fn running(&self) -> bool {
        self.state.lock().unwrap().running
    }

    fn start(&mut self, start_height: u64) {
        let mut state = self.state.lock().unwrap();

        state.running = true;
        state.start_height = start_height;
    }

    async fn stop(&mut self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.running = false;
        Ok(())
    }
}

impl info::BlockchainInfo for MockBlockchain {
    async fn get_blockchain_height(&self) -> Result<u64, Error> {
        let state = self.state.lock().unwrap();
        Ok(state.blocks.len() as u64)
    }

    async fn get_block_hash(&self, height: u64) -> Result<BlockHash, Error> {
        let state = self.state.lock().unwrap();
        Ok(state.blocks[height as usize - 1].hash)
    }
}

#[test]
fn single_run_example() {
    logging::setup();
    let config = Config::default();
    let mut runner = TestRunner::new(config);
    let strategy = gen_segment_vec();
    let test_case = strategy.new_tree(&mut runner).unwrap(); // Get a specific test case
    let result = runner.run_one(test_case, |a| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cancel_token = CancellationToken::new();
            let (ctrl, ctrl_rx) = CtrlChannel::<MockTransaction>::create();
            let (rpc_tx, rpc_rx) = mpsc::channel(100);
            let (zmq_tx, zmq_rx) = mpsc::unbounded_channel::<ZmqEvent<MockTransaction>>();

            let (steps, mut mock) = create_steps(a);

            let mut rec = reconciler::Reconciler::new(
                cancel_token.clone(),
                mock.clone(),
                mock.clone(),
                rpc_rx,
                zmq_rx,
            );

            let handle = tokio::spawn(async move {
                rec.run(ctrl_rx).await;
            });

            let mut event_rx = ctrl.clone().start(1, None).await.unwrap();
            mock.await_running().await;
            
            for step in steps {
               println!("steps {:?}", step);
                match step {
                    Step::RpcEvent((target, block)) => {
                        assert!(
                            rpc_tx
                                .send((target, block))
                                .await
                                .is_ok()
                        );
                    },
                    Step::ZmqEvent(e) => {
                        assert!(zmq_tx.send(e).is_ok());
                    },
                    Step::AppendBlocks(blocks) => {
                        mock.append_blocks(blocks);
                    },
                }
            }

            sleep(Duration::from_millis(10)).await;

            let mut expected_height = 1;
            while !event_rx.is_empty() {
                match event_rx.recv().await {
                    Some(Event::BlockInsert((_target_height, block))) => {
                        println!("received {:?}", block);
                        let height = block.height;
                        if height != expected_height {
                            println!("unexpected {:?}", block);
                        }
                        expected_height = height + 1;
                    },
                    Some(_) => {},
                    None => {},
                }
            }

            assert!(!handle.is_finished());
            cancel_token.cancel();
            let _ = handle.await;
            Ok(())
        })
    });

    match result {
        Ok(true) => println!("Test passed"),
        Ok(false) => println!("Test rejected"),
        Err(e) => println!("Test failed: {:?}", e),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        timeout: 5000,
        .. ProptestConfig::default()
    })]

    #[test]

    /**
    TEST DESIGN

    The test generates a valid blockchain which will be exposed to the Reconciler as
    series of sequential blocks either arriving via RPC or ZMQ. It will intersperse
    changes in ZMQ connection status, growth in the underlying blockchain and rewinds
    of the height blocks will be sent at.

    MockBlockchain implements BlockFetcher and BlockchainInfo info, and the info shared 
    by the latter will be kept in sync with the blockchain blocks are sent from.


    TEST DATA AND MODEL NOTES
     - Gaps in the stream of blocks via RPC will get passed through and lead to gaps
       in emitted `BlockInsert` events.
     - There's no feedback from the Reconciler to the stream of events. Specifically
       the `start` signal on the BlockFetcher is ignored, meaning the height of the
       blocks received via RPC will not correspond to those requested.

    */
    fn test_reactor_rollbacks(vec in gen_segment_vec()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cancel_token = CancellationToken::new();
            let (ctrl, ctrl_rx) = CtrlChannel::<MockTransaction>::create();
            let (rpc_tx, rpc_rx) = mpsc::channel(100);
            let (zmq_tx, zmq_rx) = mpsc::unbounded_channel::<ZmqEvent<MockTransaction>>();

            let (steps, mut mock) = create_steps(vec);

            let mut rec = reconciler::Reconciler::new(
                cancel_token.clone(),
                mock.clone(),
                mock.clone(),
                rpc_rx,
                zmq_rx,
            );

            let handle = tokio::spawn(async move {
                rec.run(ctrl_rx).await;
            });

            let mut event_rx = ctrl.clone().start(1, None).await.unwrap();
            
            for step in steps {
     //           println!("steps {:?}", step);
                match step {
                    Step::RpcEvent((target, block)) => {
                        assert!(
                            rpc_tx
                                .send((target, block))
                                .await
                                .is_ok()
                        );
                    },
                    Step::ZmqEvent(e) => {
                        assert!(zmq_tx.send(e).is_ok());
                    },
                    Step::AppendBlocks(blocks) => {
                        mock.append_blocks(blocks);
                    },
                }
            }

            sleep(Duration::from_millis(10)).await;

            let mut expected_height = 1;
            while !event_rx.is_empty() {
                match event_rx.recv().await {
                    Some(Event::BlockInsert((_target_height, block))) => {
                        println!("received {:?}", block);
                        let height = block.height;
                        if height != expected_height {
                            println!("unexpected {:?}", block);
                        }
                        expected_height = height + 1;
                    },
                    Some(_) => {},
                    None => {},
                }
            }


            assert!(!handle.is_finished());
            cancel_token.cancel();
            let _ = handle.await;
        })
    }
}
