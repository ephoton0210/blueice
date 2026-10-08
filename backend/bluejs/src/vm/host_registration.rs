// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checked native registration capacity, independent of allocation/publication.

use super::RuntimeError;

/// The offset of the last tag reserved by a registration (zero for one tag,
/// one for an accessor pair). Validate the entire reservation before installing
/// any heap wrapper or publishing a callback.
pub(super) fn host_registration_index(
    index: usize,
    last_offset: u32,
    message: &'static str,
) -> Result<u32, RuntimeError> {
    let refusal = || RuntimeError::RangeError(message.into());
    let index = u32::try_from(index).map_err(|_| refusal())?;
    index.checked_add(last_offset).ok_or_else(refusal)?;
    Ok(index)
}

pub(super) fn host_function_index(index: usize) -> Result<u32, RuntimeError> {
    host_registration_index(index, 0, "too many host functions")
}
