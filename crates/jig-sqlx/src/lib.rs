use jig_contract::{
    FeatureContext, FeatureDescriptor, NativeToolDescriptor, NativeToolKind, legacy_make_target,
    tool,
};

const COMMAND_KEYS: &[&str] = &[
    "migration_add_command",
    "schema_check_command",
    "schema_dump_command",
    "sqlx_check_command",
];
const NATIVE_TOOLS: &[NativeToolDescriptor] = &[
    NativeToolDescriptor::new(tool::MIGRATION_ADD, true, NativeToolKind::MigrationAdd),
    NativeToolDescriptor::new(tool::SCHEMA_CHECK, false, NativeToolKind::SchemaCheck),
];

pub const FEATURE: FeatureDescriptor = FeatureDescriptor::new(
    COMMAND_KEYS,
    NATIVE_TOOLS,
    required_tools,
    unavailable_tool_message,
);

fn required_tools(ctx: &dyn FeatureContext) -> Vec<&'static str> {
    let mut required = Vec::new();
    if ctx.sqlx_enabled()
        || ctx.has_required_key(legacy_make_target::SQLX_CHECK, "sqlx_check_command")
    {
        required.push(tool::SQLX_CHECK);
    }
    if ctx.sqlx_enabled()
        || ctx.has_required_key(legacy_make_target::MIGRATION_ADD, "migration_add_command")
    {
        required.push(tool::MIGRATION_ADD);
    }
    if (ctx.sqlx_enabled() && ctx.schema_dump_enabled())
        || ctx.has_required_key(legacy_make_target::SCHEMA_CHECK, "schema_check_command")
    {
        required.push(tool::SCHEMA_CHECK);
    }
    if (ctx.sqlx_enabled() && ctx.schema_dump_enabled())
        || ctx.has_required_key(legacy_make_target::SCHEMA_DUMP, "schema_dump_command")
    {
        required.push(tool::SCHEMA_DUMP);
    }
    required
}

fn unavailable_tool_message(ctx: &dyn FeatureContext, tool_name: &str) -> Option<String> {
    match tool_name {
        tool::SCHEMA_CHECK | tool::SCHEMA_DUMP if !ctx.sqlx_enabled() => Some(format!(
            "{tool_name} is not available because sqlx_enabled = false in .jig.toml. Enable SQLx and schema dumps, then run `jig update --recopy`, or remove this command/gate."
        )),
        tool::SCHEMA_CHECK | tool::SCHEMA_DUMP if !ctx.schema_dump_enabled() => Some(format!(
            "{tool_name} is not available because schema_dump_enabled = false in .jig.toml. Enable schema dumps, then run `jig update --recopy`, or remove this command/gate."
        )),
        tool::SQLX_CHECK | tool::MIGRATION_ADD if !ctx.sqlx_enabled() => Some(format!(
            "{tool_name} is not available because sqlx_enabled = false in .jig.toml. Enable SQLx, then run `jig update --recopy`, or remove this command/gate."
        )),
        _ => None,
    }
}
