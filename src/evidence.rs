use anyhow::Result;
use std::path::Path;

use crate::storage;
use crate::summarize;

pub fn run(id: String) -> Result<i32> {
    let paths = storage::Paths::discover()?;
    let entry = if id == "last" {
        storage::last_run(&paths)?
    } else {
        storage::resolve_run_id(&paths, &id)?
    };
    let sidecar = storage::read_sidecar(Path::new(&entry.summary_path))?;
    print!("{}", summarize::format_evidence(&sidecar));
    Ok(0)
}
