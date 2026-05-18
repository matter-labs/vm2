use arbitrary::Arbitrary;
use serde::Serialize;
use std::{
    any::Any,
    panic::{self, catch_unwind, AssertUnwindSafe},
};
use zkevm_opcode_defs::decoding::{EncodingModeProduction, VmEncodingMode};
use zksync_vm2::{
    single_instruction_test::{
        add_heap_to_zk_evm, vm2_to_universal, vm2_to_zk_evm, MockWorld, NoTracer, UniversalVmState,
    },
    VirtualMachine,
};
use zksync_vm2_interface::Tracer;

pub const STATUS_MATCH: &str = "match";
pub const STATUS_DIVERGENCE: &str = "divergence";
pub const STATUS_ERROR: &str = "error";

pub mod scenario;

#[derive(Arbitrary, Debug)]
pub struct VmAndWorld<T: Tracer> {
    pub vm: VirtualMachine<T, MockWorld>,
    pub world: MockWorld,
}

#[derive(Debug, Serialize)]
pub struct ValidationReport {
    pub status: String,
    pub input_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<InstructionInfo>,
    pub steps: Vec<ValidationStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divergence: Option<DivergenceReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InstructionInfo {
    pub raw: String,
    pub decoded: String,
}

#[derive(Debug, Serialize)]
pub struct ValidationStep {
    pub description: String,
    pub success: bool,
}

#[derive(Debug, Serialize)]
pub struct DivergenceReport {
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm2_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zk_evm_state: Option<String>,
}

impl ValidationReport {
    pub fn error(input_bytes: usize, error: impl Into<String>) -> Self {
        Self {
            status: STATUS_ERROR.to_owned(),
            input_bytes,
            instruction: None,
            steps: Vec::new(),
            divergence: None,
            error: Some(error.into()),
        }
    }

    fn error_with_context(
        input_bytes: usize,
        instruction: Option<InstructionInfo>,
        steps: Vec<ValidationStep>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            status: STATUS_ERROR.to_owned(),
            input_bytes,
            instruction,
            steps,
            divergence: None,
            error: Some(error.into()),
        }
    }

    fn divergence(
        input_bytes: usize,
        instruction: Option<InstructionInfo>,
        steps: Vec<ValidationStep>,
        reason: impl Into<String>,
        vm2_state: Option<String>,
        zk_evm_state: Option<String>,
    ) -> Self {
        Self {
            status: STATUS_DIVERGENCE.to_owned(),
            input_bytes,
            instruction,
            steps,
            divergence: Some(DivergenceReport {
                reason: reason.into(),
                vm2_state,
                zk_evm_state,
            }),
            error: None,
        }
    }

    fn matched(
        input_bytes: usize,
        instruction: Option<InstructionInfo>,
        steps: Vec<ValidationStep>,
    ) -> Self {
        Self {
            status: STATUS_MATCH.to_owned(),
            input_bytes,
            instruction,
            steps,
            divergence: None,
            error: None,
        }
    }
}

pub fn validate_testcase(data: &[u8]) -> ValidationReport {
    match catch_unwind(AssertUnwindSafe(|| validate_testcase_inner(data))) {
        Ok(report) => report,
        Err(payload) => ValidationReport::divergence(
            data.len(),
            None,
            Vec::new(),
            format!(
                "validator panicked: {}",
                panic_payload_to_string(payload.as_ref())
            ),
            None,
            None,
        ),
    }
}

fn validate_testcase_inner(data: &[u8]) -> ValidationReport {
    let input_bytes = data.len();
    let VmAndWorld { vm, world } =
        match arbitrary::Unstructured::new(data).arbitrary::<VmAndWorld<()>>() {
            Ok(value) => value,
            Err(err) => {
                return ValidationReport::error(
                    input_bytes,
                    format!("failed to decode testcase as vm2 arbitrary state: {err}"),
                );
            }
        };

    validate_vm_and_world(input_bytes, vm, world)
}

pub fn validate_vm_and_world(
    input_bytes: usize,
    vm: VirtualMachine<(), MockWorld>,
    world: MockWorld,
) -> ValidationReport {
    validate_vm_and_world_for_cycles(input_bytes, vm, world, 1)
}

pub fn validate_vm_and_world_for_cycles(
    input_bytes: usize,
    mut vm: VirtualMachine<(), MockWorld>,
    mut world: MockWorld,
    cycles: usize,
) -> ValidationReport {
    let instruction = Some(instruction_info(&vm));
    let mut steps = vec![ValidationStep {
        description: "loaded testcase".to_owned(),
        success: true,
    }];

    if !vm.is_in_valid_state() {
        return ValidationReport::error_with_context(
            input_bytes,
            instruction,
            steps,
            "decoded testcase produced an invalid initial vm2 state",
        );
    }
    steps.push(ValidationStep {
        description: "validated initial vm2 state".to_owned(),
        success: true,
    });

    let mut zk_evm = vm2_to_zk_evm(&vm, world.clone());

    for cycle in 0..cycles {
        if !vm.instruction_is_covered_by_harness() {
            return ValidationReport::error_with_context(
                input_bytes,
                instruction,
                steps,
                format!("instruction at cycle {cycle} is outside the current harness coverage"),
            );
        }
        steps.push(ValidationStep {
            description: format!("confirmed instruction at cycle {cycle} is covered by harness"),
            success: true,
        });

        let is_far_call = vm.instruction_is_far_call();
        match catch_unwind_without_hook(|| {
            vm.run_single_instruction(&mut world, &mut ());
        }) {
            Ok(()) => {
                steps.push(ValidationStep {
                    description: format!("executed cycle {cycle} in vm2"),
                    success: true,
                });
            }
            Err(payload) => {
                let zk_evm_result = catch_unwind_without_hook(|| {
                    let _ = zk_evm.cycle(&mut NoTracer);
                });
                return ValidationReport::divergence(
                    input_bytes,
                    instruction,
                    steps,
                    format!(
                        "vm2 panicked during cycle {cycle}: {}; zk_evm panic: {}",
                        panic_payload_to_string(payload.as_ref()),
                        zk_evm_result
                            .err()
                            .as_ref()
                            .map(|payload| panic_payload_to_string(payload.as_ref()))
                            .unwrap_or_else(|| "none".to_owned())
                    ),
                    None,
                    Some(format!("{:#?}", UniversalVmState::from(zk_evm))),
                );
            }
        }

        if !vm.is_in_valid_state() {
            return ValidationReport::divergence(
                input_bytes,
                instruction,
                steps,
                format!("vm2 entered an invalid state after executing cycle {cycle}"),
                Some(format!("{:#?}", vm.dump_state())),
                None,
            );
        }

        add_heap_to_zk_evm(&mut zk_evm, &vm);
        match catch_unwind_without_hook(|| {
            let _ = zk_evm.cycle(&mut NoTracer);
        }) {
            Ok(()) => {
                steps.push(ValidationStep {
                    description: format!("executed cycle {cycle} in zk_evm"),
                    success: true,
                });
            }
            Err(payload) => {
                return ValidationReport::divergence(
                    input_bytes,
                    instruction,
                    steps,
                    format!(
                        "zk_evm panicked during cycle {cycle}: {}",
                        panic_payload_to_string(payload.as_ref())
                    ),
                    Some(format!("{:#?}", vm.dump_state())),
                    None,
                );
            }
        }

        // zk_evm's far call sometimes creates a frame that is immediately discarded by panic.
        // This mirrors the AFL harness normalization before comparing observable post-state.
        if is_far_call && zk_evm.local_state.pending_exception {
            vm.run_single_instruction(&mut world, &mut ());
            let _ = zk_evm.cycle(&mut NoTracer);
            steps.push(ValidationStep {
                description: format!("normalized far-call panic state at cycle {cycle}"),
                success: true,
            });
        }
    }

    let zk_evm_state = UniversalVmState::from(zk_evm);
    let vm2_state = vm2_to_universal(&vm);

    if zk_evm_state != vm2_state {
        return ValidationReport::divergence(
            input_bytes,
            instruction,
            steps,
            "post-state mismatch between vm2 and zk_evm",
            Some(format!("{vm2_state:#?}")),
            Some(format!("{zk_evm_state:#?}")),
        );
    }

    steps.push(ValidationStep {
        description: "compared canonical post-state".to_owned(),
        success: true,
    });
    ValidationReport::matched(input_bytes, instruction, steps)
}

pub fn validate_scenario(input_bytes: usize, scenario: scenario::Scenario) -> ValidationReport {
    match catch_unwind(AssertUnwindSafe(|| {
        let (vm, world, cycles) = scenario.into_vm_and_world()?;
        Ok::<_, String>(validate_vm_and_world_for_cycles(
            input_bytes,
            vm,
            world,
            cycles,
        ))
    })) {
        Ok(Ok(report)) => report,
        Ok(Err(err)) => ValidationReport::error(input_bytes, err),
        Err(payload) => ValidationReport::divergence(
            input_bytes,
            None,
            Vec::new(),
            format!(
                "validator panicked: {}",
                panic_payload_to_string(payload.as_ref())
            ),
            None,
            None,
        ),
    }
}

fn instruction_info<T: Tracer, W>(vm: &VirtualMachine<T, W>) -> InstructionInfo {
    let raw = vm.raw_first_instruction();
    let (parsed, _) = EncodingModeProduction::parse_preliminary_variant_and_absolute_number(raw);
    InstructionInfo {
        raw: format!("0x{raw:016x}"),
        decoded: parsed.to_string(),
    }
}

fn panic_payload_to_string(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

fn catch_unwind_without_hook<F, R>(f: F) -> std::thread::Result<R>
where
    F: FnOnce() -> R,
{
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(f));
    panic::set_hook(hook);
    result
}
