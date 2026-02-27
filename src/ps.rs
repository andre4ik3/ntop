use chrono::{DateTime, TimeDelta, Utc};
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
    pub start_time: f64,
    // same warning as above
    // only add stuff that we need !!!
}

impl Build {
    pub fn elapsed(&self) -> TimeDelta {
        let start_time = DateTime::from_timestamp_secs(self.start_time as i64)
            .expect("failed to convert millis to datetime??");
        let now = Utc::now();
        now - start_time
    }
}

pub type Output = Vec<Build>;

// meant to use like ps::get() instead of use ps::get and then get()
pub async fn get() -> anyhow::Result<Output> {
    let cmd = Command::new("nix").arg("ps").arg("--json").output().await?;
    let mut data: Output = serde_json::from_slice(&cmd.stdout)?;
    data.sort_by(|a, b| a.derivation.cmp(&b.derivation));
    Ok(data)
}
