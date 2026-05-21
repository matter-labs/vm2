use std::collections::BTreeMap;

use primitive_types::{H160, U256};
use serde::Deserialize;
use zksync_vm2::{
    single_instruction_test::{
        build_scenario_vm, MockWorld, ScenarioFrameConfig, ScenarioMemoryConfig,
        ScenarioStackConfig, ScenarioVmConfig,
    },
    FatPointer, Settings, VirtualMachine,
};
use zksync_vm2_interface::HeapId;

#[derive(Debug, Deserialize)]
pub struct Scenario {
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub program: Option<Vec<String>>,
    #[serde(default)]
    pub cycles: Option<usize>,
    #[serde(default)]
    pub code_page: Option<String>,
    #[serde(default)]
    pub registers: BTreeMap<String, RegisterDef>,
    #[serde(default)]
    pub flags: Option<FlagsDef>,
    #[serde(default)]
    pub frame: Option<FrameDef>,
    #[serde(default)]
    pub stack: Option<StackDef>,
    #[serde(default)]
    pub memory: Option<MemoryDef>,
    #[serde(default)]
    pub storage_read: Option<Option<String>>,
    #[serde(default)]
    pub storage_write_cost: Option<u32>,
    #[serde(default)]
    pub transaction_number: Option<u16>,
    #[serde(default)]
    pub context_u128: Option<String>,
    #[serde(default)]
    pub settings: Option<SettingsDef>,
    #[serde(default = "default_true")]
    pub include_parent_frame: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum RegisterDef {
    Value(String),
    Tagged { value: String, pointer: bool },
    Pointer(PointerDef),
}

#[derive(Debug, Deserialize)]
pub struct PointerDef {
    pub memory_page: u32,
    #[serde(default)]
    pub start: u32,
    #[serde(default)]
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug, Deserialize)]
pub struct FlagsDef {
    #[serde(default, alias = "overflow_or_less_than", alias = "lt")]
    pub less_than: bool,
    #[serde(default, alias = "equality", alias = "eq")]
    pub equal: bool,
    #[serde(default, alias = "gt")]
    pub greater_than: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct FrameDef {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub code_address: Option<String>,
    #[serde(default)]
    pub caller: Option<String>,
    #[serde(default)]
    pub exception_handler: Option<u16>,
    #[serde(default)]
    pub context_u128: Option<String>,
    #[serde(default)]
    pub is_static: Option<bool>,
    #[serde(default)]
    pub sp: Option<u16>,
    #[serde(default)]
    pub gas: Option<u32>,
    #[serde(default)]
    pub base_page: Option<u32>,
    #[serde(default)]
    pub heap: Option<u32>,
    #[serde(default)]
    pub aux_heap: Option<u32>,
    #[serde(default)]
    pub calldata_heap: Option<u32>,
    #[serde(default, alias = "heap_size")]
    pub heap_bound: Option<u32>,
    #[serde(default, alias = "aux_heap_size")]
    pub aux_heap_bound: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct StackDef {
    #[serde(default)]
    pub read_value: Option<String>,
    #[serde(default)]
    pub read_pointer: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct MemoryDef {
    #[serde(default)]
    pub heap_id: Option<u32>,
    #[serde(default)]
    pub heap_read_u256: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SettingsDef {
    #[serde(default)]
    pub default_aa_code_hash: Option<String>,
    #[serde(default)]
    pub evm_interpreter_code_hash: Option<String>,
    #[serde(default)]
    pub hook_address: Option<u32>,
}

impl Scenario {
    pub fn into_vm_and_world(
        self,
    ) -> Result<(VirtualMachine<(), MockWorld>, MockWorld, usize), String> {
        let cycles = self.cycles.unwrap_or(1);
        if cycles == 0 {
            return Err("cycles must be greater than zero".to_owned());
        }

        let mut config = ScenarioVmConfig {
            raw_instructions: match self.program {
                Some(program) => {
                    if program.is_empty() {
                        return Err("program must contain at least one instruction".to_owned());
                    }
                    program
                        .iter()
                        .map(|instruction| parse_u64(instruction))
                        .collect::<Result<Vec<_>, _>>()?
                }
                None => {
                    let instruction = self
                        .instruction
                        .ok_or_else(|| "scenario must provide instruction or program".to_owned())?;
                    vec![parse_u64(&instruction)?]
                }
            },
            code_page: self
                .code_page
                .as_deref()
                .map(parse_u256)
                .transpose()?
                .unwrap_or_default(),
            include_parent_frame: self.include_parent_frame,
            ..ScenarioVmConfig::default()
        };

        if let Some(flags) = self.flags {
            config.flags = (flags.less_than, flags.equal, flags.greater_than);
        }

        if let Some(frame) = self.frame {
            apply_frame(frame, &mut config.frame)?;
        }

        config.memory = ScenarioMemoryConfig {
            heap_id: self
                .memory
                .as_ref()
                .and_then(|memory| memory.heap_id)
                .map(HeapId::from_u32_unchecked)
                .unwrap_or(config.frame.heap),
            heap_read_u256: self
                .memory
                .as_ref()
                .and_then(|memory| memory.heap_read_u256.as_deref())
                .map(parse_u256)
                .transpose()?
                .unwrap_or_default(),
        };

        if let Some(stack) = self.stack {
            config.stack = ScenarioStackConfig {
                read_value: stack
                    .read_value
                    .as_deref()
                    .map(parse_u256)
                    .transpose()?
                    .unwrap_or_default(),
                read_is_pointer: stack.read_pointer.unwrap_or(false),
            };
        }

        for (register, value) in self.registers {
            let index = parse_register_index(&register)?;
            if index == 0 {
                return Err("r0 is always zero and cannot be overridden".to_owned());
            }
            config.registers[index] = parse_register(value)?;
        }

        config.storage_read = match self.storage_read {
            Some(Some(value)) => Some(parse_u256(&value)?),
            Some(None) => None,
            None => Some(U256::zero()),
        };
        if let Some(storage_write_cost) = self.storage_write_cost {
            config.storage_write_cost = storage_write_cost;
        }
        if let Some(transaction_number) = self.transaction_number {
            config.transaction_number = transaction_number;
        }
        if let Some(context_u128) = self.context_u128 {
            config.context_u128 = parse_u128(&context_u128)?;
        }
        if let Some(settings) = self.settings {
            config.settings = parse_settings(settings)?;
        }

        let (vm, world) = build_scenario_vm(config);
        Ok((vm, world, cycles))
    }
}

fn apply_frame(frame: FrameDef, config: &mut ScenarioFrameConfig) -> Result<(), String> {
    if let Some(base_page) = frame.base_page {
        config.base_page = base_page;
        config.heap = HeapId::from_u32_unchecked(base_page + 2);
        config.aux_heap = HeapId::from_u32_unchecked(base_page + 3);
    }
    if let Some(address) = frame.address {
        config.address = parse_h160(&address)?;
    }
    if let Some(code_address) = frame.code_address {
        config.code_address = parse_h160(&code_address)?;
    } else {
        config.code_address = config.address;
    }
    if let Some(caller) = frame.caller {
        config.caller = parse_h160(&caller)?;
    }
    if let Some(exception_handler) = frame.exception_handler {
        config.exception_handler = exception_handler;
    }
    if let Some(context_u128) = frame.context_u128 {
        config.context_u128 = parse_u128(&context_u128)?;
    }
    if let Some(is_static) = frame.is_static {
        config.is_static = is_static;
    }
    if let Some(sp) = frame.sp {
        config.sp = sp;
    }
    if let Some(gas) = frame.gas {
        config.gas = gas;
    }
    if let Some(heap) = frame.heap {
        config.heap = HeapId::from_u32_unchecked(heap);
    }
    if let Some(aux_heap) = frame.aux_heap {
        config.aux_heap = HeapId::from_u32_unchecked(aux_heap);
    }
    if let Some(calldata_heap) = frame.calldata_heap {
        config.calldata_heap = HeapId::from_u32_unchecked(calldata_heap);
    }
    if let Some(heap_bound) = frame.heap_bound {
        config.heap_bound = heap_bound;
    }
    if let Some(aux_heap_bound) = frame.aux_heap_bound {
        config.aux_heap_bound = aux_heap_bound;
    }
    Ok(())
}

fn parse_register(register: RegisterDef) -> Result<(U256, bool), String> {
    match register {
        RegisterDef::Value(value) => Ok((parse_u256(&value)?, false)),
        RegisterDef::Tagged { value, pointer } => Ok((parse_u256(&value)?, pointer)),
        RegisterDef::Pointer(pointer) => {
            let value = FatPointer {
                offset: pointer.offset,
                memory_page: HeapId::from_u32_unchecked(pointer.memory_page),
                start: pointer.start,
                length: pointer.length,
            }
            .into_u256();
            Ok((value, true))
        }
    }
}

fn parse_settings(settings: SettingsDef) -> Result<Settings, String> {
    Ok(Settings {
        default_aa_code_hash: settings
            .default_aa_code_hash
            .as_deref()
            .map(parse_h256_bytes)
            .transpose()?
            .unwrap_or([0; 32]),
        evm_interpreter_code_hash: settings
            .evm_interpreter_code_hash
            .as_deref()
            .map(parse_h256_bytes)
            .transpose()?
            .unwrap_or([0; 32]),
        hook_address: settings.hook_address.unwrap_or(0),
    })
}

fn parse_register_index(register: &str) -> Result<usize, String> {
    let index = register.strip_prefix('r').unwrap_or(register);
    let index = index
        .parse::<usize>()
        .map_err(|err| format!("invalid register '{register}': {err}"))?;
    if index > 15 {
        return Err(format!("register index out of range: {register}"));
    }
    Ok(index)
}

fn parse_h160(value: &str) -> Result<H160, String> {
    value
        .parse()
        .map_err(|err| format!("invalid H160 '{value}': {err}"))
}

fn parse_h256_bytes(value: &str) -> Result<[u8; 32], String> {
    let bytes = parse_hex_bytes(value)?;
    if bytes.len() != 32 {
        return Err(format!("expected 32 bytes, got {}", bytes.len()));
    }
    let mut result = [0_u8; 32];
    result.copy_from_slice(&bytes);
    Ok(result)
}

fn parse_u64(value: &str) -> Result<u64, String> {
    if let Some(hex) = strip_hex_prefix(value) {
        u64::from_str_radix(hex, 16).map_err(|err| format!("invalid u64 '{value}': {err}"))
    } else {
        value
            .parse()
            .map_err(|err| format!("invalid u64 '{value}': {err}"))
    }
}

fn parse_u128(value: &str) -> Result<u128, String> {
    if let Some(hex) = strip_hex_prefix(value) {
        u128::from_str_radix(hex, 16).map_err(|err| format!("invalid u128 '{value}': {err}"))
    } else {
        value
            .parse()
            .map_err(|err| format!("invalid u128 '{value}': {err}"))
    }
}

fn parse_u256(value: &str) -> Result<U256, String> {
    if let Some(hex) = strip_hex_prefix(value) {
        U256::from_str_radix(hex, 16).map_err(|err| format!("invalid U256 '{value}': {err}"))
    } else {
        U256::from_dec_str(value).map_err(|err| format!("invalid U256 '{value}': {err}"))
    }
}

fn parse_hex_bytes(value: &str) -> Result<Vec<u8>, String> {
    let hex = strip_hex_prefix(value).unwrap_or(value);
    hex::decode(hex).map_err(|err| format!("invalid hex bytes '{value}': {err}"))
}

fn strip_hex_prefix(value: &str) -> Option<&str> {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
}

fn default_true() -> bool {
    true
}
