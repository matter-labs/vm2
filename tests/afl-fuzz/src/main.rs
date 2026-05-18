use zksync_vm2_afl_fuzz::{validate_testcase, STATUS_DIVERGENCE};

fn main() {
    afl::fuzz!(|data: &[u8]| {
        let report = validate_testcase(data);
        if report.status == STATUS_DIVERGENCE {
            let json = serde_json::to_string_pretty(&report).expect("failed to serialize report");
            panic!("{json}");
        }
    });
}
