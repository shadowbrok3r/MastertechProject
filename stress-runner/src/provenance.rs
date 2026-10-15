//! Build identity stamped on every `stress_test_run` row as `tool_version`.

use std::sync::OnceLock;

static HOST_BUILD: OnceLock<String> = OnceLock::new();

/// Records the host app and its build, e.g. `("MasterTech", "5.0.1+ec3ea6b.e8a3c1")`; the first call wins.
pub fn set_host_build(app: &str, build: &str) {
    register(&HOST_BUILD, app, build);
}

fn register(cell: &OnceLock<String>, app: &str, build: &str) {
    let _ = cell.set(format!("{app} {build}"));
}

/// The registered host build, or stress-runner's own version when no host registered one.
pub fn host_build() -> String {
    HOST_BUILD
        .get()
        .cloned()
        .unwrap_or_else(|| format!("stress-runner {}", env!("CARGO_PKG_VERSION")))
}

/// `tool_version` for runs this process executes: `"{host} {build}; stress-kit {build}"`.
pub fn tool_version() -> String {
    compose(&host_build(), stress_kit::BUILD_ID)
}

fn compose(host: &str, kit: &str) -> String {
    format!("{host}; stress-kit {kit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_host_and_kit_builds() {
        assert_eq!(
            compose("MasterTech 5.0.1+abc1234.000001", "0.1.0+abc1234.000002"),
            "MasterTech 5.0.1+abc1234.000001; stress-kit 0.1.0+abc1234.000002"
        );
    }

    #[test]
    fn tool_version_always_names_stress_kit_and_a_host() {
        let v = tool_version();
        let (host, kit) = v.split_once("; stress-kit ").expect("host; stress-kit kit");
        assert!(!host.is_empty());
        assert_eq!(kit, stress_kit::BUILD_ID);
    }

    #[test]
    fn first_registered_host_wins() {
        let cell = OnceLock::new();
        register(&cell, "first-app", "1.0.0+aaaaaaa");
        register(&cell, "second-app", "2.0.0+bbbbbbb");
        assert_eq!(
            cell.get().map(String::as_str),
            Some("first-app 1.0.0+aaaaaaa")
        );
    }
}
