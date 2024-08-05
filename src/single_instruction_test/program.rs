use crate::{decode::decode, Instruction};
use arbitrary::Arbitrary;
use std::{rc::Rc, sync::Arc};
use u256::U256;

use super::mock_array::MockRead;

#[derive(Clone, Debug)]
pub struct Program {
    pub raw_first_instruction: u64,

    // Need a two-instruction array so that incrementing the program counter is safe
    first_instruction: MockRead<u16, Rc<[Instruction; 2]>>,
    other_instruction: MockRead<u16, Rc<Option<[Instruction; 2]>>>,

    code_page: Arc<[U256]>,
}

impl<'a> Arbitrary<'a> for Program {
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

impl Program {
    pub fn instruction(&self, n: u16) -> Option<&Instruction> {
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

impl PartialEq for Program {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}
