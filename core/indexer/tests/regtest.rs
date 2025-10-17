use anyhow::{Context, Result};
use indexer::{logging, reactor::types::Inst, reg_tester::RegTester};
use testlib::ContractAddress;
use tracing::info;

async fn run_test_regtest(reg_tester: &mut RegTester) -> Result<()> {
    let mut alice = reg_tester.identity("alice").await?;
    let expr = reg_tester
        .instruction(
            &mut alice,
            Inst::Publish {
                name: "test".to_string(),
                bytes: b"test".to_vec(),
            },
        )
        .await
        .context("Failed to publish contract")?;
    let address: ContractAddress =
        wasm_wave::from_str::<wasm_wave::value::Value>(&ContractAddress::wave_type(), &expr)?
            .into();
    info!("Contract Address: {}", address);
    Ok(())
}

#[tokio::test]
async fn test_regtest() -> Result<()> {
    logging::setup();
    let (
        _bitcoin_data_dir,
        bitcoin_child,
        bitcoin_client,
        _kontor_data_dir,
        kontor_child,
        kontor_client,
        identity,
    ) = RegTester::setup().await?;
    let result = tokio::spawn({
        let bitcoin_client = bitcoin_client.clone();
        let kontor_client = kontor_client.clone();
        async move {
            let mut reg_tester = RegTester::new(identity, bitcoin_client, kontor_client).await?;
            run_test_regtest(&mut reg_tester).await
        }
    })
    .await;
    RegTester::teardown(bitcoin_client, bitcoin_child, kontor_client, kontor_child).await?;
    result?
}
