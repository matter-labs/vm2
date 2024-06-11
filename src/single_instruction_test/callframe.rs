use super::{heap::FIRST_AUX_HEAP, stack::StackPool};
use crate::{
    callframe::Callframe, decommit::is_kernel, predication::Flags, HeapId, Program, WorldDiff,
};
use arbitrary::Arbitrary;
use u256::H160;
use zkevm_opcode_defs::EVM_SIMULATOR_STIPEND;

impl<'a> Arbitrary<'a> for Flags {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self::new(u.arbitrary()?, u.arbitrary()?, u.arbitrary()?))
    }
}

impl<'a> Arbitrary<'a> for Callframe {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let address = u.arbitrary()?;

        // zk_evm requires a base page, which heap and aux heap are offset from
        let base_page = u.int_in_range(1..=u32::MAX - 10)?;

        let mut me = Self {
            address,
            code_address: u.arbitrary()?,
            caller: u.arbitrary()?,
            exception_handler: u.arbitrary()?,
            context_u128: u.arbitrary()?,
            is_static: u.arbitrary()?,
            is_kernel: is_kernel(address),
            stack: u.arbitrary()?,
            sp: u.arbitrary()?,
            // It is assumed that it is always possible to add the stipend
            gas: u.int_in_range(0..=u32::MAX - EVM_SIMULATOR_STIPEND)?,
            stipend: u.arbitrary()?,
            near_calls: vec![],
            program: u.arbitrary()?,
            heap: HeapId::from_u32_unchecked(base_page + 2),
            aux_heap: HeapId::from_u32_unchecked(base_page + 3),
            heap_size: u.arbitrary()?,
            aux_heap_size: u.arbitrary()?,
            // zk_evm considers smaller pages to be older
            // vm2 doesn't care about the order
            // but the calldata heap must be different from the heap and aux heap
            calldata_heap: HeapId::from_u32_unchecked(u.int_in_range(0..=base_page - 1)?),
            heaps_i_am_keeping_alive: vec![],
            world_before_this_frame: WorldDiff::default().snapshot(),
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
            is_kernel: false,
            stack: StackPool {}.get(),
            sp: 0,
            gas: 0,
            stipend: 0,
            near_calls: vec![],
            program: Program::for_decommit(),
            heap: FIRST_AUX_HEAP,
            aux_heap: FIRST_AUX_HEAP,
            heap_size: 0,
            aux_heap_size: 0,
            calldata_heap: HeapId::from_u32_unchecked(1),
            heaps_i_am_keeping_alive: vec![],
            world_before_this_frame: WorldDiff::default().snapshot(),
        }
    }
}
