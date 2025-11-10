#[tokio::main]
async fn main() {
    rust_rock_paper_scissors_server::server::start_server().await;
}
