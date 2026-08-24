#[tokio::main]
async fn main() -> anyhow::Result<()> {
    deepsave_backend::run().await
}
