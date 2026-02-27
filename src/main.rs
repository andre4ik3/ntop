use crate::app::App;

pub mod app;
pub mod ps;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let terminal = ratatui::init();
    let result = App::new().run(terminal).await;
    ratatui::restore();
    result
}
