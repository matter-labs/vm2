use arbitrary::Arbitrary;
use primitive_types::H160;
use zksync_vm2_interface::{HeapId, Tracer};

use super::stack::{Stack, StackPool};
use crate::{
    callframe::Callframe, decommit::is_kernel, predication::Flags, Program, World, WorldDiff,
};

impl<'a> Arbitrary<'a> for Flags {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self::new(u.arbitrary()?, u.arbitrary()?, u.arbitrary()?))
    }
}

impl<'a, T: Tracer, W: World<T>> Arbitrary<'a> for Callframe<T, W> {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let address: H160 = u.arbitrary()?;

        // zk_evm requires a base page, which heap and aux heap are offset from
        let base_page = u.int_in_range(1..=u32::MAX - 10)?;

        // zk_evm considers smaller pages to be older
        // vm2 doesn't care about the order
        // but the calldata heap must be different from the heap and aux heap
        #[allow(clippy::range_minus_one)]
        // cannot use exclusive range because of `int_in_range()` signature
        let calldata_heap = HeapId::from_u32_unchecked(u.int_in_range(0..=base_page - 1)?);

        let program: Program<T, W> = u.arbitrary()?;

        let mut me = Self {
            address,
            code_address: u.arbitrary()?,
            caller: u.arbitrary()?,
            exception_handler: u.arbitrary()?,
            context_u128: u.arbitrary()?,
            is_static: u.arbitrary()?,
            is_kernel: is_kernel(address),
            stack: Box::new(Stack::new_arbitrary(u, calldata_heap, base_page)?),
            sp: u.arbitrary()?,
            gas: u.arbitrary()?,
            near_calls: vec![],
            pc: program.instruction(0).unwrap(),
            program,
            heap: HeapId::from_u32_unchecked(base_page + 2),
            aux_heap: HeapId::from_u32_unchecked(base_page + 3),
            heap_size: u.arbitrary()?,
            aux_heap_size: u.arbitrary()?,
            calldata_heap,
            heaps_i_am_keeping_alive: vec![],
            world_before_this_frame: WorldDiff::default().snapshot(),
        };
        if u.arbitrary()? {
            me.push_near_call(
                u.arbitrary::<u32>()?.min(me.gas),
                u.arbitrary()?,
                WorldDiff::default().snapshot(),
            );
        }
        Ok(me)
    }
}

impl<T: Tracer, W: World<T>> Callframe<T, W> {
    pub(crate) fn dummy() -> Self {
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
            near_calls: vec![],
            pc: std::ptr::null(),
            program: Program::for_decommit(),
            heap: HeapId::FIRST_AUX,
            aux_heap: HeapId::FIRST_AUX,
            heap_size: 0,
            aux_heap_size: 0,
            calldata_heap: HeapId::from_u32_unchecked(1),
            heaps_i_am_keeping_alive: vec![],
            world_before_this_frame: WorldDiff::default().snapshot(),
        }
    }
}

impl<T, W> Callframe<T, W> {
    pub(crate) fn raw_first_instruction(&self) -> u64 {
        self.program.raw_first_instruction
    }
}
