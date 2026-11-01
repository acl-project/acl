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
