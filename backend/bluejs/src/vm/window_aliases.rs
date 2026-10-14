// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-installed aliases for the current top-level page realm.
//! https://html.spec.whatwg.org/multipage/nav-history-apis.html#the-window-object
//! https://webidl.spec.whatwg.org/#es-attributes

use super::*;

impl Vm {
    /// Installs `window`, `self`, `top` and `parent` for a top-level document.
    /// This does not create browsing contexts, a Window constructor, or a
    /// WindowProxy that survives navigation. Older profiles and ShadowRealms
    /// retain their existing globals unless their owner explicitly opts in.
    pub fn install_window_aliases(&mut self) -> Result<(), RuntimeError> {
        const NAMES: [&str; 4] = ["window", "self", "top", "parent"];
        let global = self.global("globalThis")?.object_id().unwrap();
        for name in NAMES {
            if self.heap.get_own(global, name)?.is_some() {
                return Err(RuntimeError::TypeError(
                    "window alias is already defined".into(),
                ));
            }
        }
        let prototype = self.function_prototype()?;
        for (index, name) in NAMES.into_iter().enumerate() {
            let installed = (|| {
                if matches!(name, "self" | "parent") {
                    self.install_native_accessor(
                        global,
                        prototype,
                        name,
                        NativeFunction::WindowAliasGet,
                        NativeFunction::WindowAliasSet(name),
                    )?;
                } else {
                    self.install_native_getter(
                        global,
                        prototype,
                        name,
                        NativeFunction::WindowAliasGet,
                    )?;
                }
                if !self.heap.define_own_property(
                    global,
                    name,
                    PropertyDescriptor {
                        enumerable: Some(true),
                        ..Default::default()
                    },
                )? {
                    return Err(RuntimeError::TypeError(
                        "cannot install window alias".into(),
                    ));
                }
                Ok(())
            })();
            if let Err(error) = installed {
                for installed in &NAMES[..=index] {
                    self.heap.delete(global, *installed)?;
                }
                return Err(error);
            }
        }
        for name in ["window", "top"] {
            if !self.heap.define_own_property(
                global,
                name,
                PropertyDescriptor {
                    configurable: Some(false),
                    ..Default::default()
                },
            )? {
                return Err(RuntimeError::TypeError(
                    "cannot protect window alias".into(),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn window_alias_call(
        &mut self,
        receiver: &Value,
        replacement: Option<(&str, Value)>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        let global = self.globals["globalThis"];
        // Web IDL substitutes the getter/setter's realm global for a nullish
        // receiver, then checks that the receiver implements the interface.
        if construct
            || !(matches!(receiver, Value::Null | Value::Undefined)
                || receiver.object_id() == Some(global))
        {
            return Err(RuntimeError::TypeError(
                "window alias requires the page global receiver".into(),
            ));
        }
        if let Some((name, value)) = replacement {
            self.define_data(global, name, value, true, true, true)?;
            Ok(Value::Undefined)
        } else {
            Ok(Value::Object(global))
        }
    }
}
