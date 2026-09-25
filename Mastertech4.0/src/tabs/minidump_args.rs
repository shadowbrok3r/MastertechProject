use std::path::PathBuf;

/// Minidump tab inputs parsed by the top-level command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MinidumpArgs {
    pub minidumps: Vec<PathBuf>,
    pub symbols_path: Vec<String>,
    pub symbols_url: Vec<String>,
}

impl MinidumpArgs {
    /// Arguments to register on the top-level `clap::Command`.
    pub fn clap_args() -> [clap::Arg; 3] {
        [
            clap::Arg::new("minidumps")
                .value_name("MINIDUMP")
                .help("Minidump files to list in the Minidump tab")
                .value_parser(clap::value_parser!(PathBuf))
                .action(clap::ArgAction::Append),
            clap::Arg::new("symbols-path")
                .long("symbols-path")
                .value_name("PATH")
                .help("Symbol directory for the Minidump tab (repeatable)")
                .action(clap::ArgAction::Append),
            clap::Arg::new("symbols-url")
                .long("symbols-url")
                .value_name("URL")
                .help("Symbol server for the Minidump tab (repeatable)")
                .action(clap::ArgAction::Append),
        ]
    }

    pub fn from_matches(matches: &clap::ArgMatches) -> Self {
        fn all<T: Clone + Send + Sync + 'static>(matches: &clap::ArgMatches, id: &str) -> Vec<T> {
            matches.get_many::<T>(id).into_iter().flatten().cloned().collect()
        }
        Self {
            minidumps: all(matches, "minidumps"),
            symbols_path: all(matches, "symbols-path"),
            symbols_url: all(matches, "symbols-url"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minidump_args_parse_alongside_gui_flags() {
        let matches = crate::cli()
            .try_get_matches_from([
                "MasterTech", "--cpu", "--no-frost", "-l", "-c", "a.dmp", "b.dmp",
                "--symbols-path", r"C:\sym", "--symbols-url", "https://sym.example/",
            ])
            .expect("GUI flags and minidump args parse together");
        assert!(matches.get_flag("no-frost"));
        assert_eq!(
            MinidumpArgs::from_matches(&matches),
            MinidumpArgs {
                minidumps: vec![PathBuf::from("a.dmp"), PathBuf::from("b.dmp")],
                symbols_path: vec![r"C:\sym".to_string()],
                symbols_url: vec!["https://sym.example/".to_string()],
            }
        );
    }
}
