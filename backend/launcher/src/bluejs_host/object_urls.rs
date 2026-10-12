// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) struct CoreObjectUrls {
    pub(super) client: Rc<RefCell<ScriptDomClient>>,
    pub(super) target: ScriptDocumentTarget,
}
impl blueice_bluejs::HostObjectUrls for CoreObjectUrls {
    fn create(&mut self, bytes: &[u8], media_type: &str) -> Result<String, HostFunctionError> {
        self.client
            .borrow_mut()
            .create_blob_url(self.target, bytes, media_type)
            .map_err(|_| HostFunctionError::new("Object URL creation unavailable"))
    }
    fn revoke(&mut self, url: &str) -> Result<(), HostFunctionError> {
        self.client
            .borrow_mut()
            .revoke_blob_url(self.target, url)
            .map_err(|_| HostFunctionError::new("Object URL revocation unavailable"))
    }
}
impl ScriptDomClient {
    fn create_blob_url(
        &mut self,
        target: ScriptDocumentTarget,
        bytes: &[u8],
        media_type: &str,
    ) -> io::Result<String> {
        if media_type.len() > SCRIPT_MAX_NAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Blob MIME type too long",
            ));
        }
        let ScriptReply::BlobUrlUpload { upload_id } = self.call(
            target,
            ScriptRequest::BeginBlobUrl {
                target,
                size: bytes.len() as u64,
                media_type: media_type.into(),
            },
        )?
        else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Blob upload denied",
            ));
        };
        let result = (|| {
            for (index, chunk) in bytes
                .chunks(script::SCRIPT_MAX_FILE_CHUNK_BYTES)
                .enumerate()
            {
                match self.call(
                    target,
                    ScriptRequest::AppendBlobUrl {
                        target,
                        upload_id,
                        offset: (index * script::SCRIPT_MAX_FILE_CHUNK_BYTES) as u64,
                        bytes: chunk.into(),
                    },
                )? {
                    ScriptReply::Ack => (),
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "Blob upload denied",
                        ))
                    }
                }
            }
            match self.call(target, ScriptRequest::CommitBlobUrl { target, upload_id })? {
                ScriptReply::BlobUrl { url }
                    if url.len() <= SCRIPT_MAX_NAME_BYTES && url.starts_with("blob:") =>
                {
                    Ok(url)
                }
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Blob URL unavailable",
                )),
            }
        })();
        if result.is_err() {
            let _ = self.call(target, ScriptRequest::AbortBlobUrl { target, upload_id });
        }
        result
    }
    fn revoke_blob_url(&mut self, target: ScriptDocumentTarget, url: &str) -> io::Result<()> {
        if url.len() > SCRIPT_MAX_NAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Blob URL too long",
            ));
        }
        match self.call(
            target,
            ScriptRequest::RevokeBlobUrl {
                target,
                url: url.into(),
            },
        )? {
            ScriptReply::Ack => Ok(()),
            _ => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Blob URL revocation unavailable",
            )),
        }
    }
}
