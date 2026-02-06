use chrono::Local;

pub fn timestamp_prefix() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
