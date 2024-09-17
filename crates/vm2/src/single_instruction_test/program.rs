use std::{rc::Rc, sync::Arc};

use arbitrary::Arbitrary;
use primitive_types::U256;
use zksync_vm2_interface::Tracer;

use super::mock_array::MockRead;
use crate::{decode::decode, Instruction, World};

#[derive(Debug)]
pub struct Program<T, W> {
    pub raw_first_instruction: u64,

    // Need a two-instruction array so that incrementing the program counter is safe
    first_instruction: MockRead<u16, Rc<[Instruction<T, W>; 2]>>,
    #[allow(clippy::type_complexity)]
    other_instruction: MockRead<u16, Rc<Option<[Instruction<T, W>; 2]>>>,

    code_page: Arc<[U256]>,
}

impl<T, W> Clone for Program<T, W> {
    fn clone(&self) -> Self {
        Self {
            raw_first_instruction: self.raw_first_instruction,
            first_instruction: self.first_instruction.clone(),
            other_instruction: self.other_instruction.clone(),
            code_page: self.code_page.clone(),
        }
    }
}

impl<'a, T: Tracer, W: World<T>> Arbitrary<'a> for Program<T, W> {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let raw_first_instruction = u.arbitrary()?;

        Ok(Self {
            raw_first_instruction,
            first_instruction: MockRead::new(Rc::new([
                decode(raw_first_instruction, false),
                Instruction::from_invalid(),
            ])),
            other_instruction: MockRead::new(Rc::new(
                u.arbitrary::<bool>()?
                    .then_some([Instruction::from_invalid(), Instruction::from_invalid()]),
            )),
            code_page: [u.arbitrary()?; 1].into(),
        })
    }
}

impl<T, W> Program<T, W> {
    pub fn instruction(&self, n: u16) -> Option<&Instruction<T, W>> {
        if n == 0 {
            Some(&self.first_instruction.get(n).as_ref()[0])
        } else {
            self.other_instruction
                .get(n)
                .as_ref()
                .as_ref()
                .map(|x| &x[0])
        }
    }

    pub fn code_page(&self) -> &Arc<[U256]> {
        &self.code_page
    }
}

impl<T: Tracer, W> Program<T, W> {
    pub fn for_decommit() -> Self {
        Self {
            raw_first_instruction: 0,
            first_instruction: MockRead::new(Rc::new([
                Instruction::from_invalid(),
                Instruction::from_invalid(),
            ])),
            other_instruction: MockRead::new(Rc::new(Some([
                Instruction::from_invalid(),
                Instruction::from_invalid(),
            ]))),
            code_page: Arc::new([U256::zero(); 1]),
        }
    }
}

impl<T, W> PartialEq for Program<T, W> {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}
