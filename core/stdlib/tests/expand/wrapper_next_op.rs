use stdlib::WrapperNext;

#[derive(WrapperNext)]
pub enum Op {
    Id,
    Sum(Operand),
    Mul(Operand),
    Div(Operand),
}
