// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn plural_rules_integer_option(
        &mut self,
        options: &Value,
        name: &str,
        minimum: u8,
        maximum: u8,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite()
            || value.floor() != value
            || !(f64::from(minimum)..=f64::from(maximum)).contains(&value)
        {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(value as u8))
    }

    pub(in super::super) fn plural_rules_rounding_increment(
        &mut self,
        options: &Value,
    ) -> Result<u16, RuntimeError> {
        let value = self.get_property(options, &"roundingIncrement".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let value = self.coerce_number(&value)?;
        const INCREMENTS: &[u16] = &[
            1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000,
        ];
        if !value.is_finite() || value.floor() != value || !INCREMENTS.contains(&(value as u16)) {
            return Err(RuntimeError::RangeError(
                "invalid roundingIncrement option".into(),
            ));
        }
        Ok(value as u16)
    }

    pub(in super::super) fn resolve_plural_rules(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::PluralRules>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;

        // ECMA-402's observable GetOption order is intentional. Keep this
        // sequence adjacent to the corresponding Edition 13 initialization
        // steps; property getters may throw or record every access.
        let locale_matcher = self.locale_matcher(&options)?;
        let rule_type = self
            .string_option(&options, "type", &["cardinal", "ordinal"])?
            .unwrap_or_else(|| "cardinal".into());
        let notation = self
            .string_option(
                &options,
                "notation",
                &["standard", "compact", "scientific", "engineering"],
            )?
            .unwrap_or_else(|| "standard".into());
        let compact_display = self.string_option(&options, "compactDisplay", &["short", "long"])?;
        let minimum_integer_digits = self
            .plural_rules_integer_option(&options, "minimumIntegerDigits", 1, 21)?
            .unwrap_or(1);
        let minimum_fraction_digits =
            self.plural_rules_integer_option(&options, "minimumFractionDigits", 0, 20)?;
        let maximum_fraction_digits =
            self.plural_rules_integer_option(&options, "maximumFractionDigits", 0, 20)?;
        let minimum_significant_digits =
            self.plural_rules_integer_option(&options, "minimumSignificantDigits", 1, 21)?;
        let maximum_significant_digits =
            self.plural_rules_integer_option(&options, "maximumSignificantDigits", 1, 21)?;
        let rounding_increment = self.plural_rules_rounding_increment(&options)?;
        let rounding_mode = self
            .string_option(
                &options,
                "roundingMode",
                &[
                    "ceil",
                    "floor",
                    "expand",
                    "trunc",
                    "halfCeil",
                    "halfFloor",
                    "halfExpand",
                    "halfTrunc",
                    "halfEven",
                ],
            )?
            .unwrap_or_else(|| "halfExpand".into());
        let rounding_priority = self
            .string_option(
                &options,
                "roundingPriority",
                &["auto", "morePrecision", "lessPrecision"],
            )?
            .unwrap_or_else(|| "auto".into());
        let trailing_zero_display = self
            .string_option(&options, "trailingZeroDisplay", &["auto", "stripIfInteger"])?
            .unwrap_or_else(|| "auto".into());

        let minimum_fraction_digits = minimum_fraction_digits.unwrap_or(0);
        let maximum_fraction_digits = maximum_fraction_digits.unwrap_or(3);
        if minimum_fraction_digits > maximum_fraction_digits {
            return Err(RuntimeError::RangeError(
                "minimumFractionDigits exceeds maximumFractionDigits".into(),
            ));
        }
        let (minimum_significant_digits, maximum_significant_digits) =
            match (minimum_significant_digits, maximum_significant_digits) {
                (None, None) => (None, None),
                (minimum, maximum) => {
                    let minimum = minimum.unwrap_or(1);
                    let maximum = maximum.unwrap_or(21);
                    if minimum > maximum {
                        return Err(RuntimeError::RangeError(
                            "minimumSignificantDigits exceeds maximumSignificantDigits".into(),
                        ));
                    }
                    (Some(minimum), Some(maximum))
                }
            };
        let compact_display =
            (notation == "compact").then(|| compact_display.unwrap_or_else(|| "short".into()));
        // `string_option` has already limited `type` to these two spellings.
        let rule_type = if rule_type == "ordinal" {
            blueice_ecma402::PluralRuleType::Ordinal
        } else {
            blueice_ecma402::PluralRuleType::Cardinal
        };
        let data = blueice_ecma402::PluralRules::try_new(
            &locales,
            blueice_ecma402::PluralRulesOptions {
                locale_matcher,
                rule_type,
            },
        )
        .expect("locale negotiation only selects locales with bundled plural rules");
        Ok(Rc::new(intl::PluralRules {
            data,
            rule_type,
            notation,
            compact_display,
            minimum_integer_digits,
            minimum_fraction_digits,
            maximum_fraction_digits,
            minimum_significant_digits,
            maximum_significant_digits,
            rounding_increment,
            rounding_mode,
            rounding_priority,
            trailing_zero_display,
        }))
    }

    pub(in super::super) fn plural_rules_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_plural_rules_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn create_plural_rules(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.PluralRules must be called with new".into(),
            ));
        }
        // This native only exists once the Intl namespace has been built.
        let constructor = self.globals["%Intl.PluralRules%"];
        let default = self
            .heap
            .get(constructor, "prototype")
            .expect("Intl.PluralRules is a live constructor")
            .object_id()
            .expect("Intl.PluralRules.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_plural_rules(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_plural_rules(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn plural_rules_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::PluralRules>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self
                .heap
                .plural_rules(*id)
                .expect("a receiver is a live object")
            {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.PluralRules".into(),
        ))
    }

    pub(in super::super) fn plural_category_name(
        category: blueice_ecma402::PluralCategory,
    ) -> &'static str {
        match category {
            blueice_ecma402::PluralCategory::Zero => "zero",
            blueice_ecma402::PluralCategory::One => "one",
            blueice_ecma402::PluralCategory::Two => "two",
            blueice_ecma402::PluralCategory::Few => "few",
            blueice_ecma402::PluralCategory::Many => "many",
            blueice_ecma402::PluralCategory::Other => "other",
        }
    }

    pub(in super::super) fn plural_rules_categories(&self, data: &intl::PluralRules) -> Vec<Value> {
        let mut seen = [false; 6];
        let mut record = |value: f64| {
            let category = data
                .data
                .select_f64(value)
                .expect("the probe values are finite");
            let index = match category {
                blueice_ecma402::PluralCategory::Zero => 0,
                blueice_ecma402::PluralCategory::One => 1,
                blueice_ecma402::PluralCategory::Two => 2,
                blueice_ecma402::PluralCategory::Few => 3,
                blueice_ecma402::PluralCategory::Many => 4,
                blueice_ecma402::PluralCategory::Other => 5,
            };
            seen[index] = true;
        };
        for value in 0..=10_000 {
            record(f64::from(value));
        }
        for value in [0.1, 0.2, 0.5, 1.1, 1.5, 2.1, 10.1, 1_000_000.0] {
            record(value);
        }
        ["zero", "one", "two", "few", "many", "other"]
            .into_iter()
            .enumerate()
            .filter(|&(index, _)| seen[index])
            .map(|(_, name)| Value::String(name.into()))
            .collect()
    }

    pub(in super::super) fn plural_rules_select(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.plural_rules_data(receiver)?;
        let value = self.coerce_number(value)?;
        if !value.is_finite() {
            return Ok(Value::String("other".into()));
        }
        let category = Self::plural_category_of(&data, value);
        Ok(Value::String(Self::plural_category_name(category).into()))
    }

    /// The plural category of a finite `value`, honoring compact notation.
    /// Bundled plural data answers every finite number, so the only error
    /// the underlying service has (a non-finite input) cannot occur here.
    fn plural_category_of(data: &intl::PluralRules, value: f64) -> blueice_ecma402::PluralCategory {
        let category = if data.notation == "compact" {
            data.data
                .select_compact_f64(value, data.compact_display.as_deref() == Some("long"))
        } else {
            data.data.select_f64(value)
        };
        category.expect("finite numbers always have a plural category")
    }

    pub(in super::super) fn plural_rules_select_range(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.plural_rules_data(receiver)?;
        if *start == Value::Undefined || *end == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Intl.PluralRules selectRange arguments must not be undefined".into(),
            ));
        }
        let start = self.coerce_number(start)?;
        let end = self.coerce_number(end)?;
        if !start.is_finite() || !end.is_finite() {
            return Err(RuntimeError::RangeError(
                "Intl.PluralRules selectRange arguments must be finite".into(),
            ));
        }
        let start_category = Self::plural_category_of(&data, start);
        let category = if start == end {
            start_category
        } else {
            data.data
                .select_range(start_category, Self::plural_category_of(&data, end))
        };
        Ok(Value::String(Self::plural_category_name(category).into()))
    }

    pub(in super::super) fn plural_rules_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.plural_rules_data(receiver)?;
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        let resolved = data.data.resolved_options();
        let mut properties = vec![
            ("locale", Value::String(resolved.locale.clone().into())),
            (
                "type",
                Value::String(
                    match data.rule_type {
                        blueice_ecma402::PluralRuleType::Cardinal => "cardinal",
                        blueice_ecma402::PluralRuleType::Ordinal => "ordinal",
                    }
                    .into(),
                ),
            ),
            ("notation", Value::String(data.notation.clone().into())),
        ];
        if let Some(compact_display) = &data.compact_display {
            properties.push((
                "compactDisplay",
                Value::String(compact_display.clone().into()),
            ));
        }
        properties.extend([
            (
                "minimumIntegerDigits",
                Value::Number(f64::from(data.minimum_integer_digits)),
            ),
            (
                "minimumFractionDigits",
                Value::Number(f64::from(data.minimum_fraction_digits)),
            ),
            (
                "maximumFractionDigits",
                Value::Number(f64::from(data.maximum_fraction_digits)),
            ),
        ]);
        if let (Some(minimum), Some(maximum)) = (
            data.minimum_significant_digits,
            data.maximum_significant_digits,
        ) {
            properties.extend([
                (
                    "minimumSignificantDigits",
                    Value::Number(f64::from(minimum)),
                ),
                (
                    "maximumSignificantDigits",
                    Value::Number(f64::from(maximum)),
                ),
            ]);
        }
        for (key, value) in properties {
            self.define_data(result, key, value, true, true, true)?;
        }
        let categories = self.array_from(self.plural_rules_categories(&data))?;
        self.define_data(result, "pluralCategories", categories, true, true, true)?;
        for (key, value) in [
            (
                "roundingIncrement",
                Value::Number(f64::from(data.rounding_increment)),
            ),
            (
                "roundingMode",
                Value::String(data.rounding_mode.clone().into()),
            ),
            (
                "roundingPriority",
                Value::String(data.rounding_priority.clone().into()),
            ),
            (
                "trailingZeroDisplay",
                Value::String(data.trailing_zero_display.clone().into()),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }

    pub(in super::super) fn segmenter_granularity(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::SegmenterGranularity, RuntimeError> {
        // `string_option` has already limited the value to the three spellings.
        Ok(
            match self
                .string_option(options, "granularity", &["grapheme", "word", "sentence"])?
                .as_deref()
            {
                Some("word") => blueice_ecma402::SegmenterGranularity::Word,
                Some("sentence") => blueice_ecma402::SegmenterGranularity::Sentence,
                _ => blueice_ecma402::SegmenterGranularity::Grapheme,
            },
        )
    }

    pub(in super::super) fn resolve_segmenter(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::Segmenter>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;
        let data = blueice_ecma402::Segmenter::try_new(
            &locales,
            blueice_ecma402::SegmenterOptions {
                locale_matcher: self.locale_matcher(&options)?,
                granularity: self.segmenter_granularity(&options)?,
            },
        )
        .expect("locale negotiation only selects locales with bundled segmenter data");
        Ok(Rc::new(data))
    }

    pub(in super::super) fn segmenter_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_segmenter_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn create_segmenter(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.Segmenter must be called with new".into(),
            ));
        }
        // This native only exists once the Intl namespace has been built.
        let constructor = self.globals["%Intl.Segmenter%"];
        let default = self
            .heap
            .get(constructor, "prototype")
            .expect("Intl.Segmenter is a live constructor")
            .object_id()
            .expect("Intl.Segmenter.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data = self.resolve_segmenter(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_segmenter(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn segmenter_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::Segmenter>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self
                .heap
                .segmenter(*id)
                .expect("a receiver is a live object")
            {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.Segmenter".into(),
        ))
    }

    pub(in super::super) fn segmenter_segment(
        &mut self,
        receiver: &Value,
        input: &Value,
    ) -> Result<Value, RuntimeError> {
        let segmenter = self.segmenter_data(receiver)?;
        let input = self.coerce_string(input)?;
        let (prototype, _) = self.segmenter_internal_prototypes()?;
        let data = Rc::new(intl::Segments::from_segmenter(&segmenter, input));
        self.with_roots(|heap| heap.alloc_segments(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn segments_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::Segments>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self
                .heap
                .segments(*id)
                .expect("a receiver is a live object")
            {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.Segmenter Segments object".into(),
        ))
    }

    pub(in super::super) fn segment_record(
        &mut self,
        data: &intl::Segments,
        index: usize,
    ) -> Result<Value, RuntimeError> {
        let (segment, start, is_word_like) = data.record(index);
        let prototype = self.object_prototype;
        let record = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(record));
        let result = (|| {
            self.define_data(record, "segment", Value::String(segment), true, true, true)?;
            self.define_data(
                record,
                "index",
                Value::Number(start as f64),
                true,
                true,
                true,
            )?;
            self.define_data(
                record,
                "input",
                Value::String(data.input.clone()),
                true,
                true,
                true,
            )?;
            if let Some(is_word_like) = is_word_like {
                self.define_data(
                    record,
                    "isWordLike",
                    Value::Bool(is_word_like),
                    true,
                    true,
                    true,
                )?;
            }
            Ok(Value::Object(record))
        })();
        self.stack.pop();
        result
    }

    pub(in super::super) fn segments_containing(
        &mut self,
        receiver: &Value,
        index: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.segments_data(receiver)?;
        let index = self.coerce_number(index)?;
        let index = if index.is_nan() { 0.0 } else { index.trunc() };
        if !index.is_finite() || index < 0.0 || index >= data.input.as_code_units().len() as f64 {
            return Ok(Value::Undefined);
        }
        // The records partition the whole input, and `index` is inside it.
        let record = data
            .containing(index as usize)
            .expect("every index of the input lies in one segment");
        self.segment_record(&data, record)
    }

    pub(in super::super) fn segments_iterator(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.segments_data(receiver)?;
        // A Segments object only exists once `segment` built the prototypes.
        let prototype = self.globals["%Intl.SegmentIteratorPrototype%"];
        self.with_roots(|heap| heap.alloc_segment_iterator(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn segment_iterator_next(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let Value::Object(iterator) = receiver else {
            return Err(RuntimeError::TypeError(
                "Segmenter iterator next requires an iterator".into(),
            ));
        };
        let next = self
            .heap
            .segment_iterator_next(*iterator)
            .expect("a receiver is a live object");
        if next.is_none()
            && !self
                .heap
                .is_segment_iterator(*iterator)
                .expect("the object was just read successfully")
        {
            return Err(RuntimeError::TypeError(
                "Segmenter iterator next requires a Segmenter iterator".into(),
            ));
        }
        match next {
            Some((data, index)) => self
                .segment_record(&data, index)
                .and_then(|value| self.iterator_result(value, false)),
            None => self.iterator_result(Value::Undefined, true),
        }
    }

    pub(in super::super) fn segmenter_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.segmenter_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (key, value) in [
            ("locale", Value::String(resolved.locale.clone().into())),
            (
                "granularity",
                Value::String(
                    match resolved.granularity {
                        blueice_ecma402::SegmenterGranularity::Grapheme => "grapheme",
                        blueice_ecma402::SegmenterGranularity::Word => "word",
                        blueice_ecma402::SegmenterGranularity::Sentence => "sentence",
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
