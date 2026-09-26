//! Staff email normalization shared by sign-in, sign-up and user lookups.

/// Company mail domains, the default first.
pub const COMPANY_EMAIL_DOMAINS: [&str; 2] = ["pclaptops.com", "xidax.com"];

/// Suffix the pre-4.8.5 login appended to every input that lacked it.
const LEGACY_APPENDED_SUFFIX: &str = "@pclaptops.com";

/// Trimmed, lowercased email; a bare username gets `@` plus the default company domain.
pub fn normalize_email(input: &str) -> Option<String> {
    normalize_email_with(input, COMPANY_EMAIL_DOMAINS[0])
}

/// Trimmed, lowercased email; a bare username gets `@` plus `domain`.
pub fn normalize_email_with(input: &str, domain: &str) -> Option<String> {
    let input = input.trim().to_lowercase();
    if input.is_empty() || input.chars().any(char::is_whitespace) {
        return None;
    }
    let email = if input.contains('@') {
        input
    } else {
        let domain = domain.trim().trim_start_matches('@').to_lowercase();
        format!("{input}@{domain}")
    };
    let (local, host) = email.split_once('@')?;
    let valid = !local.is_empty()
        && !host.is_empty()
        && !host.contains('@')
        && !host.chars().any(char::is_whitespace);
    valid.then_some(email)
}

/// Every address `ident` can mean: itself when it has an `@`, else one per company domain.
pub fn email_candidates(ident: &str) -> Vec<String> {
    let ident = ident.trim();
    if ident.contains('@') {
        normalize_email(ident).into_iter().collect()
    } else {
        COMPANY_EMAIL_DOMAINS
            .iter()
            .filter_map(|domain| normalize_email_with(ident, domain))
            .collect()
    }
}

/// The part before `@`, or the whole input when there is none.
pub fn short_name(email: &str) -> &str {
    email.split('@').next().unwrap_or(email)
}

/// Remainder of the company domain `input` is heading toward, `preferred` tried first.
pub fn domain_suffix(input: &str, preferred: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_whitespace) {
        return None;
    }
    let preferred = preferred.trim().trim_start_matches('@').to_lowercase();
    let Some((local, typed)) = input.split_once('@') else {
        return (!preferred.is_empty()).then(|| format!("@{preferred}"));
    };
    if local.is_empty() || typed.contains('@') {
        return None;
    }
    let typed = typed.to_lowercase();
    std::iter::once(preferred.as_str())
        .chain(COMPANY_EMAIL_DOMAINS)
        .filter(|domain| !domain.is_empty())
        .find(|domain| domain.len() > typed.len() && domain.starts_with(typed.as_str()))
        .map(|domain| domain[typed.len()..].to_string())
}

/// Strips one trailing `@pclaptops.com` that the old login appended to a full address.
pub fn repair_legacy_email(input: &str) -> String {
    let input = input.trim();
    let doubled = input.matches('@').count() >= 2;
    let suffix_at = input.len().saturating_sub(LEGACY_APPENDED_SUFFIX.len());
    match input.get(suffix_at..) {
        Some(tail) if doubled && tail.eq_ignore_ascii_case(LEGACY_APPENDED_SUFFIX) => {
            input[..suffix_at].to_string()
        }
        _ => input.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_username_gets_the_default_domain() {
        assert_eq!(normalize_email("bob").as_deref(), Some("bob@pclaptops.com"));
        assert_eq!(
            normalize_email(" Bob.Smith ").as_deref(),
            Some("bob.smith@pclaptops.com")
        );
    }

    #[test]
    fn a_typed_domain_is_kept_and_lowercased() {
        assert_eq!(
            normalize_email(" Chris@XIDAX.com ").as_deref(),
            Some("chris@xidax.com")
        );
        assert_eq!(
            normalize_email("a@gmail.com").as_deref(),
            Some("a@gmail.com")
        );
    }

    #[test]
    fn the_chosen_domain_completes_a_bare_username() {
        assert_eq!(
            normalize_email_with("chris", "xidax.com").as_deref(),
            Some("chris@xidax.com")
        );
        assert_eq!(
            normalize_email_with("chris", "@XIDAX.com").as_deref(),
            Some("chris@xidax.com")
        );
        assert_eq!(
            normalize_email_with("chris@pclaptops.com", "xidax.com").as_deref(),
            Some("chris@pclaptops.com")
        );
        assert_eq!(normalize_email_with("chris", ""), None);
    }

    #[test]
    fn malformed_input_normalizes_to_none() {
        for input in ["", "   ", "a b@c.com", "a@b@c", "@x.com", "bob@", "a@b c"] {
            assert_eq!(normalize_email(input), None, "input {input:?}");
        }
    }

    #[test]
    fn candidates_cover_every_company_domain_in_order() {
        assert_eq!(
            email_candidates(" John.Doe "),
            vec![
                "john.doe@pclaptops.com".to_string(),
                "john.doe@xidax.com".to_string()
            ]
        );
        assert_eq!(
            email_candidates("John@Xidax.com"),
            vec!["john@xidax.com".to_string()]
        );
        assert!(email_candidates("  ").is_empty());
        assert!(email_candidates("john doe").is_empty());
        assert!(email_candidates("a@b@c").is_empty());
    }

    #[test]
    fn short_name_is_the_local_part() {
        assert_eq!(short_name("bob.smith@pclaptops.com"), "bob.smith");
        assert_eq!(short_name("bob"), "bob");
        assert_eq!(short_name(""), "");
    }

    #[test]
    fn domain_suffix_completes_toward_a_company_domain() {
        assert_eq!(domain_suffix("", "pclaptops.com"), None);
        assert_eq!(
            domain_suffix("bob", "pclaptops.com").as_deref(),
            Some("@pclaptops.com")
        );
        assert_eq!(
            domain_suffix("bob", "xidax.com").as_deref(),
            Some("@xidax.com")
        );
        assert_eq!(
            domain_suffix("bob@", "pclaptops.com").as_deref(),
            Some("pclaptops.com")
        );
        assert_eq!(
            domain_suffix("bob@", "xidax.com").as_deref(),
            Some("xidax.com")
        );
        assert_eq!(
            domain_suffix("bob@x", "pclaptops.com").as_deref(),
            Some("idax.com")
        );
        assert_eq!(
            domain_suffix("bob@P", "xidax.com").as_deref(),
            Some("claptops.com")
        );
        assert_eq!(domain_suffix("bob@xidax.com", "pclaptops.com"), None);
        assert_eq!(domain_suffix("bob@gmail", "pclaptops.com"), None);
        assert_eq!(domain_suffix("@x", "pclaptops.com"), None);
        assert_eq!(domain_suffix("a@b@", "pclaptops.com"), None);
    }

    #[test]
    fn a_doubled_legacy_suffix_is_stripped_once() {
        assert_eq!(
            repair_legacy_email("christopher@xidax.com@pclaptops.com"),
            "christopher@xidax.com"
        );
        assert_eq!(
            repair_legacy_email(" bob@pclaptops.com "),
            "bob@pclaptops.com"
        );
        assert_eq!(repair_legacy_email("bob"), "bob");
        assert_eq!(repair_legacy_email("a@b@c"), "a@b@c");
        assert_eq!(
            normalize_email(&repair_legacy_email("Chris@Xidax.com@pclaptops.com")).as_deref(),
            Some("chris@xidax.com")
        );
    }
}
