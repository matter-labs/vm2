use zkevm_opcode_defs::ethereum_types::Address;

use crate::{
    addressing_modes::{Arguments, Immediate1, Register, Register1, Register2},
    interface::{
        opcodes::Normal, CallingMode, GlobalStateInterface, Opcode, OpcodeType, ReturnType,
        ShouldStop, Tracer,
    },
    testonly::{initial_decommit, TestWorld},
    Instruction, ModeRequirements, Predicate, Program, Settings, VirtualMachine,
};

struct ExpectingTracer {
    future: Vec<Opcode>,
    current: Option<Opcode>,
}

impl ExpectingTracer {
    fn new(mut opcodes: Vec<Opcode>) -> Self {
        opcodes.reverse();
        Self {
            future: opcodes,
            current: None,
        }
    }
}

impl Tracer for ExpectingTracer {
    fn before_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, _: &mut S) {
        assert!(self.current.is_none(), "expected after_instruction");

        let expected = self.future.pop().expect("expected program end");
        assert_eq!(OP::VALUE, expected);
        self.current = Some(expected);
    }
    fn after_instruction<OP: OpcodeType, S: GlobalStateInterface>(
        &mut self,
        _: &mut S,
    ) -> ShouldStop {
        assert_eq!(
            OP::VALUE,
            self.current.take().expect("expected before_instruction")
        );
        ShouldStop::Continue
    }
}

#[test]
fn trace_failing_far_call() {
    let instructions = vec![Instruction::from_far_call::<Normal>(
        Register1(Register::new(0)),
        Register2(Register::new(1)),
        Immediate1(1),
        false,
        false,
        Arguments::new(Predicate::Always, 25, ModeRequirements::none()),
    )];

    let program = Program::from_raw(instructions, vec![]);

    let address = Address::from_low_u64_be(0x_1234_5678_90ab_cdef);
    let mut world = TestWorld::new(&[(address, program)]);
    let program = initial_decommit(&mut world, address);

    let mut vm = VirtualMachine::new(
        address,
        program,
        Address::zero(),
        &[],
        1000,
        Settings {
            default_aa_code_hash: [0; 32],
            evm_interpreter_code_hash: [0; 32],
            hook_address: 0,
        },
    );

    vm.run(
        &mut world,
        &mut ExpectingTracer::new(vec![
            Opcode::FarCall(CallingMode::Normal),
            Opcode::Ret(ReturnType::Panic),
        ]),
    );
}
