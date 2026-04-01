// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use std::path::Path;

/// Handle the `+upload` subcommand.
pub(super) async fn handle_upload(
    doc: &crate::discovery::RestDescription,
    matches: &ArgMatches,
) -> Result<(), GwsError> {
    let file_path = matches.get_one::<String>("file").unwrap();
    let parent_id = matches.get_one::<String>("parent");
    let name_arg = matches.get_one::<String>("name");

    let filename = determine_filename(file_path, name_arg.map(|s| s.as_str()))?;

    let files_res = doc
        .resources
        .get("files")
        .ok_or_else(|| GwsError::Discovery("Resource 'files' not found".to_string()))?;
    let create_method = files_res
        .methods
        .get("create")
        .ok_or_else(|| GwsError::Discovery("Method 'files.create' not found".to_string()))?;

    let metadata = build_upload_metadata(&filename, parent_id.map(|s| s.as_str()));
    let body_str = metadata.to_string();

    let scopes: Vec<&str> = create_method.scopes.iter().map(|s| s.as_str()).collect();
    let (token, auth_method) = match auth::get_token(&scopes).await {
        Ok(t) => (Some(t), executor::AuthMethod::OAuth),
        Err(_) if matches.get_flag("dry-run") => (None, executor::AuthMethod::None),
        Err(e) => return Err(GwsError::Auth(format!("Drive auth failed: {e}"))),
    };

    executor::execute_method(
        doc,
        create_method,
        None,
        Some(&body_str),
        token.as_deref(),
        auth_method,
        None,
        Some(executor::UploadSource::File {
            path: file_path,
            content_type: None,
        }),
        matches.get_flag("dry-run"),
        &executor::PaginationConfig::default(),
        None,
        &crate::helpers::modelarmor::SanitizeMode::Warn,
        &crate::formatter::OutputFormat::default(),
        false,
    )
    .await?;

    Ok(())
}

fn determine_filename(file_path: &str, name_arg: Option<&str>) -> Result<String, GwsError> {
    if let Some(n) = name_arg {
        Ok(n.to_string())
    } else {
        Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| GwsError::Validation("Invalid file path".to_string()))
    }
}

fn build_upload_metadata(filename: &str, parent_id: Option<&str>) -> Value {
    let mut metadata = json!({
        "name": filename
    });

    if let Some(parent) = parent_id {
        metadata["parents"] = json!([parent]);
    }

    metadata
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_filename_explicit() {
        assert_eq!(
            determine_filename("path/to/file.txt", Some("custom.txt")).unwrap(),
            "custom.txt"
        );
    }

    #[test]
    fn test_determine_filename_from_path() {
        assert_eq!(
            determine_filename("path/to/file.txt", None).unwrap(),
            "file.txt"
        );
    }

    #[test]
    fn test_determine_filename_invalid_path() {
        assert!(determine_filename("", None).is_err());
        assert!(determine_filename("/", None).is_err());
    }

    #[test]
    fn test_build_metadata_no_parent() {
        let meta = build_upload_metadata("file.txt", None);
        assert_eq!(meta["name"], "file.txt");
        assert!(meta.get("parents").is_none());
    }

    #[test]
    fn test_build_metadata_with_parent() {
        let meta = build_upload_metadata("file.txt", Some("folder123"));
        assert_eq!(meta["name"], "file.txt");
        assert_eq!(meta["parents"][0], "folder123");
    }
}
