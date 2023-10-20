/// Select an instantiation of a generic function based on runtime variables
/// For example `monomorphize!(load [H] match_reg_imm src match_boolean increment)`
macro_rules! monomorphize {
    ($function_name: ident [$($types: tt)*] $next_matcher: ident $($rest: ident)*) => {
        $next_matcher!([$($types)*] $($rest)* parameterize $function_name)
    };

    ($function_name: ident $($rest: ident)*) => {
        monomorphize!($function_name [] $($rest)*)
    };
}

macro_rules! match_reg_imm {
    ([ $($types: tt)* ] $input_type: ident $next_matcher: ident $($rest: ident)*) => {
        match $input_type {
            RegisterOrImmediate::Register1(_) => $next_matcher!([$($types)* Register1] $($rest)*),
            RegisterOrImmediate::Immediate1(_) => $next_matcher!([$($types)* Immediate1] $($rest)*),
        }
    };
}

macro_rules! match_boolean {
    ([ $($types: tt)* ] $increment: ident $next_matcher: ident $($rest: ident)*) => {
        if $increment {
            $next_matcher!([$($types)* {true}] $($rest)*)
        } else {
            $next_matcher!([$($types)* {false}] $($rest)*)
        }
    };
}

macro_rules! parameterize {
    ([$($types: tt)*] $function_name:ident) => {
        $function_name::<$($types),*>
    };
}

pub(crate) use {match_boolean, match_reg_imm, monomorphize, parameterize};
