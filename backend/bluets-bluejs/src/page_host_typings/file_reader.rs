// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Immutable extension of the drag-image profile for installed reader APIs.

use super::*;

const EVENT: &str = r#"
interface BlueIceEventInit { bubbles?: boolean; cancelable?: boolean; composed?: boolean; }
interface BlueIceUIEventInit extends BlueIceEventInit { view?: null; detail?: number; }
interface BlueIceMouseEventInit extends BlueIceUIEventInit { ctrlKey?: boolean; shiftKey?: boolean; altKey?: boolean; metaKey?: boolean; modifierAltGraph?: boolean; modifierCapsLock?: boolean; modifierFn?: boolean; modifierFnLock?: boolean; modifierHyper?: boolean; modifierNumLock?: boolean; modifierScrollLock?: boolean; modifierSuper?: boolean; modifierSymbol?: boolean; modifierSymbolLock?: boolean; screenX?: number; screenY?: number; clientX?: number; clientY?: number; button?: number; buttons?: number; relatedTarget?: BlueIceNode | null; }
interface BlueIceDragEventInit extends BlueIceMouseEventInit { dataTransfer?: BlueIceDataTransfer | null; }
type BlueIceEventPathTarget = BlueIceNode | BlueIceEventTarget;
interface BlueIceEvent { readonly type: string; readonly target: BlueIceNode | BlueIceEventTarget | null; readonly srcElement: BlueIceNode | BlueIceEventTarget | null; readonly currentTarget: BlueIceNode | BlueIceEventTarget | null; readonly eventPhase: number; readonly bubbles: boolean; readonly cancelable: boolean; readonly composed: boolean; readonly defaultPrevented: boolean; readonly isTrusted: boolean; readonly timeStamp: number; cancelBubble: boolean; returnValue: boolean; preventDefault(): void; stopPropagation(): void; stopImmediatePropagation(): void; composedPath(): BlueIceEventPathTarget[]; initEvent(eventType: string, bubbles?: boolean, cancelable?: boolean): void; }
declare const Event: { new(eventType: string, init?: BlueIceEventInit): BlueIceEvent; readonly prototype: BlueIceEvent; readonly NONE: number; readonly CAPTURING_PHASE: number; readonly AT_TARGET: number; readonly BUBBLING_PHASE: number; };
"#;

const TARGET: &str = r#"
type BlueIceEventCallback = (event: BlueIceEvent) => void;
interface BlueIceEventListener { handleEvent(event: BlueIceEvent): void; }
interface BlueIceEventTarget {
    addEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragListenerOptions): void;
    removeEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;
    dispatchEvent(event: BlueIceEvent): boolean;
}
declare const EventTarget: { new(): BlueIceEventTarget; readonly prototype: BlueIceEventTarget; };
"#;

const PROGRESS: &str = r#"
interface BlueIceProgressEventInit extends BlueIceEventInit { lengthComputable?: boolean; loaded?: number; total?: number; }
interface BlueIceProgressEvent extends BlueIceEvent { readonly lengthComputable: boolean; readonly loaded: number; readonly total: number; }
declare const ProgressEvent: { new(eventType: string, init?: BlueIceProgressEventInit): BlueIceProgressEvent; readonly prototype: BlueIceProgressEvent; };
"#;

const READER: &str = r#"
interface BlueIceArrayBuffer { readonly byteLength: number; slice(begin: number, end?: number): BlueIceArrayBuffer; }
type BlueIceReaderCallback = (event: BlueIceProgressEvent) => void;
interface BlueIceReaderEventListener { handleEvent(event: BlueIceProgressEvent): void; }
interface BlueIceFileReader extends BlueIceEventTarget {
    readonly readyState: number;
    readonly result: string | BlueIceArrayBuffer | null;
    readonly error: BlueIceDOMException | null;
    readonly EMPTY: number; readonly LOADING: number; readonly DONE: number;
    onloadstart: BlueIceReaderCallback | null;
    onprogress: BlueIceReaderCallback | null;
    onabort: BlueIceReaderCallback | null;
    onerror: BlueIceReaderCallback | null;
    onload: BlueIceReaderCallback | null;
    onloadend: BlueIceReaderCallback | null;
    readAsText(blob: BlueIceBlob, encoding?: string): void;
    readAsArrayBuffer(blob: BlueIceBlob): void;
    readAsBinaryString(blob: BlueIceBlob): void;
    readAsDataURL(blob: BlueIceBlob): void;
    abort(): void;
    addEventListener(eventType: 'loadstart' | 'progress' | 'abort' | 'error' | 'load' | 'loadend', listener: BlueIceReaderCallback | BlueIceReaderEventListener | null, options?: boolean | BlueIceDragListenerOptions): void;
    addEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragListenerOptions): void;
    removeEventListener(eventType: 'loadstart' | 'progress' | 'abort' | 'error' | 'load' | 'loadend', listener: BlueIceReaderCallback | BlueIceReaderEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;
    removeEventListener(eventType: string, listener: BlueIceEventCallback | BlueIceEventListener | null, options?: boolean | BlueIceDragRemovalOptions): void;
}
declare const FileReader: { new(): BlueIceFileReader; readonly prototype: BlueIceFileReader; readonly EMPTY: number; readonly LOADING: number; readonly DONE: number; };
"#;

pub(super) fn bindings() -> [PageHostDocumentBindingV1; 27] {
    let added = [
        (
            "event.global-EventTarget",
            "global.EventTarget",
            TARGET,
            "dom-read",
        ),
        (
            "event.global-ProgressEvent",
            "global.ProgressEvent",
            PROGRESS,
            "dom-read",
        ),
        (
            "file.global-FileReader",
            "global.FileReader",
            READER,
            "selected-file-read",
        ),
    ];
    let mut bindings = std::array::from_fn(|index| {
        if index < 24 {
            PAGE_HOST_DOM_DRAG_IMAGE_BINDINGS_V2[index]
        } else {
            let (stable_id, runtime_binding_id, declaration, capability) = added[index - 24];
            PageHostDocumentBindingV1 {
                stable_id,
                declaration,
                role: PageHostBindingRoleV1::Value,
                runtime_binding_id,
                capability,
                feature_flag: "live-dom-file-reader",
                first_host_api_version: "blueice-core-script-v8",
            }
        }
    });
    bindings[16].declaration = EVENT;
    bindings[16].first_host_api_version = "blueice-core-script-v8";
    bindings
}
