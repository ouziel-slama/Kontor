use testlib::*;

import!(
    name = "storage_agreement",
    height = 0,
    tx_index = 0,
    path = "../../native-contracts/storage-agreement/wit",
);

fn make_descriptor(file_id: String, root: Vec<u8>, depth: u64) -> RawFileDescriptor {
    RawFileDescriptor {
        file_id,
        root,
        depth,
    }
}

async fn prepare_real_descriptor() -> Result<RawFileDescriptor> {
    let root: Vec<u8> = [0u8; 32].to_vec();
    let depth: u64 = 4;
    Ok(make_descriptor("test_file".to_string(), root, depth))
}

async fn storage_agreement_create_and_get(runtime: &mut Runtime) -> Result<()> {
    let signer = runtime.identity().await?;
    let descriptor = prepare_real_descriptor().await?;

    let created =
        storage_agreement::create_agreement(runtime, &signer, descriptor.clone()).await??;
    assert_eq!(created.agreement_id, descriptor.file_id);

    let got = storage_agreement::get_agreement(runtime, created.agreement_id.as_str()).await?;
    let got = got.expect("agreement should exist");

    assert_eq!(got.agreement_id, created.agreement_id);
    assert_eq!(got.file_id, descriptor.file_id);
    assert_eq!(got.root, descriptor.root);
    assert_eq!(got.depth, descriptor.depth);
    assert!(!got.active);
    Ok(())
}

async fn storage_agreement_count_increments(runtime: &mut Runtime) -> Result<()> {
    let signer = runtime.identity().await?;

    let c0 = storage_agreement::agreement_count(runtime).await?;
    let d1 = prepare_real_descriptor().await?;
    storage_agreement::create_agreement(runtime, &signer, d1).await??;
    let c1 = storage_agreement::agreement_count(runtime).await?;
    assert_eq!(c1, c0 + 1);

    let d2 = make_descriptor("another_file".to_string(), vec![7u8; 32], 8);
    storage_agreement::create_agreement(runtime, &signer, d2).await??;
    let c2 = storage_agreement::agreement_count(runtime).await?;
    assert_eq!(c2, c1 + 1);

    Ok(())
}

async fn storage_agreement_duplicate_fails(runtime: &mut Runtime) -> Result<()> {
    let signer = runtime.identity().await?;
    let descriptor = make_descriptor("dup_file".to_string(), vec![1u8; 32], 8);

    storage_agreement::create_agreement(runtime, &signer, descriptor.clone()).await??;
    let err = storage_agreement::create_agreement(runtime, &signer, descriptor).await?;
    assert!(matches!(err, Err(Error::Message(_))));
    Ok(())
}

async fn storage_agreement_invalid_root_fails(runtime: &mut Runtime) -> Result<()> {
    let signer = runtime.identity().await?;
    let descriptor = make_descriptor("bad_root".to_string(), vec![1u8; 31], 8);

    let err = storage_agreement::create_agreement(runtime, &signer, descriptor).await?;
    assert!(matches!(err, Err(Error::Validation(_))));
    Ok(())
}

async fn storage_agreement_zero_depth_fails(runtime: &mut Runtime) -> Result<()> {
    let signer = runtime.identity().await?;
    let descriptor = make_descriptor("zero_depth".to_string(), vec![1u8; 32], 0);

    let err = storage_agreement::create_agreement(runtime, &signer, descriptor).await?;
    assert!(matches!(err, Err(Error::Message(_))));
    Ok(())
}

#[testlib::test(contracts_dir = "../../test-contracts")]
async fn test_storage_agreement_create_and_get() -> Result<()> {
    storage_agreement_create_and_get(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts")]
async fn test_storage_agreement_count_increments() -> Result<()> {
    storage_agreement_count_increments(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts")]
async fn test_storage_agreement_duplicate_fails() -> Result<()> {
    storage_agreement_duplicate_fails(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts")]
async fn test_storage_agreement_invalid_root_fails() -> Result<()> {
    storage_agreement_invalid_root_fails(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts")]
async fn test_storage_agreement_zero_depth_fails() -> Result<()> {
    storage_agreement_zero_depth_fails(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts", mode = "regtest")]
async fn test_storage_agreement_create_and_get_regtest() -> Result<()> {
    storage_agreement_create_and_get(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts", mode = "regtest")]
async fn test_storage_agreement_count_increments_regtest() -> Result<()> {
    storage_agreement_count_increments(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts", mode = "regtest")]
async fn test_storage_agreement_duplicate_fails_regtest() -> Result<()> {
    storage_agreement_duplicate_fails(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts", mode = "regtest")]
async fn test_storage_agreement_invalid_root_fails_regtest() -> Result<()> {
    storage_agreement_invalid_root_fails(runtime).await
}

#[testlib::test(contracts_dir = "../../test-contracts", mode = "regtest")]
async fn test_storage_agreement_zero_depth_fails_regtest() -> Result<()> {
    storage_agreement_zero_depth_fails(runtime).await
}
