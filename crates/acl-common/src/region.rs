use std::env;

pub fn is_mainland_china() -> bool {
    // Environment variable bypass.
    if let Ok(val) = env::var("ACL_REGION") {
        match val.to_uppercase().as_str() {
            "CN" | "CHINA" => return true,
            _ => return false,
        }
    }

    // Check the system locale.
    if let Some(locale) = sys_locale::get_locale() {
        let normalized = locale.to_uppercase().replace('_', "-");

        if normalized.ends_with("-CN") || normalized.contains("-CN-") {
            return true;
        }
    }

    // Check the system time zone.
    if let Ok(tz) = iana_time_zone::get_timezone()
        && (tz == "Asia/Shanghai" || tz == "Asia/Urumqi" || tz == "PRC")
    {
        // The time zone is UTC+8 indicates mainland China.
        return true;
    }

    // Default to falling back to non-mainland-China.
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, val: &str) -> Self {
            let original = env::var(key).ok();
            // SAFETY: Tests serialize access via ENV_LOCK to avoid concurrent env modification.
            unsafe {
                env::set_var(key, val);
            }
            Self { key, original }
        }

        fn remove(key: &'static str) -> Self {
            let original = env::var(key).ok();
            // SAFETY: Tests serialize access via ENV_LOCK to avoid concurrent env modification.
            unsafe {
                env::remove_var(key);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: Tests serialize access via ENV_LOCK to avoid concurrent env modification.
            unsafe {
                match &self.original {
                    Some(val) => env::set_var(self.key, val),
                    None => env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn test_is_mainland_china_env_override() {
        let _lock = ENV_LOCK.lock().expect("Failed to lock ENV_LOCK");

        {
            let _guard = EnvGuard::set("ACL_REGION", "CN");
            assert!(is_mainland_china());
        }

        {
            let _guard = EnvGuard::set("ACL_REGION", "china");
            assert!(is_mainland_china());
        }

        {
            let _guard = EnvGuard::set("ACL_REGION", "US");
            assert!(!is_mainland_china());
        }

        {
            let _guard = EnvGuard::set("ACL_REGION", "GLOBAL");
            assert!(!is_mainland_china());
        }
    }

    #[test]
    fn test_is_mainland_china_fallback_deterministic() {
        let _lock = ENV_LOCK.lock().expect("Failed to lock ENV_LOCK");
        let _guard = EnvGuard::remove("ACL_REGION");
        let _result: bool = is_mainland_china();
    }
}
