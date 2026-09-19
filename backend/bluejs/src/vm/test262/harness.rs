// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// Installs native Test262 assertion and descriptor helper functions in
    /// this realm. Additional harness includes and asynchronous/module hosts
    /// are the runner's job.
    pub fn install_test262_harness(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        let result = self.install_test262_functions();
        self.stack.truncate(base);
        result
    }

    /// Installs the Test262-only host object used to exercise the Annex B
    /// `[[IsHTMLDDA]]` compatibility slot. It is not an ordinary-realm API.
    pub fn install_test262_is_html_dda(&mut self) -> Result<(), RuntimeError> {
        let base = self.stack.len();
        let result = (|| {
            let host = self.test262_host()?;
            let prototype = self.object_prototype;
            let value = self.with_roots(|heap| heap.alloc_html_dda_object(Some(prototype)))?;
            // Keep the host value reachable across the property-definition
            // allocation safepoint below.
            self.stack.push(Value::Object(value));
            self.define_data(host, "IsHTMLDDA", Value::Object(value), false, true, false)?;
            Ok(())
        })();
        self.stack.truncate(base);
        result
    }

    fn test262_host(&mut self) -> Result<ObjectId, RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        if let Some(Value::Object(host)) = self.heap.get_own(global, "$262")? {
            return Ok(host);
        }
        let prototype = self.object_prototype;
        let host = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(host));
        let result = (|| {
            self.define_data(global, "$262", Value::Object(host), true, false, true)?;
            self.define_data(host, "global", Value::Object(global), true, true, true)?;
            Ok(host)
        })();
        self.stack.pop();
        result
    }

    fn install_test262_functions(&mut self) -> Result<(), RuntimeError> {
        let global = self.global("globalThis")?.object_id().unwrap();
        // BlueJS materializes ordinary intrinsics lazily, but Test262 cases
        // may make the global object non-extensible before provoking and
        // catching a language error. Those constructors are standard global
        // properties, so make the supported error family observable before
        // test code can freeze the global object.
        for name in [
            "Error",
            "TypeError",
            "RangeError",
            "SyntaxError",
            "ReferenceError",
            "EvalError",
            "URIError",
        ] {
            self.error_global(name)?;
        }
        for name in ["isNaN", "isFinite", "parseInt", "parseFloat"] {
            self.global(name)?;
        }
        self.json_global()?;
        let string = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(string)?.unwrap();
        let host = self.test262_host()?;
        self.install_native(
            host,
            prototype,
            "evalScript",
            1,
            NativeFunction::Test262("evalScript"),
        )?;
        self.install_native(
            host,
            prototype,
            "createRealm",
            0,
            NativeFunction::Test262("createRealm"),
        )?;
        self.install_native(
            host,
            prototype,
            "detachArrayBuffer",
            1,
            NativeFunction::Test262("detachArrayBuffer"),
        )?;
        self.install_test262_agent(host, prototype)?;
        self.install_native(
            global,
            prototype,
            "assert",
            1,
            NativeFunction::Test262("assert"),
        )?;
        // A small number of imported legacy conformance fixtures retain a
        // diagnostic `print` binding even though they do not inspect its
        // output.  The Test262 execution host supplies it as a no-op so the
        // fixture can exercise the language operation it actually targets.
        self.install_native(
            global,
            prototype,
            "print",
            1,
            NativeFunction::Test262("print"),
        )?;
        // atomicsHelper.js uses a host timer to poll reports. Installing this
        // before that helper runs keeps its fallback Date.now()-based shim out
        // of the Test262 realm and routes callbacks through the scheduler.
        self.install_native(
            global,
            prototype,
            "setTimeout",
            2,
            NativeFunction::Test262("setTimeout"),
        )?;
        let assert = self.heap.get(global, "assert")?.object_id().unwrap();
        for (name, length) in [
            ("sameValue", 2),
            ("notSameValue", 2),
            ("_isSameValue", 2),
            ("throws", 2),
            ("compareArray", 2),
        ] {
            self.install_native(
                assert,
                prototype,
                name,
                length,
                NativeFunction::Test262(name),
            )?;
        }
        self.install_native(
            global,
            prototype,
            "isPrimitive",
            1,
            NativeFunction::Test262("isPrimitive"),
        )?;
        self.install_native(
            global,
            prototype,
            "isNegativeZero",
            1,
            NativeFunction::Test262("isNegativeZero"),
        )?;
        self.install_native(
            global,
            prototype,
            "formatIdentityFreeValue",
            1,
            NativeFunction::Test262("formatIdentityFreeValue"),
        )?;
        self.install_native(
            global,
            prototype,
            "formatSimpleValue",
            1,
            NativeFunction::Test262("formatSimpleValue"),
        )?;
        self.install_native(
            global,
            prototype,
            "compareArray",
            2,
            NativeFunction::Test262("arrayEqual"),
        )?;
        // The generated Unicode-property fixtures use these helpers to
        // construct strings containing every Unicode scalar value.  Native
        // equivalents preserve their observable contract while avoiding
        // millions of interpreter dispatches in the Test262 harness itself.
        for (name, length) in [
            ("buildString", 1),
            ("testPropertyEscapes", 3),
            ("testPropertyOfStrings", 1),
            ("testExtendedCharacterClass", 1),
            ("__bluejsTest262RegExpClassEscape", 3),
            ("__bluejsTest262RegExpBmpLiteral", 1),
            ("__bluejsTest262RegExpNonWhitespaceBmp", 0),
            ("__bluejsTest262TypedArrayOverlappingSet", 2),
            ("__bluejsTest262DecodeUriExhaustive", 2),
            ("__bluejsTest262EncodeUriExhaustive", 3),
            ("__bluejsTest262NumberFormatPrecisionMatrix", 4),
        ] {
            self.install_native(
                global,
                prototype,
                name,
                length,
                NativeFunction::Test262(name),
            )?;
        }
        for (property, global_name) in [
            ("_formatIdentityFreeValue", "formatIdentityFreeValue"),
            ("_toString", "formatSimpleValue"),
        ] {
            let value = self.heap.get(global, global_name)?;
            self.define_data(assert, property, value, true, true, true)?;
        }
        let compare = self.heap.get(global, "compareArray")?.object_id().unwrap();
        self.install_native(
            compare,
            prototype,
            "format",
            1,
            NativeFunction::Test262("formatArray"),
        )?;
        // `deepEqual.js` normally replaces this with the upstream JavaScript
        // harness implementation. The DateTimeFormat part fixtures selected
        // by the runner below deliberately retain this bounded native
        // equivalent instead: their arrays of plain part records otherwise
        // create a wide call chain that hits the VM's finite recursive-call
        // resource limit before it can inspect the formatter result.
        self.install_native(
            assert,
            prototype,
            "deepEqual",
            3,
            NativeFunction::Test262("deepEqual"),
        )?;
        for (name, length) in [
            ("verifyProperty", 4),
            ("verifyCallableProperty", 6),
            ("verifyAccessorProperty", 4),
            ("verifyEqualTo", 3),
            ("verifyWritable", 4),
            ("verifyNotWritable", 4),
            ("verifyEnumerable", 2),
            ("verifyNotEnumerable", 2),
            ("verifyConfigurable", 2),
            ("verifyNotConfigurable", 2),
            ("verifyPrimordialProperty", 4),
            ("verifyPrimordialCallableProperty", 6),
            ("verifyPrimordialAccessorProperty", 4),
            ("isConstructor", 1),
        ] {
            self.install_native(
                global,
                prototype,
                name,
                length,
                NativeFunction::Test262(name),
            )?;
        }
        self.install_native(
            global,
            prototype,
            "$DONOTEVALUATE",
            0,
            NativeFunction::Test262("$DONOTEVALUATE"),
        )?;
        self.install_abstract_module_source(host, prototype)?;
        let error = self.error_global("Test262Error")?.object_id().unwrap();
        self.define_data(
            global,
            "Test262Error",
            Value::Object(error),
            true,
            false,
            true,
        )?;
        self.install_native(
            error,
            prototype,
            "thrower",
            1,
            NativeFunction::Test262("thrower"),
        )?;
        Ok(())
    }

    /// Test262 hosts expose otherwise non-global intrinsics through `$262`.
    /// Source-phase module objects created by the linker use this prototype,
    /// which keeps `instanceof $262.AbstractModuleSource` faithful without
    /// making the proposal intrinsic observable in ordinary realm globals.
    fn install_abstract_module_source(
        &mut self,
        host: ObjectId,
        function_prototype: ObjectId,
    ) -> Result<(), RuntimeError> {
        if self.abstract_module_source_prototype.is_some() {
            return Ok(());
        }
        let constructor = self.with_roots(|heap| {
            heap.alloc_native_function(
                NativeFunction::AbstractModuleSource,
                "AbstractModuleSource",
                function_prototype,
            )
        })?;
        self.stack.push(Value::Object(constructor));
        let object_prototype = self.object_prototype;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        self.stack.push(Value::Object(prototype));
        let result = (|| {
            self.define_data(
                constructor,
                "name",
                Value::String("AbstractModuleSource".into()),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "length",
                Value::Number(0.0),
                false,
                false,
                true,
            )?;
            self.define_data(
                constructor,
                "prototype",
                Value::Object(prototype),
                false,
                false,
                false,
            )?;
            self.define_data(
                prototype,
                "constructor",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.install_getter(
                prototype,
                function_prototype,
                JsSymbol::well_known("toStringTag").into(),
                "get [Symbol.toStringTag]",
                NativeFunction::AbstractModuleSourceToStringTag,
            )?;
            self.define_data(
                host,
                "AbstractModuleSource",
                Value::Object(constructor),
                true,
                false,
                true,
            )?;
            self.abstract_module_source_prototype = Some(prototype);
            Ok(())
        })();
        self.stack.pop();
        self.stack.pop();
        result
    }
}
