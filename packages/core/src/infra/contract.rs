pub const CONTRACT_VERSION: &str = "1.0";
pub const CONTRACT_VERSION_HEADER: &str = "x-refine-contract-version";

pub fn normalize_contract_major(version: &str) -> &str {
    let version = version.trim();
    version.split('.').next().unwrap_or(version)
}

pub fn is_contract_compatible(client_version: &str, server_version: &str) -> bool {
    let client_major = normalize_contract_major(client_version);
    let server_major = normalize_contract_major(server_version);
    !client_major.is_empty() && client_major == server_major
}

#[cfg(test)]
mod tests {
    use super::{is_contract_compatible, normalize_contract_major};

    #[test]
    fn normalize_contract_major_uses_first_segment() {
        assert_eq!(normalize_contract_major("1.2.3"), "1");
        assert_eq!(normalize_contract_major(" 2 "), "2");
    }

    #[test]
    fn is_contract_compatible_checks_major_version() {
        assert!(is_contract_compatible("1.0", "1.2.0"));
        assert!(is_contract_compatible("1.9", "1.0"));
        assert!(!is_contract_compatible("2.0", "1.0"));
        assert!(!is_contract_compatible("", "1.0"));
    }
}
