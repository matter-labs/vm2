use arbitrary::{Arbitrary, Unstructured};
use std::hint::black_box;
use vm2::{
    addressing_modes::{Immediate1, Register, Register1, Register2},
    end_execution,
    instruction_handlers::Sub,
    Instruction, State,
};

fn main() {
    // Maximum contract length is 2^16
    let mut program: Vec<_> = (0..(1 << 16) - 3)
        .map(|_| {
            let buf: [u8; 10] = rand::random();
            let mut unstructured = Unstructured::new(&buf);
            Arbitrary::arbitrary(&mut unstructured).unwrap()
        })
        .collect();

    // Amount of iterations chosen so that 10^9 ergs are required
    let runs = 2543;

    program.extend([
        Instruction::from_counter(Register1(Register::new(1)).into()),
        Instruction::from_binop::<Sub>(
            Immediate1(runs).into(),
            Register2(Register::new(1)),
            Register1(Register::new(0)).into(),
            (),
            vm2::Predicate::Always,
            false,
            true,
        ),
        Instruction::from_jump(Immediate1(0).into(), vm2::Predicate::IfNotEQ),
        end_execution(),
    ]);
    dbg!(program.len());

    let mut state = State::default();

    let start = std::time::Instant::now();
    state.run(black_box(&program));

    dbg!(start.elapsed());
    dbg!(state.registers);
}
