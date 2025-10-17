use testlib::*;

interface!(name = "arith", path = "../contracts/arith/wit",);

interface!(name = "fib", path = "../contracts/fib/wit",);

interface!(name = "proxy", path = "../contracts/proxy/wit",);

#[runtime(contracts_dir = "../../contracts")]
async fn test_fib_contract() -> Result<()> {
    let signer = runtime.identity().await?;
    let fib = runtime.publish(&signer, "fib").await?;
    let arith = runtime.publish(&signer, "arith").await?;
    let proxy = runtime.publish(&signer, "proxy").await?;

    let result = arith::last_op(runtime, &arith).await?;
    assert_eq!(result, Some(arith::Op::Id));

    let n = 8;
    let result = fib::fib(runtime, &fib, &signer, n).await?;
    assert_eq!(result, 21);

    let last_op = Some(arith::Op::Sum(arith::Operand { y: 8 }));
    let result = arith::last_op(runtime, &arith).await?;
    assert_eq!(result, last_op);

    let result = fib::fib(runtime, &proxy, &signer, n).await?;
    assert_eq!(result, 21);

    let result = proxy::get_contract_address(runtime, &proxy).await?;
    assert_eq!(result, fib);

    proxy::set_contract_address(runtime, &proxy, &signer, arith.clone()).await?;

    let result = arith::last_op(runtime, &proxy).await?;
    assert_eq!(result, Some(arith::Op::Sum(arith::Operand { y: 8 })));

    // result
    let x = "5";
    let y = "3";
    let result = arith::checked_sub(runtime, &arith, x, y).await?;
    assert_eq!(result, Ok(2));

    let result = arith::checked_sub(runtime, &arith, y, x).await?;
    assert_eq!(result, Err(Error::Message("less than 0".to_string())));

    // result through import
    let x = "18";
    let y = "10";
    let result = fib::fib_of_sub(runtime, &fib, &signer, x, y).await?;
    assert_eq!(result, Ok(21));

    let result = fib::fib_of_sub(runtime, &fib, &signer, y, x).await?;
    assert_eq!(result, Err(Error::Message("less than 0".to_string())));

    // reentrancy prevented
    let result = arith::fib(runtime, &arith, &signer, 9).await;
    assert!(result.is_err_and(|e| e.root_cause().to_string().contains("reentrancy prevented")));

    let result = fib::cached_values(runtime, &fib).await?;
    assert_eq!(result, vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);

    Ok(())
}
