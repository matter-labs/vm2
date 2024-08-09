use crate::{decode::decode, Instruction};
use arbitrary::Arbitrary;
use eravm_stable_interface::Tracer;
use std::{rc::Rc, sync::Arc};
use u256::U256;

use super::mock_array::MockRead;

#[derive(Debug)]
pub struct Program<T> {
    pub raw_first_instruction: u64,

    // Need a two-instruction array so that incrementing the program counter is safe
    first_instruction: MockRead<u16, Rc<[Instruction<T>; 2]>>,
    other_instruction: MockRead<u16, Rc<Option<[Instruction<T>; 2]>>>,

    code_page: Arc<[U256]>,
}

impl<T> Clone for Program<T> {
    fn clone(&self) -> Self {
        Self {
            raw_first_instruction: self.raw_first_instruction,
            first_instruction: self.first_instruction.clone(),
            other_instruction: self.other_instruction.clone(),
            code_page: self.code_page.clone(),
        }
    }
}

impl<'a, T: Tracer> Arbitrary<'a> for Program<T> {
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

impl<T> Program<T> {
    pub fn instruction(&self, n: u16) -> Option<&Instruction<T>> {
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

impl<T: Tracer> Program<T> {
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

impl<T> PartialEq for Program<T> {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}
