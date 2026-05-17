use jig_contract::{
    FeatureContext, FeatureDescriptor, NativeToolDescriptor, NativeToolKind, legacy_make_target,
    tool,
};

const COMMAND_KEYS: &[&str] = &["bootstrap_command", "contract_check_command"];
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
    if ctx.contract_version() >= 2
        || ctx.has_required_key(legacy_make_target::BOOTSTRAP, "bootstrap_command")
    {
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
        required_make_targets: Vec<String>,
    }

    impl FeatureContext for StubContext {
        fn contract_version(&self) -> u32 {
            self.contract_version
        }

        fn required_commands(&self) -> &[String] {
            &self.required_commands
        }

        fn required_make_targets(&self) -> &[String] {
            &self.required_make_targets
        }

        fn makefile_enabled(&self) -> bool {
            false
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
    fn core_bootstrap_is_required_for_legacy_make_contracts_that_declare_it() {
        let ctx = StubContext {
            contract_version: 1,
            required_make_targets: vec![legacy_make_target::BOOTSTRAP.to_string()],
            ..StubContext::default()
        };

        assert_eq!(
            (FEATURE.required_tools)(&ctx),
            vec![tool::CONTRACT_CHECK, tool::BOOTSTRAP]
        );
    }
}
