// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Full immutable object families, observed through the public compiler twice.
#![allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[path = "oracle_support/ge5_object_roles.rs"]
mod object_roles;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;
