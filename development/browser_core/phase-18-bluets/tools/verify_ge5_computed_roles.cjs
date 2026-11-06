// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Preserve the entire frozen ES5/publication selection for computed assignment.
process.argv.push('--computed');
require('./verify_ge5_linear_roles.cjs');
