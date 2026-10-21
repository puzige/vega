mod runner;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let exit_code = runner::run_cli().await;
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
