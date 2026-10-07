// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! macOS download metadata, installed on the owned descriptor before publish.
//!
//! CoreFoundation's public quarantine API requires a pathname and rejects
//! `/dev/fd`. Let it generate metadata on a private temporary file, then copy
//! the system-generated attribute to the download fd. Never reopen an untrusted
//! download path or synthesize the undocumented quarantine attribute format.

use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::propertylist::{create_data, kCFPropertyListBinaryFormat_v1_0};
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::url::{CFURLSetResourcePropertyForKey, CFURL};
use std::ffi::CStr;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    static kLSQuarantineAgentNameKey: CFStringRef;
    static kLSQuarantineAgentBundleIdentifierKey: CFStringRef;
    static kLSQuarantineTypeKey: CFStringRef;
    static kLSQuarantineTypeWebDownload: CFStringRef;
    static kLSQuarantineTypeOtherDownload: CFStringRef;
    static kLSQuarantineDataURLKey: CFStringRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFURLQuarantinePropertiesKey: CFStringRef;
}

struct Scratch {
    directory: PathBuf,
    file: File,
}

impl Scratch {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "blueice-quarantine-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&directory)?;
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join("metadata"))
        {
            Ok(file) => Ok(Self { directory, file }),
            Err(error) => {
                let _ = std::fs::remove_dir(&directory);
                Err(error)
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.directory.join("metadata"));
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn read_attribute(file: &File, name: &CStr) -> io::Result<Vec<u8>> {
    // Attribute sizes are bounded; the generated quarantine value is normally
    // a few dozen bytes. Read from the retained fd, including after copying.
    let mut value = vec![0; 8192];
    let size = unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if size < 0 {
        return Err(io::Error::last_os_error());
    }
    if size == 0 {
        return Err(io::Error::other("empty macOS download metadata"));
    }
    value.truncate(size as usize);
    Ok(value)
}

fn write_attribute(file: &File, name: &CStr, value: &[u8]) -> io::Result<()> {
    if unsafe {
        libc::fsetxattr(
            file.as_raw_fd(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if read_attribute(file, name)? != value {
        return Err(io::Error::other("macOS did not retain download metadata"));
    }
    Ok(())
}

pub(crate) fn apply(file: &File, source: &str) -> io::Result<()> {
    apply_metadata(file, source)
        .map_err(|error| io::Error::other(format!("macOS download quarantine failed: {error}")))
}

fn apply_metadata(file: &File, source: &str) -> io::Result<()> {
    let mut address = url::Url::parse(source).map_err(io::Error::other)?;
    address
        .set_username("")
        .map_err(|()| io::Error::other("invalid download source"))?;
    address
        .set_password(None)
        .map_err(|()| io::Error::other("invalid download source"))?;
    // Do not persist credentials or query/fragment tokens in Finder metadata.
    address.set_query(None);
    address.set_fragment(None);
    let source = CFString::new(address.as_str());
    let scratch = Scratch::new()?;
    let target = CFURL::from_path(scratch.directory.join("metadata"), false)
        .ok_or_else(|| io::Error::other("invalid metadata file URL"))?;
    // The framework owns these constant CFStrings. Get-rule wrappers retain
    // them; the dictionary then retains all of its keys and values.
    let properties = unsafe {
        let key = |value| CFString::wrap_under_get_rule(value);
        let kind = if matches!(address.scheme(), "http" | "https") {
            kLSQuarantineTypeWebDownload
        } else {
            kLSQuarantineTypeOtherDownload
        };
        CFDictionary::from_CFType_pairs(&[
            (
                key(kLSQuarantineAgentNameKey),
                CFString::new("BlueIce").as_CFType(),
            ),
            (
                key(kLSQuarantineAgentBundleIdentifierKey),
                CFString::new("cc.blueice.BlueIce").as_CFType(),
            ),
            (key(kLSQuarantineTypeKey), key(kind).as_CFType()),
            // Apple documents this value as a CFURL, rather than a string.
            (
                key(kLSQuarantineDataURLKey),
                source_url(&source)?.as_CFType(),
            ),
        ])
    };
    if unsafe {
        CFURLSetResourcePropertyForKey(
            target.as_concrete_TypeRef(),
            kCFURLQuarantinePropertiesKey,
            properties.as_CFTypeRef(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::other("macOS refused quarantine metadata"));
    }
    let quarantine = read_attribute(&scratch.file, c"com.apple.quarantine")?;
    let origins = CFArray::from_CFTypes(&[source]);
    let origins = create_data(origins.as_CFTypeRef(), kCFPropertyListBinaryFormat_v1_0)
        .map_err(|error| io::Error::other(format!("cannot encode download source: {error:?}")))?;
    write_attribute(
        file,
        c"com.apple.metadata:kMDItemWhereFroms",
        origins.bytes(),
    )?;
    write_attribute(file, c"com.apple.quarantine", &quarantine)
}

fn source_url(source: &CFString) -> io::Result<CFURL> {
    use core_foundation::url::CFURLCreateWithString;
    let value = unsafe {
        CFURLCreateWithString(
            std::ptr::null_mut(),
            source.as_concrete_TypeRef(),
            std::ptr::null(),
        )
    };
    if value.is_null() {
        return Err(io::Error::other("invalid download source URL"));
    }
    Ok(unsafe { CFURL::wrap_under_create_rule(value) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::data::CFData;
    use core_foundation::propertylist::create_with_data;

    #[test]
    fn quarantine_uses_the_owned_file_and_redacts_source_credentials_and_tokens() {
        let scratch = Scratch::new().unwrap();
        apply(
            &scratch.file,
            "https://alice:secret@example.test/notes.txt?token=private#private",
        )
        .unwrap();
        assert!(!read_attribute(&scratch.file, c"com.apple.quarantine")
            .unwrap()
            .is_empty());
        let origins =
            read_attribute(&scratch.file, c"com.apple.metadata:kMDItemWhereFroms").unwrap();
        let (origins, _) = create_with_data(CFData::from_buffer(&origins), 0).unwrap();
        let origins = unsafe { CFArray::<CFString>::wrap_under_create_rule(origins.cast()) };
        assert_eq!(origins.len(), 1);
        assert_eq!(
            origins.get(0).unwrap().to_string(),
            "https://example.test/notes.txt"
        );
    }

    #[test]
    fn a_file_without_extended_attribute_support_refuses_quarantine() {
        let device = File::open("/dev/null").unwrap();
        let error = apply(&device, "https://example.test/notes.txt").unwrap_err();
        assert!(error
            .to_string()
            .contains("macOS download quarantine failed"));
    }
}
