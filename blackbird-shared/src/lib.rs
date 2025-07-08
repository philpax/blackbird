pub mod config {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(default)]
    pub struct Server {
        pub base_url: String,
        pub username: String,
        pub password: String,
        pub transcode: bool,
    }
    impl Default for Server {
        fn default() -> Self {
            Self {
                base_url: "http://localhost:4533".to_string(),
                username: "YOUR_USERNAME".to_string(),
                password: "YOUR_PASSWORD".to_string(),
                transcode: false,
            }
        }
    }
}
