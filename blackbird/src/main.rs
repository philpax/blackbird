use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    base_url: String,
    username: String,
    password: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = toml::from_str::<Config>(&std::fs::read_to_string("config.toml")?)?;
    let client = blackbird_subsonic::Client::new(
        config.base_url,
        config.username,
        config.password,
        "blackbird".to_string(),
    );

    println!("{}", client.ping().await?);
    Ok(())
}
