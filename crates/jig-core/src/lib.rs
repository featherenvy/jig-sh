use jig_contract::{FeatureContext, FeatureDescriptor, NativeToolDescriptor, NativeToolKind, tool};

const BOOTSTRAP_COMMAND: &str = "bootstrap_command";
const COMMAND_KEYS: &[&str] = &[BOOTSTRAP_COMMAND, "contract_check_command"];
const NATIVE_TOOLS: &[NativeToolDescriptor] = &[NativeToolDescriptor::new(
    tool::CONTRACT_CHECK,
    false,
    NativeToolKind::ContractCheck,
)];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    NATIVE_TOOLS,
    required_tools,
    no_unavailable_tool_message,
);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    let mut required = vec![tool::CONTRACT_CHECK];
    if ctx.contract_version() >= 2 || ctx.has_required_command(BOOTSTRAP_COMMAND) {
        required.push(tool::BOOTSTRAP);
    }
    required
}

fn no_unavailable_tool_message(_ctx: &dyn FeatureContext, _tool_name: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct StubContext {
        contract_version: u32,
        required_commands: Vec<String>,
    }

    impl FeatureContext for StubContext {
        fn contract_version(&self) -> u32 {
            self.contract_version
        }

        fn required_commands(&self) -> &[String] {
            &self.required_commands
        }

        fn sqlx_enabled(&self) -> bool {
            false
        }

        fn schema_dump_enabled(&self) -> bool {
            false
        }

        fn frontend_app_count(&self) -> usize {
            0
        }
    }

    #[test]
    fn core_contract_check_is_always_required() {
        let ctx = StubContext {
            contract_version: 1,
            ..StubContext::default()
        };

        assert_eq!((FEATURE.required_tools)(&ctx), vec![tool::CONTRACT_CHECK]);
    }

    #[test]
    fn core_bootstrap_is_required_for_command_backed_contracts() {
        let ctx = StubContext {
            contract_version: 2,
            ..StubContext::default()
        };

        assert_eq!(
            (FEATURE.required_tools)(&ctx),
            vec![tool::CONTRACT_CHECK, tool::BOOTSTRAP]
        );
    }

    #[test]
    fn core_bootstrap_is_required_when_command_is_declared() {
        let ctx = StubContext {
            required_commands: vec![BOOTSTRAP_COMMAND.to_string()],
            ..StubContext::default()
        };

        assert_eq!(
            (FEATURE.required_tools)(&ctx),
            vec![tool::CONTRACT_CHECK, tool::BOOTSTRAP]
        );
    }
}
