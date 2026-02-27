use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildProcess {
    pub argv: Vec<String>,
    pub parent_pid: usize,
    pub pid: usize,
    pub stime: f64,
    pub utime: f64,
    // !!! other stuff might be null !!!
    // actually I don't know, even these might be null as well...
    // BUT I checked, at least on Linux and macOS, these seem to not be null
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    pub derivation: String,
    pub main_pid: usize,
    pub nix_pid: usize,
    pub processes: Vec<BuildProcess>,
    // same warning as above
    // only add stuff that we need !!!
}

pub type Output = Vec<Build>;

// meant to use like ps::get() instead of use ps::get and then get()
pub async fn get() -> anyhow::Result<Output> {
    let cmd = Command::new("nix").arg("ps").arg("--json").output().await?;
    Ok(serde_json::from_slice(&cmd.stdout)?)
}
