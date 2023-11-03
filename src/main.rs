use vm2::run_arbitrary_program;

fn main() {
    let _ = run_arbitrary_program(&std::fs::read(std::env::args().nth(1).unwrap()).unwrap());
}
