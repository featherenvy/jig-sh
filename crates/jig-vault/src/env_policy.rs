pub(crate) fn is_preserved_env_var_name(name: &str) -> bool {
    should_preserve_env_var(name, PRESERVED_ENV_EXACT)
}

pub(crate) fn env_var_names_equal(left: &str, right: &str) -> bool {
    env_var_names_equal_inner(left, right)
}

fn should_preserve_env_var(name: &str, exact: &[&str]) -> bool {
    // Exact matching is deliberate: do not reintroduce prefix forwarding.
    exact
        .iter()
        .any(|preserved| env_var_names_equal_inner(name, preserved))
}

#[cfg(windows)]
fn env_var_names_equal_inner(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(not(windows))]
fn env_var_names_equal_inner(left: &str, right: &str) -> bool {
    left == right
}

#[cfg(unix)]
const PRESERVED_ENV_EXACT: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "TEMP",
    "TMP",
    "LANG",
    "LC_ALL",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
];

#[cfg(windows)]
const PRESERVED_ENV_EXACT: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "USERPROFILE",
    "USERNAME",
    "TEMP",
    "TMP",
];

#[cfg(not(any(unix, windows)))]
const PRESERVED_ENV_EXACT: &[&str] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_environment_does_not_preserve_arbitrary_lc_names() {
        assert!(should_preserve_env_var("LC_TIME", &["LC_TIME"]));
        assert!(!should_preserve_env_var(
            "LC_MALICIOUS",
            &["LC_ALL", "LC_CTYPE", "LC_TIME"]
        ));
    }

    #[test]
    fn preserved_environment_name_case_follows_platform_rules() {
        assert!(should_preserve_env_var("PATH", &["PATH"]));
        #[cfg(windows)]
        assert!(should_preserve_env_var("Path", &["PATH"]));
        #[cfg(not(windows))]
        assert!(!should_preserve_env_var("Path", &["PATH"]));
    }
}
