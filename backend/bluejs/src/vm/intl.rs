// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::intl;
use icu_collator::options::{AlternateHandling, CaseLevel, CollatorOptions, MaxVariable, Strength};
use icu_collator::preferences::{CollationCaseFirst, CollationNumericOrdering};
use icu_locale_core::{locale, Locale};
use std::rc::Rc;

impl Vm {
    pub(super) fn intl_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("Intl") {
            return Ok(Value::Object(id));
        }
        let string = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(string)?.unwrap();
        let object_prototype = self.object_prototype;
        let namespace = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(namespace)?;
        let mut constructor_root = None;
        let result = (|| {
            self.define_data(namespace, JsSymbol::well_known("toStringTag"), Value::String("Intl".into()), false, false, true)?;
            self.install_native(namespace, function_prototype, "getCanonicalLocales", 1, NativeFunction::CanonicalLocales)?;
            self.install_native(namespace, function_prototype, "Collator", 0, NativeFunction::Collator)?;
            let constructor = self.heap.get(namespace, "Collator")?.object_id().unwrap();
            constructor_root = Some(self.heap.root(constructor)?);
            let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
            self.define_data(constructor, "prototype", Value::Object(prototype), false, false, false)?;
            self.define_data(prototype, "constructor", Value::Object(constructor), true, false, true)?;
            self.define_data(prototype, JsSymbol::well_known("toStringTag"), Value::String("Intl.Collator".into()), false, false, true)?;
            self.install_native(constructor, function_prototype, "supportedLocalesOf", 1, NativeFunction::SupportedLocales)?;
            self.install_native(prototype, function_prototype, "resolvedOptions", 0, NativeFunction::CollatorResolvedOptions)?;
            self.install_getter(prototype, function_prototype, "compare".into(), "get compare", NativeFunction::CollatorCompareGetter)?;
            self.globals.insert("%Intl.Collator%".into(), constructor);
            self.globals.insert("Intl".into(), namespace);
            Ok(Value::Object(namespace))
        })();
        if result.is_err() {
            self.heap.unroot(root)?;
            if let Some(root) = constructor_root {
                self.heap.unroot(root)?;
            }
        }
        result
    }

    pub(super) fn canonical_locales(&mut self, locales: &Value) -> Result<Vec<Locale>, RuntimeError> {
        if *locales == Value::Undefined {
            return Ok(Vec::new());
        }
        if let Value::String(string) = locales {
            return Ok(vec![intl::canonicalize(string)?]);
        }
        let object = self.coerce_object(locales)?;
        self.stack.push(Value::Object(object));
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        let length = self.coerce_length(&length)? as u64;
        let mut result = Vec::new();
        for index in 0..length {
            self.charge_step()?;
            let key: PropertyName = index.to_string().into();
            let mut current = Some(object);
            let mut present = false;
            while let Some(id) = current {
                if self.heap.get_own_property_descriptor(id, &key)?.is_some() {
                    present = true;
                    break;
                }
                current = self.heap.prototype(id)?;
            }
            if !present {
                continue;
            }
            let value = self.get_property(&Value::Object(object), &key)?;
            if !matches!(value, Value::Object(_) | Value::String(_)) {
                return Err(RuntimeError::TypeError("locale must be a String or Object".into()));
            }
            let locale = intl::canonicalize(&self.coerce_string(&value)?)?;
            if !result.contains(&locale) {
                result.push(locale);
            }
        }
        Ok(result)
    }

    fn intl_options(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let object = if *value == Value::Undefined { self.with_roots(|heap| heap.alloc_object(None))? } else { self.coerce_object(value)? };
        let result = Value::Object(object);
        self.stack.push(result.clone());
        Ok(result)
    }

    fn string_option(&mut self, options: &Value, name: &str, allowed: &[&str]) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        let string = string.to_utf8().map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))?;
        if !allowed.is_empty() && !allowed.contains(&string.as_str()) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(string))
    }

    pub(super) fn supported_locales(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        self.string_option(&options, "localeMatcher", &["lookup", "best fit"])?;
        self.array_from(locales.iter().filter(|l| intl::supported(l)).map(|l| Value::String(l.to_string().into())).collect())
    }

    pub(super) fn resolve_collator(&mut self, locales: &Value, options: &Value) -> Result<Rc<intl::Collator>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_options(options)?;
        let usage = self.string_option(&options, "usage", &["sort", "search"])?.unwrap_or_else(|| "sort".into());
        self.string_option(&options, "localeMatcher", &["lookup", "best fit"])?;
        let collation = self.string_option(&options, "collation", &[])?;
        if let Some(collation) = &collation {
            if !collation.split('-').all(|part| (3..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())) {
                return Err(RuntimeError::RangeError("invalid collation option".into()));
            }
        }
        let numeric = self.get_property(&options, &"numeric".into())?;
        let numeric = if numeric == Value::Undefined { None } else { Some(primitive::truthy(&numeric)) };
        let case_first = self.string_option(&options, "caseFirst", &["upper", "lower", "false"])?;
        let selected = locales.into_iter().find(intl::supported).unwrap_or(locale!("en-US"));
        let mut resolved = Locale::from(selected.id.clone());
        let mut algorithm_locale = resolved.clone();
        let mut selected_collation = "default".to_string();
        // Retain a supported extension only when an option does not override it.
        for (key, option) in [("co", collation.map(|s| s.to_ascii_lowercase())), ("kf", case_first), ("kn", numeric.map(|b| b.to_string()))] {
            let extension = intl::keyword(&selected, key).map(|s| if key == "kn" && s.is_empty() { "true".into() } else { s });
            let valid = |value: &str| match key {
                "co" => usage == "sort" && intl::supports_collation(&selected, value),
                "kf" => ["upper", "lower", "false"].contains(&value),
                _ => ["true", "false"].contains(&value),
            };
            let extension = extension.filter(|s| valid(s));
            let choice = option.filter(|s| valid(s)).or_else(|| extension.clone());
            if let Some(value) = choice {
                if extension.as_ref() == Some(&value) {
                    resolved.extensions.unicode.keywords.set(key.parse().unwrap(), value.parse().unwrap());
                }
                algorithm_locale.extensions.unicode.keywords.set(key.parse().unwrap(), value.parse().unwrap());
                if key == "co" {
                    selected_collation = value;
                }
            }
        }
        let mut preferences = intl::preferences(&algorithm_locale);
        if usage == "search" {
            preferences.collation_type = Some(icu_collator::preferences::CollationType::Search);
        }
        let sensitivity = self.string_option(&options, "sensitivity", &["base", "accent", "case", "variant"])?.unwrap_or_else(|| "variant".into());
        let punctuation = self.get_property(&options, &"ignorePunctuation".into())?;
        let ignore_punctuation = if punctuation == Value::Undefined { selected.id.language.as_str() == "th" } else { primitive::truthy(&punctuation) };
        let mut options = CollatorOptions::default();
        options.strength = Some(match sensitivity.as_str() {
            "base" | "case" => Strength::Primary,
            "accent" => Strength::Secondary,
            _ => Strength::Tertiary,
        });
        options.case_level = Some(if sensitivity == "case" { CaseLevel::On } else { CaseLevel::Off });
        options.alternate_handling = Some(if ignore_punctuation { AlternateHandling::Shifted } else { AlternateHandling::NonIgnorable });
        options.max_variable = Some(MaxVariable::Punctuation);
        // All preferences are validated above; the bundled provider includes
        // root fallback plus every advertised tailoring. Missing data here
        // is a broken build invariant, not a user locale RangeError.
        let algorithm = icu_collator::Collator::try_new(preferences, options).expect("bundled ICU collation data includes validated preferences");
        Ok(Rc::new(intl::Collator { algorithm, locale: resolved.to_string(), usage, sensitivity, ignore_punctuation, collation: selected_collation }))
    }

    pub(super) fn create_collator(&mut self, args: &[Value], construct: bool) -> Result<Value, RuntimeError> {
        self.intl_global()?;
        let constructor = self.globals["%Intl.Collator%"];
        let default = self.heap.get(constructor, "prototype")?.object_id().unwrap();
        let prototype = if construct { self.constructor_prototype(default)? } else { default };
        self.stack.push(Value::Object(prototype));
        let data = self.resolve_collator(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_collator(data, prototype)).map(Value::Object)
    }

    pub(super) fn collator_data(&self, value: &Value) -> Result<Rc<intl::Collator>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.collator(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError("receiver is not an Intl.Collator".into()))
    }

    pub(super) fn collator_compare_getter(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        self.collator_data(receiver)?;
        let id = receiver.object_id().unwrap();
        if let Some(function) = self.heap.collator_compare(id) {
            return Ok(Value::Object(function));
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let target = self.with_roots(|heap| heap.alloc_native_function(NativeFunction::CollatorCompare, "", prototype))?;
        let bound = crate::heap::BoundFunction { target, this: receiver.clone(), args: vec![], constructible: false };
        let function = self.with_roots(|heap| heap.alloc_bound_function(bound, Some(prototype)))?;
        self.stack.push(Value::Object(function));
        self.define_data(function, "name", Value::String("".into()), false, false, true)?;
        self.define_data(function, "length", Value::Number(2.0), false, false, true)?;
        self.heap.set_collator_compare(id, function);
        Ok(Value::Object(function))
    }

    pub(super) fn collator_resolved_options(&mut self, receiver: &Value) -> Result<Value, RuntimeError> {
        let data = self.collator_data(receiver)?;
        let resolved = data.algorithm.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (key, value) in [
            ("locale", Value::String(data.locale.as_str().into())),
            ("usage", Value::String(data.usage.as_str().into())),
            ("sensitivity", Value::String(data.sensitivity.as_str().into())),
            ("ignorePunctuation", Value::Bool(data.ignore_punctuation)),
            ("collation", Value::String(data.collation.as_str().into())),
            ("numeric", Value::Bool(resolved.numeric == CollationNumericOrdering::True)),
            (
                "caseFirst",
                Value::String(
                    match resolved.case_first {
                        CollationCaseFirst::Upper => "upper",
                        CollationCaseFirst::Lower => "lower",
                        _ => "false",
                    }
                    .into(),
                ),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }
}
