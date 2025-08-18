#![allow(dead_code)]

macros::contract!(name = "fib");

macros::import!(name = "arith", height = 0, tx_index = 0, path = "arith/wit");

impl arith_next::Operand {
    pub fn wave_type() -> wasm_wave::value::Type {
        wasm_wave::value::Type::record([("y", wasm_wave::value::Type::U64)]).unwrap()
    }
}

mod arith {
    use wasm_wave::wasm::WasmValue as _;

    use super::context;
    use super::foreign;

    const CONTRACT_NAME: &str = "arith";

    #[derive(Clone)]
    pub struct Operand {
        pub y: u64,
    }

    impl Operand {
        pub fn wave_type() -> wasm_wave::value::Type {
            wasm_wave::value::Type::record([("y", wasm_wave::value::Type::U64)]).unwrap()
        }
    }

    impl From<Operand> for wasm_wave::value::Value {
        fn from(value_: Operand) -> Self {
            wasm_wave::value::Value::make_record(
                &Operand::wave_type(),
                [("y", wasm_wave::value::Value::from(value_.y))],
            )
            .unwrap()
        }
    }

    impl From<wasm_wave::value::Value> for Operand {
        fn from(value_: wasm_wave::value::Value) -> Self {
            let mut y = None;

            for (key_, val_) in value_.unwrap_record() {
                match key_.as_ref() {
                    "y" => y = Some(val_.unwrap_u64()),
                    key_ => panic!("Unknown field: {key_}"),
                }
            }

            Self {
                y: y.expect("Missing 'y' field"),
            }
        }
    }

    #[derive(Clone)]
    pub enum Op {
        Id,
        Sum(Operand),
        Mul(Operand),
        Div(Operand),
    }

    impl Op {
        pub fn wave_type() -> wasm_wave::value::Type {
            wasm_wave::value::Type::variant([
                ("id", None),
                ("sum", Some(Operand::wave_type())),
                ("mul", Some(Operand::wave_type())),
                ("div", Some(Operand::wave_type())),
            ])
            .unwrap()
        }
    }

    impl From<Op> for wasm_wave::value::Value {
        fn from(value_: Op) -> Self {
            match value_ {
                Op::Id => wasm_wave::value::Value::make_variant(&Op::wave_type(), "id", None),
                Op::Sum(operand) => wasm_wave::value::Value::make_variant(
                    &Op::wave_type(),
                    "sum",
                    Some(wasm_wave::value::Value::from(operand)),
                ),
                Op::Mul(operand) => wasm_wave::value::Value::make_variant(
                    &Op::wave_type(),
                    "mul",
                    Some(wasm_wave::value::Value::from(operand)),
                ),
                Op::Div(operand) => wasm_wave::value::Value::make_variant(
                    &Op::wave_type(),
                    "div",
                    Some(wasm_wave::value::Value::from(operand)),
                ),
            }
            .unwrap()
        }
    }

    impl From<wasm_wave::value::Value> for Op {
        fn from(value_: wasm_wave::value::Value) -> Self {
            let (key_, val_) = value_.unwrap_variant();
            match key_ {
                key_ if key_.eq("id") => Op::Id,
                key_ if key_.eq("sum") => Op::Sum(val_.unwrap().into_owned().into()),
                key_ if key_.eq("mul") => Op::Mul(val_.unwrap().into_owned().into()),
                key_ if key_.eq("div") => Op::Div(val_.unwrap().into_owned().into()),
                key_ => panic!("Unknown tag {key_}"),
            }
        }
    }

    #[derive(Clone)]
    pub struct ArithReturn {
        pub value: u64,
    }

    impl ArithReturn {
        pub fn wave_type() -> wasm_wave::value::Type {
            wasm_wave::value::Type::record([("value", wasm_wave::value::Type::U64)]).unwrap()
        }
    }

    impl From<ArithReturn> for wasm_wave::value::Value {
        fn from(value_: ArithReturn) -> Self {
            wasm_wave::value::Value::make_record(
                &ArithReturn::wave_type(),
                [("value", wasm_wave::value::Value::from(value_.value))],
            )
            .unwrap()
        }
    }

    impl From<wasm_wave::value::Value> for ArithReturn {
        fn from(value_: wasm_wave::value::Value) -> Self {
            let mut value = None;

            for (key_, val_) in value_.unwrap_record() {
                match key_.as_ref() {
                    "value" => value = Some(val_.unwrap_u64()),
                    key_ => panic!("Unknown field: {key_}"),
                }
            }
            ArithReturn {
                value: value.expect("Missing 'value' field"),
            }
        }
    }

    pub fn eval(ctx: &context::ProcContext, x: u64, op: Op) -> ArithReturn {
        let expr = format!(
            "eval({}, {})",
            &wasm_wave::to_string(&wasm_wave::value::Value::from(x)).unwrap(),
            &wasm_wave::to_string(&wasm_wave::value::Value::from(op)).unwrap()
        );
        let ret = foreign::call(
            &foreign::ContractAddress {
                name: CONTRACT_NAME.to_string(),
                height: 0,
                tx_index: 0,
            },
            Some(&ctx.signer()),
            expr.as_str(),
        );
        wasm_wave::from_str::<wasm_wave::value::Value>(&ArithReturn::wave_type(), &ret)
            .unwrap()
            .into()
    }
}

// #[storage]
#[derive(Clone, Store, Wrapper)]
struct FibValue {
    pub value: u64,
}

#[derive(Clone, Store, Wrapper, Root)]
struct FibStorage {
    pub cache: Map<u64, FibValue>,
}

impl Fib {
    fn raw_fib(ctx: &ProcContext, n: u64) -> u64 {
        let cache = storage(ctx).cache();
        if let Some(v) = cache.get(ctx, n).map(|v| v.value(ctx)) {
            return v;
        }

        let value = match n {
            0 | 1 => n,
            _ => {
                arith::eval(
                    ctx,
                    Self::raw_fib(ctx, n - 1),
                    arith::Op::Sum(arith::Operand {
                        y: Self::raw_fib(ctx, n - 2),
                    }),
                )
                .value
            }
        };
        cache.set(ctx, n, FibValue { value });
        value
    }
}

impl Guest for Fib {
    fn fallback(ctx: &FallContext, expr: String) -> String {
        format!("{:?}:{}", ctx.signer().map(|s| s.to_string()), expr)
    }

    fn init(ctx: &ProcContext) {
        FibStorage {
            cache: Map::new(&[(0, FibValue { value: 0 })]),
        }
        .init(ctx);
    }

    fn fib(ctx: &ProcContext, n: u64) -> u64 {
        Self::raw_fib(ctx, n)
    }
}

export!(Fib);
