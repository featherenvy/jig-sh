use jig_contract::{FeatureContext, FeatureDescriptor, legacy_make_target, tool};

const COMMAND_KEYS: &[&str] = &[
    "rust_clippy_command",
    "rust_fmt_check_command",
    "rust_test_command",
    "rust_test_locked_command",
];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    &[],
    required_tools,
    no_unavailable_tool_message,
);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    let mut required = Vec::new();
    if ctx.has_required_command("rust_fmt_check_command")
        || ctx.has_required_make_target(legacy_make_target::FMT_CHECK)
    {
        required.push(tool::FMT_CHECK);
    }
    if ctx.has_required_command("rust_clippy_command")
        || ctx.has_required_make_target(legacy_make_target::CLIPPY)
    {
        required.push(tool::CLIPPY);
    }
    if ctx.has_required_command("rust_test_command")
        || ctx.has_required_make_target(legacy_make_target::TEST)
    {
        required.push(tool::TEST);
    }
    if ctx.has_required_command("rust_test_locked_command")
        || ctx.has_required_make_target(legacy_make_target::TEST_RUST_LOCKED)
        || ctx.has_required_make_target(legacy_make_target::TEST_LOCKED)
    {
        required.push(tool::TEST_LOCKED);
    }
    required
}

fn no_unavailable_tool_message(_ctx: &dyn FeatureContext, _tool_name: &str) -> Option<String> {
    None
}
