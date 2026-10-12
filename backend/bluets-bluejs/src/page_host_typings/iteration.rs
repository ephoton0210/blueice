// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
const STREAM: &str = r#"
interface BlueIceArrayBufferView { readonly buffer: BlueIceArrayBuffer; readonly byteOffset: number; readonly byteLength: number; }
interface BlueIceStreamReadResult { readonly value: unknown; readonly done: boolean; }
interface BlueIceStreamBYOBResult { readonly value: BlueIceArrayBufferView | undefined; readonly done: boolean; }
interface BlueIceStreamStrategy { highWaterMark?: number; size?: (chunk: unknown) => number; }
interface BlueIceDefaultSource { start?: (controller: BlueIceReadableStreamDefaultController) => unknown; pull?: (controller: BlueIceReadableStreamDefaultController) => unknown; cancel?: (reason: unknown) => unknown; }
interface BlueIceByteSource { type: 'bytes'; autoAllocateChunkSize?: number; start?: (controller: BlueIceReadableByteStreamController) => unknown; pull?: (controller: BlueIceReadableByteStreamController) => unknown; cancel?: (reason: unknown) => unknown; }
interface BlueIceReadableStream extends BlueIceReadableStreamTee, BlueIceReadableStreamIterable { readonly locked: boolean; getReader(options: {mode: 'byob'}): BlueIceReadableStreamBYOBReader; getReader(options?: {mode?: undefined}): BlueIceReadableStreamDefaultReader; cancel(reason?: unknown): BlueIceFilePromise<void>; }
declare const ReadableStream: { new(source?: BlueIceDefaultSource | BlueIceByteSource, strategy?: BlueIceStreamStrategy): BlueIceReadableStream; readonly prototype: BlueIceReadableStream; };
"#;
pub(super) fn bindings() -> [PageHostDocumentBindingV1; 40] {
    let old = tee::bindings();
    let mut bindings = std::array::from_fn(|index| {
        if index < old.len() {
            old[index]
        } else {
            PageHostDocumentBindingV1{stable_id:"stream.live-iteration",declaration:"interface BlueIceStreamAsyncIterator { next(value?: unknown): BlueIceFilePromise<BlueIceStreamReadResult>; return(value?: unknown): BlueIceFilePromise<BlueIceStreamReadResult>; }\ninterface BlueIceReadableStreamIterable { values(options?: {preventCancel?: boolean}): BlueIceStreamAsyncIterator; }",role:PageHostBindingRoleV1::Type,runtime_binding_id:"stream.values",capability:"dom-read",feature_flag:"live-dom-stream-iteration",first_host_api_version:"blueice-core-script-v15"}
        }
    });
    bindings[27].declaration = STREAM;
    bindings[27].first_host_api_version = "blueice-core-script-v15";
    bindings
}
