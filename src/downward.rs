use std::path::{MAIN_SEPARATOR, Path};

use anyhow::Result;
use walkdir::WalkDir;

use crate::metrics::Metrics;

/// Načte všechny soubory z DownwardAPI volume a vytvoří
/// kubernetes_downward_info{field="...", value="..."} 1
pub fn init_downward_info(metrics: &Metrics, dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path());
            let field = rel.to_string_lossy().replace(MAIN_SEPARATOR, "/");
            let value = std::fs::read_to_string(entry.path())
                .unwrap_or_default()
                .trim()
                .to_string();

            metrics
                .downward_info
                .with_label_values(&[&field, &value])
                .set(1);
        }
    }

    Ok(())
}
