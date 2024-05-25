use crate::{callframe::Callframe, predication::Flags, WorldDiff};
use arbitrary::Arbitrary;

impl<'a> Arbitrary<'a> for Flags {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self::new(u.arbitrary()?, u.arbitrary()?, u.arbitrary()?))
    }
}

impl<'a> Arbitrary<'a> for Callframe {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
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
            near_calls: vec![], // TODO
            program: u.arbitrary()?,
            heap: u.arbitrary()?,
            aux_heap: u.arbitrary()?,
            heap_size: u.arbitrary()?,
            aux_heap_size: u.arbitrary()?,
            calldata_heap: u.arbitrary()?,
            heaps_i_am_keeping_alive: vec![], // TODO
            world_before_this_frame: WorldDiff::default().snapshot(), // TODO
        })
    }
}

impl Callframe {
    pub fn raw_first_instruction(&self) -> u64 {
        self.program.raw_first_instruction
    }
}
