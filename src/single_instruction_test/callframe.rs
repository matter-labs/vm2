use super::{heap::FIRST_AUX_HEAP, stack::StackPool};
use crate::{callframe::Callframe, predication::Flags, Program, WorldDiff};
use arbitrary::Arbitrary;
use u256::H160;

impl<'a> Arbitrary<'a> for Flags {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self::new(u.arbitrary()?, u.arbitrary()?, u.arbitrary()?))
    }
}

impl<'a> Arbitrary<'a> for Callframe {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let mut me = Self {
            address: u.arbitrary()?,
            code_address: u.arbitrary()?,
            caller: u.arbitrary()?,
            exception_handler: u.arbitrary()?,
            context_u128: u.arbitrary()?,
            is_static: u.arbitrary()?,
            stack: u.arbitrary()?,
            sp: u.arbitrary()?,
            gas: u.arbitrary()?,
            stipend: u.arbitrary()?,
            near_calls: vec![],
            program: u.arbitrary()?,
            heap: u.arbitrary()?,
            aux_heap: u.arbitrary()?,
            heap_size: u.arbitrary()?,
            aux_heap_size: u.arbitrary()?,
            calldata_heap: u.arbitrary()?,
            heaps_i_am_keeping_alive: vec![], // TODO
            world_before_this_frame: WorldDiff::default().snapshot(), // TODO
        };
        if u.arbitrary()? {
            me.push_near_call(
                u.arbitrary::<u32>()?.min(me.gas),
                me.program.instruction(0).unwrap(),
                u.arbitrary()?,
                WorldDiff::default().snapshot(),
            );
        }
        Ok(me)
    }
}

impl Callframe {
    pub fn raw_first_instruction(&self) -> u64 {
        self.program.raw_first_instruction
    }

    pub fn dummy() -> Self {
        Self {
            address: H160::zero(),
            code_address: H160::zero(),
            caller: H160::zero(),
            exception_handler: 0,
            context_u128: 0,
            is_static: false,
            stack: StackPool.get(),
            sp: 0,
            gas: 0,
            stipend: 0,
            near_calls: vec![],
            program: Program::for_decommit(),
            heap: FIRST_AUX_HEAP,
            aux_heap: FIRST_AUX_HEAP,
            heap_size: 0,
            aux_heap_size: 0,
            calldata_heap: FIRST_AUX_HEAP,
            heaps_i_am_keeping_alive: vec![],
            world_before_this_frame: WorldDiff::default().snapshot(),
        }
    }
}
