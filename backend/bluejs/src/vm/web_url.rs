// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-installed URL objects and branded Blob export callbacks.
//! URL parsing/serialization: https://url.spec.whatwg.org/
//! Blob URL lifetime/storage are owned by Core, not by this JS realm.
use super::*;
use crate::native::UrlNative;
use url::{quirks, Url};

pub trait HostObjectUrls: 'static {
    fn create(&mut self, bytes: &[u8], mime_type: &str) -> Result<String, HostFunctionError>;
    fn revoke(&mut self, url: &str) -> Result<(), HostFunctionError>;
}

const PARTS: [&str; 11] = [
    "href", "origin", "protocol", "username", "password", "host", "hostname", "port", "pathname",
    "search", "hash",
];

impl Vm {
    pub fn install_host_object_urls(
        &mut self,
        host: impl HostObjectUrls,
    ) -> Result<(), RuntimeError> {
        if self.host_object_urls.is_some() {
            return Err(RuntimeError::TypeError(
                "Object URL owner already installed".into(),
            ));
        }
        self.install_web_file_api()?;
        self.install_web_url_api()?;
        self.host_object_urls = Some(Box::new(host));
        Ok(())
    }

    pub fn install_web_url_api(&mut self) -> Result<(), RuntimeError> {
        if self.globals.contains_key("%WebURL%") {
            return Ok(());
        }
        let global = self.global("globalThis")?.object_id().unwrap();
        if self.heap.get_own(global, "URL")?.is_some() {
            return Err(RuntimeError::TypeError("URL global already defined".into()));
        }
        let base = self.stack.len();
        let result = (|| {
            let function = self.function_prototype()?;
            let object = self.object_prototype;
            let prototype = self.with_roots(|heap| heap.alloc_object(Some(object)))?;
            self.stack.push(Value::Object(prototype));
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String("URL".into()),
                false,
                false,
                true,
            )?;
            for (index, name) in PARTS.iter().enumerate() {
                let getter = NativeFunction::WebUrl(UrlNative::Part(index as u8, false));
                if index == 1 {
                    self.install_native_getter(prototype, function, name, getter)?;
                } else {
                    self.install_native_accessor(
                        prototype,
                        function,
                        name,
                        getter,
                        NativeFunction::WebUrl(UrlNative::Part(index as u8, true)),
                    )?;
                }
            }
            for name in ["toString", "toJSON"] {
                self.install_native(
                    prototype,
                    function,
                    name,
                    0,
                    NativeFunction::WebUrl(UrlNative::Serialize),
                )?;
            }
            let constructor = self.web_file_constructor(
                "URL",
                1,
                NativeFunction::WebUrl(UrlNative::Constructor),
                function,
                prototype,
            )?;
            self.stack.push(Value::Object(constructor));
            for (name, native) in [
                ("canParse", UrlNative::Parse(true)),
                ("parse", UrlNative::Parse(false)),
                ("createObjectURL", UrlNative::Create),
                ("revokeObjectURL", UrlNative::Revoke),
            ] {
                self.install_native(
                    constructor,
                    function,
                    name,
                    1,
                    NativeFunction::WebUrl(native),
                )?;
            }
            for name in PARTS.into_iter().chain(["toString", "toJSON"]) {
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
            for name in ["canParse", "parse", "createObjectURL", "revokeObjectURL"] {
                self.with_roots(|heap| {
                    heap.define_own_property(
                        constructor,
                        name,
                        PropertyDescriptor {
                            enumerable: Some(true),
                            ..Default::default()
                        },
                    )
                })?;
            }
            self.define_data(global, "URL", Value::Object(constructor), true, false, true)?;
            self.globals.insert("%WebURL%".into(), prototype);
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    fn url_parse_arguments(&mut self, args: &[Value]) -> Result<Result<Url, ()>, RuntimeError> {
        if args.is_empty() {
            return Err(RuntimeError::TypeError("URL requires an input".into()));
        }
        let input = self.coerce_string(&args[0])?;
        let input = String::from_utf16_lossy(input.as_code_units());
        let base = if !matches!(native::argument(args, 1), Value::Undefined) {
            let base = self.coerce_string(&args[1])?;
            match Url::parse(&String::from_utf16_lossy(base.as_code_units())) {
                Ok(base) => Some(base),
                Err(_) => return Ok(Err(())),
            }
        } else {
            None
        };
        Ok(Url::options()
            .base_url(base.as_ref())
            .parse(&input)
            .map_err(|_| ()))
    }

    pub(super) fn web_url_call(
        &mut self,
        native: UrlNative,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        use UrlNative::*;
        if construct && native != Constructor {
            return Err(RuntimeError::TypeError(
                "URL method is not a constructor".into(),
            ));
        }
        match native {
            Constructor | Parse(_) => {
                if native == Constructor && !construct {
                    return Err(RuntimeError::TypeError("URL requires new".into()));
                }
                let parsed = self.url_parse_arguments(args)?;
                if native == Parse(true) {
                    return Ok(Value::Bool(parsed.is_ok()));
                }
                let url = match parsed {
                    Ok(url) => url,
                    Err(()) if native == Parse(false) => return Ok(Value::Null),
                    Err(()) => return Err(RuntimeError::TypeError("Invalid URL".into())),
                };
                let default = self.globals["%WebURL%"];
                let prototype = if native == Constructor {
                    self.constructor_prototype(default)?
                } else {
                    default
                };
                Ok(Value::Object(
                    self.with_roots(|heap| heap.alloc_web_url(url, prototype))?,
                ))
            }
            Create => {
                let id = native::argument(args, 0)
                    .object_id()
                    .ok_or_else(|| RuntimeError::TypeError("Object URL requires a Blob".into()))?;
                let blob = self
                    .heap
                    .web_blob(id)
                    .map_err(|_| RuntimeError::TypeError("Object URL requires a Blob".into()))?
                    .clone();
                let host = self.host_object_urls.as_mut().ok_or_else(|| {
                    RuntimeError::TypeError("Object URL owner unavailable".into())
                })?;
                let url = host
                    .create(
                        &blob.bytes,
                        &String::from_utf16_lossy(blob.media_type.as_code_units()),
                    )
                    .map_err(|error| RuntimeError::TypeError(error.to_string()))?;
                let result = Value::String(url.into());
                self.check_string(&result)?;
                Ok(result)
            }
            Revoke => {
                if args.is_empty() {
                    return Err(RuntimeError::TypeError(
                        "URL revocation requires an input".into(),
                    ));
                }
                let value = self.coerce_string(&args[0])?;
                let value = String::from_utf16_lossy(value.as_code_units());
                self.host_object_urls
                    .as_mut()
                    .ok_or_else(|| RuntimeError::TypeError("Object URL owner unavailable".into()))?
                    .revoke(&value)
                    .map_err(|error| RuntimeError::TypeError(error.to_string()))?;
                Ok(Value::Undefined)
            }
            Serialize | Part(_, _) => {
                let id = receiver
                    .object_id()
                    .ok_or_else(|| RuntimeError::TypeError("Invalid URL receiver".into()))?;
                let url = self
                    .heap
                    .web_url(id)
                    .map_err(|_| RuntimeError::TypeError("Invalid URL receiver".into()))?
                    .clone();
                match native {
                    Serialize => Ok(Value::String(url.to_string().into())),
                    Part(index, false) => Ok(Value::String(
                        match index {
                            0 => quirks::href(&url).to_string(),
                            1 => quirks::origin(&url),
                            2 => quirks::protocol(&url).to_string(),
                            3 => quirks::username(&url).to_string(),
                            4 => quirks::password(&url).to_string(),
                            5 => quirks::host(&url).to_string(),
                            6 => quirks::hostname(&url).to_string(),
                            7 => quirks::port(&url).to_string(),
                            8 => quirks::pathname(&url).to_string(),
                            9 => quirks::search(&url).to_string(),
                            10 => quirks::hash(&url).to_string(),
                            _ => unreachable!("fixed URL accessor"),
                        }
                        .into(),
                    )),
                    Part(index, true) => {
                        let value = self.coerce_string(native::argument(args, 0))?;
                        let value = String::from_utf16_lossy(value.as_code_units());
                        // Conversion may run JS and mutate this same URL first.
                        let mut url = self.heap.web_url(id)?.clone();
                        match index {
                            0 => quirks::set_href(&mut url, &value)
                                .map_err(|_| RuntimeError::TypeError("Invalid URL".into()))?,
                            2 => {
                                let _ = quirks::set_protocol(&mut url, &value);
                            }
                            3 => {
                                let _ = quirks::set_username(&mut url, &value);
                            }
                            4 => {
                                let _ = quirks::set_password(&mut url, &value);
                            }
                            5 => {
                                let _ = quirks::set_host(&mut url, &value);
                            }
                            6 => {
                                let _ = quirks::set_hostname(&mut url, &value);
                            }
                            7 => {
                                let _ = quirks::set_port(&mut url, &value);
                            }
                            8 => quirks::set_pathname(&mut url, &value),
                            9 => quirks::set_search(&mut url, &value),
                            10 => quirks::set_hash(&mut url, &value),
                            _ => unreachable!("fixed writable URL accessor"),
                        }
                        self.with_roots(|heap| heap.set_web_url(id, url))?;
                        Ok(Value::Undefined)
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}
