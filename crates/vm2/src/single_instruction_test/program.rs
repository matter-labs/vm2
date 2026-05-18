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
    scenario_instructions: Option<Arc<[Instruction<T, W>]>>,

    code_page: Arc<[U256]>,
}

impl<T, W> Clone for Program<T, W> {
    fn clone(&self) -> Self {
        Self {
            raw_first_instruction: self.raw_first_instruction,
            first_instruction: self.first_instruction.clone(),
            other_instruction: self.other_instruction.clone(),
            scenario_instructions: self.scenario_instructions.clone(),
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
            scenario_instructions: None,
            code_page: [u.arbitrary()?; 1].into(),
        })
    }
}

impl<T, W> Program<T, W> {
    pub fn instruction(&self, n: u16) -> Option<&Instruction<T, W>> {
        if let Some(instructions) = &self.scenario_instructions {
            return instructions.get::<usize>(n.into());
        }
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

    pub(crate) fn initial_code_word(&self) -> U256 {
        if self.scenario_instructions.is_some() {
            self.code_page.first().copied().unwrap_or_default()
        } else {
            U256([0, 0, 0, self.raw_first_instruction])
        }
    }
}

impl<T: Tracer, W: World<T>> Program<T, W> {
    pub fn from_raw_instruction(raw_first_instruction: u64, code_page: U256) -> Self {
        Self {
            raw_first_instruction,
            first_instruction: MockRead::new(Rc::new([
                decode(raw_first_instruction, false),
                Instruction::from_invalid(),
            ])),
            other_instruction: MockRead::new(Rc::new(Some([
                Instruction::from_invalid(),
                Instruction::from_invalid(),
            ]))),
            scenario_instructions: None,
            code_page: Arc::new([code_page]),
        }
    }

    pub fn from_raw_instructions(raw_instructions: Vec<u64>) -> Self {
        let raw_first_instruction = raw_instructions.first().copied().unwrap_or_default();
        let instructions = raw_instructions
            .iter()
            .map(|&raw| decode(raw, false))
            .chain(std::iter::once(Instruction::from_invalid()))
            .collect::<Vec<_>>();

        let mut bytecode = Vec::with_capacity(raw_instructions.len().next_multiple_of(4) * 8);
        for raw in raw_instructions {
            bytecode.extend_from_slice(&raw.to_be_bytes());
        }
        bytecode.resize(bytecode.len().next_multiple_of(32), 0);
        let mut code_page = bytecode
            .chunks_exact(32)
            .map(U256::from_big_endian)
            .collect::<Vec<_>>();
        if code_page.is_empty() {
            code_page.push(U256::zero());
        }

        Self {
            raw_first_instruction,
            first_instruction: MockRead::new(Rc::new([
                decode(raw_first_instruction, false),
                Instruction::from_invalid(),
            ])),
            other_instruction: MockRead::new(Rc::new(Some([
                Instruction::from_invalid(),
                Instruction::from_invalid(),
            ]))),
            scenario_instructions: Some(instructions.into()),
            code_page: code_page.into(),
        }
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
            scenario_instructions: None,
            code_page: Arc::new([U256::zero(); 1]),
        }
    }

    pub(crate) fn new_panicking() -> Self {
        Self {
            raw_first_instruction: 0xBAD,
            first_instruction: MockRead::new(Rc::new([
                Instruction::from_spontaneous_panic(),
                Instruction::from_invalid(),
            ])),
            other_instruction: MockRead::new(Rc::new(Some([
                Instruction::from_invalid(),
                Instruction::from_invalid(),
            ]))),
            scenario_instructions: None,
            code_page: Arc::new([U256::zero(); 1]),
        }
    }
}

impl<T, W> PartialEq for Program<T, W> {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}
