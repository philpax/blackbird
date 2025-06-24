/// Convert a number of seconds to a string in the format "HH:MM:SS".
/// If the number of hours is 0, it will be omitted.
///
/// # Arguments
/// * `seconds` - The number of seconds to convert
/// * `pad_first` - Whether to zero-pad the first segment (hours when present, or minutes when hours are 0)
pub fn seconds_to_hms_string(seconds: u32, pad_first: bool) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;

    #[allow(clippy::collapsible_else_if)]
    if hours > 0 {
        if pad_first {
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{}:{:02}:{:02}", hours, minutes, seconds)
        }
    } else {
        if pad_first {
            format!("{:02}:{:02}", minutes, seconds)
        } else {
            format!("{}:{:02}", minutes, seconds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seconds_to_hms_string_padded() {
        // Test with hours, padded
        assert_eq!(seconds_to_hms_string(3661, true), "01:01:01");
        assert_eq!(seconds_to_hms_string(7323, true), "02:02:03");
        assert_eq!(seconds_to_hms_string(3600, true), "01:00:00");

        // Test without hours, padded
        assert_eq!(seconds_to_hms_string(61, true), "01:01");
        assert_eq!(seconds_to_hms_string(123, true), "02:03");
        assert_eq!(seconds_to_hms_string(60, true), "01:00");

        // Test edge cases, padded
        assert_eq!(seconds_to_hms_string(0, true), "00:00");
        assert_eq!(seconds_to_hms_string(59, true), "00:59");
    }

    #[test]
    fn test_seconds_to_hms_string_unpadded() {
        // Test with hours, unpadded
        assert_eq!(seconds_to_hms_string(3661, false), "1:01:01");
        assert_eq!(seconds_to_hms_string(7323, false), "2:02:03");
        assert_eq!(seconds_to_hms_string(3600, false), "1:00:00");

        // Test without hours, unpadded
        assert_eq!(seconds_to_hms_string(61, false), "1:01");
        assert_eq!(seconds_to_hms_string(123, false), "2:03");
        assert_eq!(seconds_to_hms_string(60, false), "1:00");

        // Test edge cases, unpadded
        assert_eq!(seconds_to_hms_string(0, false), "0:00");
        assert_eq!(seconds_to_hms_string(59, false), "0:59");
    }
}
