use crate::error::GwsError;
use std::fs;

pub async fn handle_cache_command(args: &[String]) -> Result<(), GwsError> {
    if args.is_empty() {
        return Err(GwsError::Validation("Usage: gws cache clear".to_string()));
    }

    match args[0].as_str() {
        "clear" => {
            let cache_dir = crate::auth_commands::config_dir().join("cache");
            if cache_dir.exists() {
                fs::remove_dir_all(&cache_dir)
                    .map_err(|e| GwsError::Validation(format!("Failed to clear cache: {e}")))?;
                println!("Discovery cache cleared.");
            } else {
                println!("Discovery cache is already empty.");
            }
            Ok(())
        }
        other => Err(GwsError::Validation(format!(
            "Unknown cache command '{other}'. Usage: gws cache clear"
        ))),
    }
}
