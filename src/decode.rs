use crate::{
    addressing_modes::{
        AbsoluteStack, AnyDestination, AnySource, Immediate1, Register, Register1, Register2,
        RelativeStack,
    },
    end_execution,
    instruction_handlers::{
        Add, And, Div, Mul, Or, RotateLeft, RotateRight, ShiftLeft, ShiftRight, Sub, Xor,
    },
    jump_to_beginning, Instruction,
};
use zkevm_opcode_defs::{
    decoding::{EncodingModeProduction, VmEncodingMode},
    ImmMemHandlerFlags,
    Operand::*,
    RegOrImmFlags, SET_FLAGS_FLAG_IDX, SWAP_OPERANDS_FLAG_IDX_FOR_ARITH_OPCODES,
};

pub fn decode_program(raw: &[u64]) -> Vec<Instruction> {
    raw[..1 << 16]
        .iter()
        .map(|i| decode(*i))
        .chain(std::iter::once(if raw.len() >= 1 << 16 {
            jump_to_beginning()
        } else {
            // TODO execute invalid instruction or something instead
            end_execution()
        }))
        .collect()
}

fn decode(raw: u64) -> Instruction {
    let (parsed, _) = EncodingModeProduction::parse_preliminary_variant_and_absolute_number(raw);

    let predicate = match parsed.condition {
        zkevm_opcode_defs::Condition::Always => crate::Predicate::Always,
        zkevm_opcode_defs::Condition::Gt => crate::Predicate::IfGT,
        zkevm_opcode_defs::Condition::Lt => crate::Predicate::IfLT,
        zkevm_opcode_defs::Condition::Eq => crate::Predicate::IfEQ,
        zkevm_opcode_defs::Condition::Ge => crate::Predicate::IfGE,
        zkevm_opcode_defs::Condition::Le => crate::Predicate::IfLE,
        zkevm_opcode_defs::Condition::Ne => crate::Predicate::IfNotEQ,
        zkevm_opcode_defs::Condition::GtOrLt => crate::Predicate::IfGtOrLT,
    };

    let src1: AnySource = match parsed.variant.src0_operand_type {
        RegOnly | RegOrImm(RegOrImmFlags::UseRegOnly) | Full(ImmMemHandlerFlags::UseRegOnly) => {
            Register1(Register::new(parsed.src0_reg_idx)).into()
        }
        RegOrImm(RegOrImmFlags::UseImm16Only) | Full(ImmMemHandlerFlags::UseImm16Only) => {
            Immediate1(parsed.imm_0).into()
        }
        Full(ImmMemHandlerFlags::UseAbsoluteOnStack) => AbsoluteStack {
            immediate: parsed.imm_0,
            register: Register::new(parsed.src0_reg_idx),
        }
        .into(),
        Full(ImmMemHandlerFlags::UseStackWithPushPop) => RelativeStack {
            immediate: parsed.imm_0,
            register: Register::new(parsed.src0_reg_idx),
        }
        .into(),
        Full(ImmMemHandlerFlags::UseStackWithOffset) => todo!(),
        Full(ImmMemHandlerFlags::UseCodePage) => todo!(),
    };

    let out: AnyDestination = match parsed.variant.dst0_operand_type {
        RegOnly | RegOrImm(RegOrImmFlags::UseRegOnly) | Full(ImmMemHandlerFlags::UseRegOnly) => {
            Register1(Register::new(parsed.dst0_reg_idx)).into()
        }
        RegOrImm(RegOrImmFlags::UseImm16Only) | Full(ImmMemHandlerFlags::UseImm16Only) => {
            panic!("Parser wants to output to immediate")
        }
        Full(ImmMemHandlerFlags::UseAbsoluteOnStack) => AbsoluteStack {
            immediate: parsed.imm_1,
            register: Register::new(parsed.dst0_reg_idx),
        }
        .into(),
        Full(ImmMemHandlerFlags::UseStackWithPushPop) => RelativeStack {
            immediate: parsed.imm_1,
            register: Register::new(parsed.dst0_reg_idx),
        }
        .into(),
        Full(ImmMemHandlerFlags::UseStackWithOffset) => todo!(),
        Full(ImmMemHandlerFlags::UseCodePage) => todo!(),
    };

    let out2 = Register2(Register::new(parsed.dst1_reg_idx));

    macro_rules! binop {
        ($op: ident, $snd: tt) => {
            Instruction::from_binop::<$op>(
                src1,
                Register2(Register::new(parsed.src1_reg_idx)),
                out,
                $snd,
                predicate,
                parsed.variant.flags[SWAP_OPERANDS_FLAG_IDX_FOR_ARITH_OPCODES],
                parsed.variant.flags[SET_FLAGS_FLAG_IDX],
            )
        };
    }

    match parsed.variant.opcode {
        zkevm_opcode_defs::Opcode::Add(_) => binop!(Add, ()),
        zkevm_opcode_defs::Opcode::Sub(_) => binop!(Sub, ()),
        zkevm_opcode_defs::Opcode::Mul(_) => binop!(Mul, out2),
        zkevm_opcode_defs::Opcode::Div(_) => binop!(Div, out2),
        zkevm_opcode_defs::Opcode::Binop(x) => match x {
            zkevm_opcode_defs::BinopOpcode::Xor => binop!(Xor, ()),
            zkevm_opcode_defs::BinopOpcode::And => binop!(And, ()),
            zkevm_opcode_defs::BinopOpcode::Or => binop!(Or, ()),
        },
        zkevm_opcode_defs::Opcode::Shift(x) => match x {
            zkevm_opcode_defs::ShiftOpcode::Shl => binop!(ShiftLeft, ()),
            zkevm_opcode_defs::ShiftOpcode::Shr => binop!(ShiftRight, ()),
            zkevm_opcode_defs::ShiftOpcode::Rol => binop!(RotateLeft, ()),
            zkevm_opcode_defs::ShiftOpcode::Ror => binop!(RotateRight, ()),
        },
        zkevm_opcode_defs::Opcode::Jump(_) => Instruction::from_jump(src1, predicate),
        zkevm_opcode_defs::Opcode::Context(_) => todo!(),
        zkevm_opcode_defs::Opcode::Ptr(_) => todo!(),
        zkevm_opcode_defs::Opcode::NearCall(_) => todo!(),
        zkevm_opcode_defs::Opcode::Log(_) => todo!(),
        zkevm_opcode_defs::Opcode::FarCall(_) => todo!(),
        zkevm_opcode_defs::Opcode::Ret(_) => todo!(),
        zkevm_opcode_defs::Opcode::UMA(_) => todo!(),
        zkevm_opcode_defs::Opcode::Invalid(_) => todo!(),
        zkevm_opcode_defs::Opcode::Nop(_) => todo!(),
    }
}
