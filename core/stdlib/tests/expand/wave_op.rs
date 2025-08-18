use stdlib::Wavey;

#[derive(Wavey)]
pub enum Op {
    Id,
    Sum(Operand),
    Mul(Operand),
    Div(Operand),
}
