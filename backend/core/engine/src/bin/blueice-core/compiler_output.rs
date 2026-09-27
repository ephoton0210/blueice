// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independent, owner-only output listener. It owns framing and per-stream
//! receipts; compilation and publication stay on the core session thread.

use super::*;
use blueice_ipc::compiler_output::{
    self, CompilerOutputRequest, CompilerOutputSessionReceipt, COMPILER_OUTPUT_PROTOCOL_VERSION,
};

fn mint_receipt() -> io::Result<CompilerOutputSessionReceipt> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut id = String::from("ow-");
    for byte in bytes {
        use std::fmt::Write;
        write!(id, "{byte:02x}").expect("writing to a String cannot fail");
    }
    let receipt = CompilerOutputSessionReceipt { id };
    debug_assert!(receipt.is_well_formed());
    Ok(receipt)
}

fn serve_connection(
    mut stream: UnixStream,
    sender: CompilerServiceIpcRequestSender,
) -> io::Result<()> {
    let first = compiler_output::read_compiler_output_request(&mut stream)?;
    let accepted = matches!(
        first,
        CompilerOutputRequest::Hello {
            protocol_version: COMPILER_OUTPUT_PROTOCOL_VERSION
        }
    );
    let receipt = accepted.then(mint_receipt).transpose()?;
    let bound = receipt
        .as_ref()
        .map(|receipt| sender.bind_output_session(receipt.clone()))
        .transpose()?;
    let reply = compiler_output::negotiate(&first, receipt);
    compiler_output::write_compiler_output_reply(&mut stream, &reply)?;
    let Some(bound) = bound else {
        return Ok(());
    };

    loop {
        let request = match compiler_output::read_compiler_output_request(&mut stream) {
            Ok(request) => request,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        };
        let reply = bound.request(request)?;
        compiler_output::write_compiler_output_reply(&mut stream, &reply)?;
    }
}

pub(super) fn serve_compiler_output_listener(
    listener: UnixListener,
    sender: CompilerServiceIpcRequestSender,
) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            break;
        };
        let _ = serve_connection(stream, sender.clone());
    }
}
