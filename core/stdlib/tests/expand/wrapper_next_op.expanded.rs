use stdlib::WrapperNext;
pub enum Op {
    Id,
    Sum(Operand),
    Mul(Operand),
    Div(Operand),
}
pub enum OpWrapperNext<'a> {
    Id,
    Sum(OperandWrapperNext<'a>),
    Mul(OperandWrapperNext<'a>),
    Div(OperandWrapperNext<'a>),
}
#[automatically_derived]
impl<'a> ::core::clone::Clone for OpWrapperNext<'a> {
    #[inline]
    fn clone(&self) -> OpWrapperNext<'a> {
        match self {
            OpWrapperNext::Id => OpWrapperNext::Id,
            OpWrapperNext::Sum(__self_0) => {
                OpWrapperNext::Sum(::core::clone::Clone::clone(__self_0))
            }
            OpWrapperNext::Mul(__self_0) => {
                OpWrapperNext::Mul(::core::clone::Clone::clone(__self_0))
            }
            OpWrapperNext::Div(__self_0) => {
                OpWrapperNext::Div(::core::clone::Clone::clone(__self_0))
            }
        }
    }
}
impl<'a> OpWrapperNext<'a> {
    pub fn new(ctx: &'a crate::ProcContext, base_path: stdlib::DotPathBuf) -> Self {
        ctx.__extend_path_with_match(&base_path, &["id", "sum", "mul", "div"])
            .map(|path| match path {
                p if p.starts_with(base_path.push("id").as_ref()) => OpWrapperNext::Id,
                p if p.starts_with(base_path.push("sum").as_ref()) => {
                    OpWrapperNext::Sum(
                        OperandWrapperNext::new(ctx, base_path.push("sum")),
                    )
                }
                p if p.starts_with(base_path.push("mul").as_ref()) => {
                    OpWrapperNext::Mul(
                        OperandWrapperNext::new(ctx, base_path.push("mul")),
                    )
                }
                p if p.starts_with(base_path.push("div").as_ref()) => {
                    OpWrapperNext::Div(
                        OperandWrapperNext::new(ctx, base_path.push("div")),
                    )
                }
                _ => {
                    ::core::panicking::panic_fmt(
                        format_args!("Matching path not found"),
                    );
                }
            })
            .unwrap()
    }
    pub fn load(&self) -> Op {
        match self {
            OpWrapperNext::Id => Op::Id,
            OpWrapperNext::Sum(inner) => Op::Sum(inner.load()),
            OpWrapperNext::Mul(inner) => Op::Mul(inner.load()),
            OpWrapperNext::Div(inner) => Op::Div(inner.load()),
        }
    }
}
