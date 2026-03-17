pub fn schedule_retry(max_attempts: u8) -> &'static str {
    if max_attempts > 3 {
        "manual-review"
    } else {
        "retry-queue"
    }
}

pub fn standing_data_version() -> &'static str {
    "customer-profile-v3"
}
