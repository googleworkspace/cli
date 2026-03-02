#!/bin/bash
# Copyright 2026 Google LLC
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

set -e

# Check if cargo-llvm-cov is installed
if ! cargo llvm-cov --version &> /dev/null; then
  echo "cargo-llvm-cov is not installed. Installing..."
  cargo install cargo-llvm-cov
fi

# Run coverage and generate HTML report
echo "Running tests with coverage..."
cargo llvm-cov --all-features --workspace --html
cargo llvm-cov --all-features --workspace # Print text summary

echo "Coverage report generated at target/llvm-cov/html/index.html"

# Open the report if on macOS
if [[ "$OSTYPE" == "darwin"* ]]; then
  open target/llvm-cov/html/index.html
fi
