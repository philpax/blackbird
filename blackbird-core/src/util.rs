/// Convert a number of seconds to a string in the format "HH:MM:SS".
/// If the number of hours is 0, it will be omitted.
pub fn seconds_to_hms_string(seconds: u32) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{}:{:02}", minutes, seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seconds_to_hms_string() {
        // Test with hours
        assert_eq!(seconds_to_hms_string(3661), "1:01:01");
        assert_eq!(seconds_to_hms_string(7323), "2:02:03");
        assert_eq!(seconds_to_hms_string(3600), "1:00:00");

        // Test without hours
        assert_eq!(seconds_to_hms_string(61), "1:01");
        assert_eq!(seconds_to_hms_string(123), "2:03");
        assert_eq!(seconds_to_hms_string(60), "1:00");

        // Test edge cases
        assert_eq!(seconds_to_hms_string(0), "0:00");
        assert_eq!(seconds_to_hms_string(59), "0:59");
    }
}
