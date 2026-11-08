#[tokio::main]
async fn main() -> anyhow::Result<()> {
    recall_mcp_cli::run().await
}
