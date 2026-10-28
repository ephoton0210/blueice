// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Compare every decoded declaration-map segment and every metadata field.
use serde_json::{json, Value};

pub fn decoded(text: &str) -> Value {
    let mut map: Value = serde_json::from_str(text).unwrap();
    let encoded = map.as_object_mut().unwrap().remove("mappings").unwrap();
    let mut source = 0i64;
    let mut line = 0i64;
    let mut column = 0i64;
    let mut name = 0i64;
    let mut segments = Vec::new();
    for (generated_line, encoded_line) in encoded.as_str().unwrap().split(';').enumerate() {
        let mut generated_column = 0i64;
        for encoded in encoded_line.split(',').filter(|value| !value.is_empty()) {
            let mut fields = Vec::new();
            let mut value = 0i64;
            let mut shift = 0u32;
            for byte in encoded.bytes() {
                let digit = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
                    .iter()
                    .position(|candidate| *candidate == byte)
                    .unwrap() as i64;
                assert!(shift <= 55);
                value += (digit & 31) << shift;
                if digit & 32 != 0 {
                    shift += 5;
                } else {
                    fields.push(if value & 1 != 0 {
                        -(value >> 1)
                    } else {
                        value >> 1
                    });
                    value = 0;
                    shift = 0;
                }
            }
            assert_eq!(shift, 0);
            assert!(matches!(fields.len(), 1 | 4 | 5));
            generated_column += fields[0];
            let mut entry = vec![generated_line as i64, generated_column];
            if fields.len() > 1 {
                source += fields[1];
                line += fields[2];
                column += fields[3];
                entry.extend([source, line, column]);
                if fields.len() == 5 {
                    name += fields[4];
                    entry.push(name);
                }
            }
            segments.push(entry);
        }
    }
    json!({"metadata":map,"segments":segments})
}
