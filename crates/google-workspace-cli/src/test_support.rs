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

//! Shared test-only helpers.

/// Saves an env var's current value on construction and restores it (or unsets
/// it) on drop — including on panic. Pair with `#[serial_test::serial]`, since
/// the environment is process-global.
pub(crate) struct EnvVarGuard {
    name: String,
    original: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    /// Save the current value of `name`, then set it to `value`.
    pub(crate) fn set(name: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(name);
        std::env::set_var(name, value);
        Self {
            name: name.to_string(),
            original,
        }
    }

    /// Save the current value of `name`, then remove it.
    pub(crate) fn remove(name: &str) -> Self {
        let original = std::env::var_os(name);
        std::env::remove_var(name);
        Self {
            name: name.to_string(),
            original,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(v) => std::env::set_var(&self.name, v),
            None => std::env::remove_var(&self.name),
        }
    }
}
