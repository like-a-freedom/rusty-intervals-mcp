#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    intervals_icu_mcp::run().await
}
