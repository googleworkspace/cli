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

//! JSON Schema Validation & Reference Resolution
//!
//! Provides utilities to validate JSON payloads against the Google API Discovery Document
//! schemas before dispatching requests. This ensures immediate client-side feedback
//! for invalid API payloads.

use serde_json::{json, Value};

use crate::discovery::{
    fetch_discovery_document, JsonSchema, MethodParameter, RestDescription, RestMethod,
    RestResource,
};
use crate::error::GwsError;
use crate::services::resolve_service;

/// Handles the `gws schema <dotted.path>` command.
///
/// Path format: `service.resource[.subresource].method`
/// Example: `drive.files.list` or `drive.files.permissions.list`
pub async fn handle_schema_command(path: &str, resolve_refs: bool) -> Result<(), GwsError> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.len() < 2 {
        return Err(GwsError::Validation(format!(
            "Schema path must be at least 'service.Message' or 'service.resource.method', got '{path}'"
        )));
    }

    let service_name = parts[0];
    let (api_name, version) = resolve_service(service_name)?;

    let doc = fetch_discovery_document(&api_name, &version)
        .await
        .map_err(|e| GwsError::Discovery(format!("{e:#}")))?;

    // Case 1: Schema lookup (e.g., "drive.File")
    if parts.len() == 2 {
        let schema_name = parts[1];
        if let Some(schema) = doc.schemas.get(schema_name) {
            let mut output = schema_to_json(schema);
            if resolve_refs {
                let mut seen = std::collections::HashSet::new();
                // Add self to seen to prevent immediate recursion
                seen.insert(schema_name.to_string());
                resolve_schema_refs(&mut output, &doc, &mut seen);
            }

            println!(
                "{}",
                serde_json::to_string_pretty(&output).unwrap_or_default()
            );
            return Ok(());
        } else {
            // It might be a resource path that is incomplete, but let's see if it's a schema typo first
            // or perhaps the user meant "drive.files" (resource) which we don't support dumping yet.
            // Let's check if it matches a resource name to give a better error.
            if doc.resources.contains_key(schema_name) {
                return Err(GwsError::Validation(format!(
                    "'{schema_name}' is a resource. To see its methods, try 'gws schema {service_name}.{schema_name}.list' (or similar). To see a type definition, try 'gws schema {service_name}.<Type>'."
                )));
            }

            let available: Vec<&String> = doc.schemas.keys().collect();
            return Err(GwsError::Validation(format!(
                "Schema or resource '{schema_name}' not found. Available schemas: {:?}",
                available
            )));
        }
    }

    // Case 2: Method lookup (e.g., "drive.files.list")
    let resource_path = &parts[1..parts.len() - 1];
    let method_name = parts[parts.len() - 1];

    let method = find_method(&doc, resource_path, method_name)?;

    let mut output = build_schema_output(&doc, method);
    if resolve_refs {
        let mut seen = std::collections::HashSet::new();
        resolve_schema_refs(&mut output, &doc, &mut seen);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_default()
    );

    Ok(())
}

/// Walks the resource tree to find a method.
fn find_method<'a>(
    doc: &'a RestDescription,
    resource_path: &[&str],
    method_name: &str,
) -> Result<&'a RestMethod, GwsError> {
    if resource_path.is_empty() {
        return Err(GwsError::Validation(
            "Resource path cannot be empty".to_string(),
        ));
    }

    let first_resource_name = resource_path[0];
    let resource = doc.resources.get(first_resource_name).ok_or_else(|| {
        let available: Vec<&String> = doc.resources.keys().collect();
        GwsError::Validation(format!(
            "Resource '{}' not found. Available resources: {:?}",
            first_resource_name, available
        ))
    })?;

    // Walk deeper into sub-resources
    let mut current_resource: &RestResource = resource;
    for &sub_name in &resource_path[1..] {
        current_resource = current_resource.resources.get(sub_name).ok_or_else(|| {
            let available: Vec<&String> = current_resource.resources.keys().collect();
            GwsError::Validation(format!(
                "Sub-resource '{}' not found. Available: {:?}",
                sub_name, available
            ))
        })?;
    }

    current_resource.methods.get(method_name).ok_or_else(|| {
        let available: Vec<&String> = current_resource.methods.keys().collect();
        GwsError::Validation(format!(
            "Method '{}' not found. Available methods: {:?}",
            method_name, available
        ))
    })
}

/// Builds the schema output JSON for a method.
fn build_schema_output(doc: &RestDescription, method: &RestMethod) -> Value {
    let mut params = json!({});
    for (name, param) in &method.parameters {
        params[name] = param_to_json(param);
    }

    let mut output = json!({
        "httpMethod": method.http_method,
        "path": method.path,
        "description": method.description.as_deref().unwrap_or(""),
        "parameters": params,
        "scopes": method.scopes,
    });

    if !method.parameter_order.is_empty() {
        output["parameterOrder"] = json!(method.parameter_order);
    }

    // Resolve request body schema
    if let Some(ref req_ref) = method.request {
        if let Some(ref schema_name) = req_ref.schema_ref {
            output["requestBody"] = json!({
                "schemaRef": schema_name,
            });
            if let Some(schema) = doc.schemas.get(schema_name) {
                output["requestBody"]["schema"] = schema_to_json(schema);
            }
        }
    }

    // Response schema ref
    if let Some(ref resp_ref) = method.response {
        if let Some(ref schema_name) = resp_ref.schema_ref {
            output["response"] = json!({
                "schemaRef": schema_name,
            });
            // Also inline the response schema structure if available
            if let Some(schema) = doc.schemas.get(schema_name) {
                output["response"]["schema"] = schema_to_json(schema);
            }
        }
    }

    output
}

fn param_to_json(param: &MethodParameter) -> Value {
    let mut p = json!({
        "type": param.param_type.as_deref().unwrap_or("string"),
        "required": param.required,
    });

    if let Some(ref loc) = param.location {
        p["location"] = json!(loc);
    }
    if let Some(ref desc) = param.description {
        p["description"] = json!(desc);
    }
    if let Some(ref fmt) = param.format {
        p["format"] = json!(fmt);
    }
    if let Some(ref def) = param.default {
        p["default"] = json!(def);
    }
    if let Some(ref vals) = param.enum_values {
        p["enum"] = json!(vals);
    }
    if param.repeated {
        p["repeated"] = json!(true);
    }
    if param.deprecated {
        p["deprecated"] = json!(true);
    }

    p
}

fn schema_to_json(schema: &JsonSchema) -> Value {
    let mut s = json!({});

    if let Some(ref t) = schema.schema_type {
        s["type"] = json!(t);
    }
    if let Some(ref desc) = schema.description {
        s["description"] = json!(desc);
    }

    if !schema.properties.is_empty() {
        let mut props = json!({});
        for (name, prop) in &schema.properties {
            let mut p = json!({});
            if let Some(ref t) = prop.prop_type {
                p["type"] = json!(t);
            }
            if let Some(ref r) = prop.schema_ref {
                p["$ref"] = json!(r);
            }
            if let Some(ref desc) = prop.description {
                p["description"] = json!(desc);
            }
            if prop.read_only {
                p["readOnly"] = json!(true);
            }
            if let Some(ref fmt) = prop.format {
                p["format"] = json!(fmt);
            }

            // Handle items for array types
            if let Some(ref items) = prop.items {
                let mut items_json = json!({});
                if let Some(ref t) = items.prop_type {
                    items_json["type"] = json!(t);
                }
                if let Some(ref r) = items.schema_ref {
                    items_json["$ref"] = json!(r);
                }
                p["items"] = items_json;
            }

            props[name] = p;
        }
        s["properties"] = props;
    }

    s
}

/// Recursively resolves "$ref" fields in the JSON value.
fn resolve_schema_refs(
    val: &mut Value,
    doc: &RestDescription,
    seen: &mut std::collections::HashSet<String>,
) {
    match val {
        Value::Object(map) => {
            // Check if this object is a reference
            if let Some(ref_name) = map
                .get("$ref")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
            {
                // If we haven't seen this schema yet in this branch
                if !seen.contains(&ref_name) {
                    if let Some(schema) = doc.schemas.get(&ref_name) {
                        seen.insert(ref_name.clone());
                        let mut resolved = schema_to_json(schema);
                        // Recursively resolve the resolved schema
                        resolve_schema_refs(&mut resolved, doc, seen);
                        seen.remove(&ref_name);

                        // Merge resolved schema into current object, but preserve existing fields
                        // (though usually $ref stands alone)
                        if let Value::Object(resolved_map) = resolved {
                            for (k, v) in resolved_map {
                                map.entry(k).or_insert(v);
                            }
                        }
                    }
                }
            }

            // Recurse into all fields
            for (_, v) in map.iter_mut() {
                resolve_schema_refs(v, doc, seen);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                resolve_schema_refs(v, doc, seen);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_param_to_json() {
        let param = MethodParameter {
            param_type: Some("integer".to_string()),
            description: Some("desc".to_string()),
            location: Some("query".to_string()),
            required: true,
            format: Some("int32".to_string()),
            default: Some("0".to_string()),
            enum_values: Some(vec!["0".to_string(), "1".to_string()]),
            enum_descriptions: None,
            repeated: false,
            minimum: None,
            maximum: None,
            deprecated: true,
        };

        let json = param_to_json(&param);
        assert_eq!(json["type"], "integer");
        assert_eq!(json["description"], "desc");
        assert_eq!(json["location"], "query");
        assert_eq!(json["required"], true);
        assert_eq!(json["format"], "int32");
        assert_eq!(json["default"], "0");
        assert!(json["enum"].is_array());
        assert_eq!(json["deprecated"], true);
        // repeated: false should NOT appear in output
        assert!(json.get("repeated").is_none());
    }

    #[test]
    fn test_param_to_json_repeated() {
        let param = MethodParameter {
            param_type: Some("string".to_string()),
            location: Some("query".to_string()),
            repeated: true,
            ..Default::default()
        };

        let json = param_to_json(&param);
        assert_eq!(json["type"], "string");
        assert_eq!(json["repeated"], true);
    }

    #[test]
    fn test_schema_to_json_basic() {
        let mut properties = std::collections::HashMap::new();
        properties.insert(
            "name".to_string(),
            crate::discovery::JsonSchemaProperty {
                prop_type: Some("string".to_string()),
                ..Default::default()
            },
        );

        let schema = JsonSchema {
            schema_type: Some("object".to_string()),
            properties,
            ..Default::default()
        };

        let json = schema_to_json(&schema);
        assert_eq!(json["type"], "object");
        assert!(json["properties"].is_object());
        assert_eq!(json["properties"]["name"]["type"], "string");
    }

    #[test]
    fn test_resolve_schema_refs_basic() {
        let mut schemas = std::collections::HashMap::new();
        let target_schema = JsonSchema {
            schema_type: Some("string".to_string()),
            description: Some("Resolved type".to_string()),
            ..Default::default()
        };
        schemas.insert("Target".to_string(), target_schema);

        let doc = RestDescription {
            schemas,
            ..Default::default()
        };

        let mut val = json!({
            "$ref": "Target"
        });

        let mut seen = std::collections::HashSet::new();
        resolve_schema_refs(&mut val, &doc, &mut seen);

        assert_eq!(val["type"], "string");
        assert_eq!(val["description"], "Resolved type");
        // $ref might remain or effectively be merged, checking properties is key
    }

    #[test]
    fn test_resolve_schema_refs_nested() {
        let mut schemas = std::collections::HashMap::new();
        let child = JsonSchema {
            schema_type: Some("integer".to_string()),
            ..Default::default()
        };
        schemas.insert("Child".to_string(), child);

        let parent = JsonSchema {
            schema_type: Some("object".to_string()),
            properties: {
                let mut map = std::collections::HashMap::new();
                map.insert(
                    "f".to_string(),
                    crate::discovery::JsonSchemaProperty {
                        schema_ref: Some("Child".to_string()),
                        ..Default::default()
                    },
                );
                map
            },
            ..Default::default()
        };
        schemas.insert("Parent".to_string(), parent);

        let doc = RestDescription {
            schemas,
            ..Default::default()
        };

        let mut val = json!({
            "$ref": "Parent"
        });

        let mut seen = std::collections::HashSet::new();
        resolve_schema_refs(&mut val, &doc, &mut seen);

        // Check Parent resolved
        assert_eq!(val["type"], "object");
        // Check Child resolved inside Parent
        // note: schema_to_json converts ref to $ref property, then resolve_schema_refs follows it
        // The implementation matches on "$ref" keys in objects.
        // schema_to_json for Parent produces { properties: { f: { $ref: "Child" } } }
        // The recursion should resolve f.$ref to Child content.

        let f_node = &val["properties"]["f"];
        assert_eq!(f_node["type"], "integer");
    }
}
