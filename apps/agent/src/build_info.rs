//! Identidad embebida; una compilación de desarrollo no pretende ser un release.
const COMMIT: &str = match option_env!("ATENEA_BUILD_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};

pub(crate) fn print_version(name: &str) -> bool {
    if std::env::args()
        .nth(1)
        .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!("{name} {} ({COMMIT})", env!("CARGO_PKG_VERSION"));
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn commit_is_embedded() {
        assert_eq!(
            super::COMMIT,
            option_env!("ATENEA_BUILD_COMMIT").unwrap_or("unknown")
        );
        assert!(!super::COMMIT.is_empty());
    }
}
