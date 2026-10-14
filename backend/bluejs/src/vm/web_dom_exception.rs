// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Branded browser exceptions, including Error inheritance and legacy codes.
//! Behavioral reference: https://webidl.spec.whatwg.org/#idl-DOMException

use super::*;

const LEGACY_EXCEPTIONS: &[(&str, &str)] = &[
    ("IndexSizeError", "INDEX_SIZE_ERR"),
    ("", "DOMSTRING_SIZE_ERR"),
    ("HierarchyRequestError", "HIERARCHY_REQUEST_ERR"),
    ("WrongDocumentError", "WRONG_DOCUMENT_ERR"),
    ("InvalidCharacterError", "INVALID_CHARACTER_ERR"),
    ("", "NO_DATA_ALLOWED_ERR"),
    ("NoModificationAllowedError", "NO_MODIFICATION_ALLOWED_ERR"),
    ("NotFoundError", "NOT_FOUND_ERR"),
    ("NotSupportedError", "NOT_SUPPORTED_ERR"),
    ("InUseAttributeError", "INUSE_ATTRIBUTE_ERR"),
    ("InvalidStateError", "INVALID_STATE_ERR"),
    ("SyntaxError", "SYNTAX_ERR"),
    ("InvalidModificationError", "INVALID_MODIFICATION_ERR"),
    ("NamespaceError", "NAMESPACE_ERR"),
    ("InvalidAccessError", "INVALID_ACCESS_ERR"),
    ("", "VALIDATION_ERR"),
    ("TypeMismatchError", "TYPE_MISMATCH_ERR"),
    ("SecurityError", "SECURITY_ERR"),
    ("NetworkError", "NETWORK_ERR"),
    ("AbortError", "ABORT_ERR"),
    ("URLMismatchError", "URL_MISMATCH_ERR"),
    ("QuotaExceededError", "QUOTA_EXCEEDED_ERR"),
    ("TimeoutError", "TIMEOUT_ERR"),
    ("InvalidNodeTypeError", "INVALID_NODE_TYPE_ERR"),
    ("DataCloneError", "DATA_CLONE_ERR"),
];

impl Vm {
    pub(super) fn install_web_dom_exception(&mut self) -> Result<(), RuntimeError> {
        if self.globals.contains_key("%WebDOMException%") {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.heap.get_own(global, "DOMException")?.is_some() {
            return Err(RuntimeError::TypeError(
                "DOMException global already defined".into(),
            ));
        }
        let error = self.error_global("Error")?.object_id().unwrap();
        let parent = self.heap.get(error, "prototype")?.object_id().unwrap();
        let base = self.stack.len();
        let result = (|| {
            // Web IDL also gives the interface prototype [[ErrorData]].
            let prototype = self.with_roots(|heap| heap.alloc_error(Some(parent)))?;
            self.stack.push(Value::Object(prototype));
            let function = self.function_prototype()?;
            for (name, native) in [
                ("name", NativeFunction::DOMExceptionName),
                ("message", NativeFunction::DOMExceptionMessage),
                ("code", NativeFunction::DOMExceptionCode),
            ] {
                self.install_native_getter(prototype, function, name, native)?;
                self.with_roots(|heap| {
                    heap.define_own_property(
                        prototype,
                        name,
                        PropertyDescriptor {
                            enumerable: Some(true),
                            ..Default::default()
                        },
                    )
                })?;
            }
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("DOMException".into()),
                false,
                false,
                true,
            )?;
            let constructor = self.web_file_constructor(
                "DOMException",
                0,
                NativeFunction::DOMException,
                function,
                prototype,
            )?;
            self.stack.push(Value::Object(constructor));
            for (index, (_, name)) in LEGACY_EXCEPTIONS.iter().enumerate() {
                for owner in [prototype, constructor] {
                    self.define_data(
                        owner,
                        *name,
                        Value::Number((index + 1) as f64),
                        false,
                        true,
                        false,
                    )?;
                }
            }
            self.define_data(
                global,
                "DOMException",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.heap.root(prototype)?;
            self.globals.insert("%WebDOMException%".into(), prototype);
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    pub(super) fn web_dom_exception_call(
        &mut self,
        native: NativeFunction,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if native == NativeFunction::DOMException {
            if !construct {
                return Err(RuntimeError::TypeError("DOMException requires new".into()));
            }
            let message = if args.first().is_none_or(|value| *value == Value::Undefined) {
                JsString::default()
            } else {
                self.coerce_string(&args[0])?
            };
            let name = if args.get(1).is_none_or(|value| *value == Value::Undefined) {
                "Error".into()
            } else {
                self.coerce_string(&args[1])?
            };
            self.check_string(&Value::String(name.clone()))?;
            self.check_string(&Value::String(message.clone()))?;
            let default = self.globals["%WebDOMException%"];
            let prototype = self.constructor_prototype_for(default, None)?;
            return Ok(Value::Object(self.with_roots(|heap| {
                heap.alloc_dom_exception(name, message, prototype)
            })?));
        }
        let object = receiver
            .object_id()
            .ok_or_else(|| RuntimeError::TypeError("Invalid DOMException receiver".into()))?;
        let (name, message) = self
            .heap
            .dom_exception(object)
            .map_err(|_| RuntimeError::TypeError("Invalid DOMException receiver".into()))?;
        Ok(match native {
            NativeFunction::DOMExceptionName => Value::String(name.clone()),
            NativeFunction::DOMExceptionMessage => Value::String(message.clone()),
            _ => Value::Number(
                LEGACY_EXCEPTIONS
                    .iter()
                    .position(|(candidate, _)| !candidate.is_empty() && *name == *candidate)
                    .map_or(0.0, |index| (index + 1) as f64),
            ),
        })
    }
}
