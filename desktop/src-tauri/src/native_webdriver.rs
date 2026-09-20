//! Explicit opt-in for an instrumented binary; ordinary packages omit this module.

pub fn port() -> u16 {
    let value = std::env::var("SOTTO_NATIVE_E2E_PORT").ok();
    match parse_port(value.as_deref()) {
        Some(port) => port,
        None => {
            // Refuse before logging, configuration access or plugin setup.
            eprintln!("SOTTO_NATIVE_E2E_ONLY_V1: set SOTTO_NATIVE_E2E_PORT in an isolated test session (1024..65535)");
            std::process::exit(2);
        }
    }
}

fn parse_port(value: Option<&str>) -> Option<u16> {
    value?.parse::<u16>().ok().filter(|port| *port >= 1024)
}

#[cfg(test)]
mod tests {
    use super::parse_port;

    #[test]
    fn refuses_implicit_or_invalid_server_configuration() {
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("80"),
            Some("65536"),
            Some("localhost:4445"),
        ] {
            assert_eq!(parse_port(value), None);
        }
        assert_eq!(parse_port(Some("4445")), Some(4445));
        assert_eq!(parse_port(Some("65535")), Some(65535));
    }
}
