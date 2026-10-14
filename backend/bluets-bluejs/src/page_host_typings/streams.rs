// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Installed readable-stream APIs; prior generated profiles remain immutable.
use super::*;
const BLOB: &str = r#"
interface BlueIceFilePromise<T> { then(onfulfilled: (value: T) => unknown): BlueIceFilePromise<unknown>; catch(onrejected: (reason: unknown) => unknown): BlueIceFilePromise<unknown>; finally(onfinally: () => void): BlueIceFilePromise<T>; }
interface BlueIceBlob { readonly size: number; readonly type: string; slice(start?: number, end?: number, contentType?: string): BlueIceBlob; text(): BlueIceFilePromise<string>; arrayBuffer(): BlueIceFilePromise<unknown>; bytes(): BlueIceFilePromise<unknown>; stream(): BlueIceReadableStream; textStream(): BlueIceReadableStream; }
declare const Blob: { new(parts?: unknown[], options?: { type?: string; endings?: 'transparent' | 'native' }): BlueIceBlob; readonly prototype: BlueIceBlob; };
"#;
const STREAM: &str = r#"
interface BlueIceArrayBufferView { readonly buffer: BlueIceArrayBuffer; readonly byteOffset: number; readonly byteLength: number; }
interface BlueIceStreamReadResult { readonly value: unknown; readonly done: boolean; }
interface BlueIceStreamBYOBResult { readonly value: BlueIceArrayBufferView | undefined; readonly done: boolean; }
interface BlueIceStreamStrategy { highWaterMark?: number; size?: (chunk: unknown) => number; }
interface BlueIceDefaultSource { start?: (controller: BlueIceReadableStreamDefaultController) => unknown; pull?: (controller: BlueIceReadableStreamDefaultController) => unknown; cancel?: (reason: unknown) => unknown; }
interface BlueIceByteSource { type: 'bytes'; autoAllocateChunkSize?: number; start?: (controller: BlueIceReadableByteStreamController) => unknown; pull?: (controller: BlueIceReadableByteStreamController) => unknown; cancel?: (reason: unknown) => unknown; }
interface BlueIceReadableStream { readonly locked: boolean; getReader(options: {mode: 'byob'}): BlueIceReadableStreamBYOBReader; getReader(options?: {mode?: undefined}): BlueIceReadableStreamDefaultReader; cancel(reason?: unknown): BlueIceFilePromise<void>; }
declare const ReadableStream: { new(source?: BlueIceDefaultSource | BlueIceByteSource, strategy?: BlueIceStreamStrategy): BlueIceReadableStream; readonly prototype: BlueIceReadableStream; };
"#;
const DEFAULT_READER: &str = r#"
interface BlueIceReadableStreamDefaultReader { readonly closed: BlueIceFilePromise<void>; read(): BlueIceFilePromise<BlueIceStreamReadResult>; cancel(reason?: unknown): BlueIceFilePromise<void>; releaseLock(): void; }
declare const ReadableStreamDefaultReader: { new(stream: BlueIceReadableStream): BlueIceReadableStreamDefaultReader; readonly prototype: BlueIceReadableStreamDefaultReader; };
"#;
const BYOB_READER: &str = r#"
interface BlueIceReadableStreamBYOBReader { readonly closed: BlueIceFilePromise<void>; read(view: BlueIceArrayBufferView, options?: {min?: number}): BlueIceFilePromise<BlueIceStreamBYOBResult>; cancel(reason?: unknown): BlueIceFilePromise<void>; releaseLock(): void; }
declare const ReadableStreamBYOBReader: { new(stream: BlueIceReadableStream): BlueIceReadableStreamBYOBReader; readonly prototype: BlueIceReadableStreamBYOBReader; };
"#;
const DEFAULT_CONTROLLER: &str = r#"
interface BlueIceReadableStreamDefaultController { readonly desiredSize: number | null; enqueue(chunk?: unknown): void; close(): void; error(reason?: unknown): void; }
declare const ReadableStreamDefaultController: { readonly prototype: BlueIceReadableStreamDefaultController; };
"#;
const BYTE_CONTROLLER: &str = r#"
interface BlueIceReadableByteStreamController { readonly desiredSize: number | null; readonly byobRequest: BlueIceReadableStreamBYOBRequest | null; enqueue(chunk: BlueIceArrayBufferView): void; close(): void; error(reason?: unknown): void; }
declare const ReadableByteStreamController: { readonly prototype: BlueIceReadableByteStreamController; };
"#;
const REQUEST: &str = r#"
interface BlueIceReadableStreamBYOBRequest { readonly view: BlueIceArrayBufferView | null; respond(bytesWritten: number): void; respondWithNewView(view: BlueIceArrayBufferView): void; }
declare const ReadableStreamBYOBRequest: { readonly prototype: BlueIceReadableStreamBYOBRequest; };
"#;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 33] {
    let old = file_reader::bindings();
    let added = [
        ("ReadableStream", STREAM),
        ("ReadableStreamDefaultReader", DEFAULT_READER),
        ("ReadableStreamBYOBReader", BYOB_READER),
        ("ReadableStreamDefaultController", DEFAULT_CONTROLLER),
        ("ReadableByteStreamController", BYTE_CONTROLLER),
        ("ReadableStreamBYOBRequest", REQUEST),
    ];
    let ids = [
        ("stream.global-ReadableStream", "global.ReadableStream"),
        (
            "stream.global-ReadableStreamDefaultReader",
            "global.ReadableStreamDefaultReader",
        ),
        (
            "stream.global-ReadableStreamBYOBReader",
            "global.ReadableStreamBYOBReader",
        ),
        (
            "stream.global-ReadableStreamDefaultController",
            "global.ReadableStreamDefaultController",
        ),
        (
            "stream.global-ReadableByteStreamController",
            "global.ReadableByteStreamController",
        ),
        (
            "stream.global-ReadableStreamBYOBRequest",
            "global.ReadableStreamBYOBRequest",
        ),
    ];
    let mut bindings = std::array::from_fn(|index| {
        if index < 27 {
            old[index]
        } else {
            let (_, declaration) = added[index - 27];
            let (stable_id, runtime_binding_id) = ids[index - 27];
            PageHostDocumentBindingV1 {
                stable_id,
                declaration,
                role: PageHostBindingRoleV1::Value,
                runtime_binding_id,
                capability: "selected-file-read",
                feature_flag: "live-dom-streams",
                first_host_api_version: "blueice-core-script-v9",
            }
        }
    });
    bindings[10].declaration = BLOB;
    bindings[10].first_host_api_version = "blueice-core-script-v9";
    bindings
}
