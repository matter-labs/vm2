use crate::{
    addressing_modes::{
        AbsoluteStack, AdvanceStackPointer, AnySource, Arguments, CodePage, Immediate1, Register1,
        RelativeStack, Source, SourceWriter,
    },
    predication::Predicate,
    state::{Instruction, State},
};

fn jump<In: Source>(state: &mut State, mut instruction: *const Instruction) {
    let args = unsafe { &(*instruction).arguments };
    unsafe {
        instruction = instruction.add(1);
    }

    if args.predicate.satisfied(&state.flags) {
        let target = In::get(args, state).low_u32() as u16 as usize;
        if target < state.program_len {
            instruction = unsafe { state.program_start.add(target) };
        } else {
            // TODO panic
            return;
        }
    }

    unsafe { ((*instruction).handler)(state, instruction) }
}

impl Instruction {
    pub fn from_jump(source: AnySource, predicate: Predicate) -> Self {
        let mut arguments = Arguments::default();
        source.write_source(&mut arguments);
        arguments.predicate = predicate;

        Self {
            handler: match source {
                AnySource::Register1(_) => jump::<Register1>,
                AnySource::Immediate1(_) => jump::<Immediate1>,
                AnySource::AbsoluteStack(_) => jump::<AbsoluteStack>,
                AnySource::RelativeStack(_) => jump::<RelativeStack>,
                AnySource::AdvanceStackPointer(_) => jump::<AdvanceStackPointer>,
                AnySource::CodePage(_) => jump::<CodePage>,
            },
            arguments,
        }
    }
}
