// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Intl.PluralRules, Intl.Segmenter, Intl.Collator and Intl.Locale: option
//! reads that throw or convert badly at every position, wrong receivers,
//! every plural category, and allocation failure at each step.

mod cov_g3_support;
use cov_g3_support::{assert_true, option_read_sweep, sweep_each};

#[test]
fn plural_rules_options_are_read_and_validated_in_order() {
    let factory = "(options) => new Intl.PluralRules('en', options)";
    assert_true(&option_read_sweep(factory));
    for source in [
        // Inconsistent digit ranges.
        "try { new Intl.PluralRules('en', { minimumFractionDigits: 5, maximumFractionDigits: 1 }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en', { minimumSignificantDigits: 5, maximumSignificantDigits: 1 }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en', { roundingIncrement: 3 }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en', { roundingIncrement: Infinity }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en', { roundingIncrement: 2.5 }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en', { minimumIntegerDigits: NaN }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en', { minimumIntegerDigits: 0 }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en', { maximumFractionDigits: 21 }); false } catch (e) { e instanceof RangeError }",
        "try { Intl.PluralRules('en'); false } catch (e) { e instanceof TypeError }",
        "try { new Intl.PluralRules('en-'); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules({ length: 1, get 0() { throw new EvalError('l') } }); false } catch (e) { e instanceof EvalError }",
        "try { new Intl.PluralRules('en', null); false } catch (e) { e instanceof TypeError }",
        "try { new Intl.PluralRules('en', { localeMatcher: 'bogus' }); false } catch (e) { e instanceof RangeError }",
        // One significant-digit bound alone is fine and shows in the result.
        "const a = new Intl.PluralRules('en', { minimumSignificantDigits: 2 }).resolvedOptions();
         a.minimumSignificantDigits === 2 && a.maximumSignificantDigits === 21",
        "const b = new Intl.PluralRules('en', { maximumSignificantDigits: 4 }).resolvedOptions();
         b.minimumSignificantDigits === 1 && b.maximumSignificantDigits === 4",
        "const c = new Intl.PluralRules('en', { type: 'ordinal', roundingIncrement: 5, roundingMode: 'ceil', roundingPriority: 'morePrecision', trailingZeroDisplay: 'stripIfInteger' }).resolvedOptions();
         c.type === 'ordinal' && c.roundingIncrement === 5 && c.roundingMode === 'ceil'
           && c.roundingPriority === 'morePrecision' && c.trailingZeroDisplay === 'stripIfInteger'",
        // Compact notation carries a display, defaulting to short.
        "const d = new Intl.PluralRules('en', { notation: 'compact' }).resolvedOptions();
         d.notation === 'compact' && d.compactDisplay === 'short'",
        "new Intl.PluralRules('en', { notation: 'compact', compactDisplay: 'long' }).resolvedOptions().compactDisplay === 'long'",
        "!('compactDisplay' in new Intl.PluralRules('en').resolvedOptions())",
        // Subclassing and a new.target whose prototype lookup throws.
        "class Sub extends Intl.PluralRules {} new Sub('en') instanceof Sub",
        "const target = new Proxy(function () {}, { get() { throw new EvalError('proto') } });
         try { Reflect.construct(Intl.PluralRules, ['en'], target); false } catch (e) { e instanceof EvalError }",
    ] {
        assert_true(source);
    }
}

#[test]
fn plural_rules_report_every_category_and_select_with_and_without_compact_notation() {
    for source in [
        "const ar = new Intl.PluralRules('ar').resolvedOptions().pluralCategories;
         ['zero', 'one', 'two', 'few', 'many', 'other'].every((name) => ar.includes(name))",
        "const ru = new Intl.PluralRules('ru').resolvedOptions().pluralCategories;
         ru.includes('many') && !ru.includes('zero')",
        "new Intl.PluralRules('en').select(1) === 'one' && new Intl.PluralRules('en').select(2) === 'other'",
        "new Intl.PluralRules('ar').select(0) === 'zero' && new Intl.PluralRules('ar').select(11) === 'many'",
        "new Intl.PluralRules('en').select(NaN) === 'other' && new Intl.PluralRules('en').select(Infinity) === 'other'",
        "typeof new Intl.PluralRules('fr', { notation: 'compact' }).select(1500000) === 'string'",
        "typeof new Intl.PluralRules('fr', { notation: 'compact', compactDisplay: 'long' }).select(2000000) === 'string'",
        "typeof new Intl.PluralRules('fr', { notation: 'compact' }).select(0) === 'string'",
        "typeof new Intl.PluralRules('en').selectRange(1, 5) === 'string'",
        "new Intl.PluralRules('en').selectRange(1, 1) === 'one'",
        "typeof new Intl.PluralRules('fr', { notation: 'compact' }).selectRange(1000000, 3000000) === 'string'",
        "new Intl.PluralRules('gv').select(1) === 'one'",
        // Coercion and validation of the arguments.
        "try { new Intl.PluralRules('en').select({ valueOf() { throw new EvalError('v') } }); false } catch (e) { e instanceof EvalError }",
        "try { new Intl.PluralRules('en').selectRange(undefined, 1); false } catch (e) { e instanceof TypeError }",
        "try { new Intl.PluralRules('en').selectRange(1, undefined); false } catch (e) { e instanceof TypeError }",
        "try { new Intl.PluralRules('en').selectRange(NaN, 1); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en').selectRange(1, Infinity); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.PluralRules('en').selectRange({ valueOf() { throw new EvalError('s') } }, 1); false } catch (e) { e instanceof EvalError }",
        "try { new Intl.PluralRules('en').selectRange(1, { valueOf() { throw new EvalError('e') } }); false } catch (e) { e instanceof EvalError }",
        // Receivers.
        "const proto = Intl.PluralRules.prototype;
         [undefined, 1, {}, new Intl.Segmenter()].every((receiver) => {
           for (const method of ['select', 'selectRange', 'resolvedOptions']) {
             try { proto[method].call(receiver, 1, 2); return false } catch (e) { if (!(e instanceof TypeError)) return false }
           }
           return true;
         })",
    ] {
        assert_true(source);
    }
}

#[test]
fn plural_rules_supported_locales_read_their_arguments() {
    for source in [
        "Intl.PluralRules.supportedLocalesOf(['en', 'fr']).length === 2",
        "Intl.PluralRules.supportedLocalesOf('en', { localeMatcher: 'lookup' }).length === 1",
        "try { Intl.PluralRules.supportedLocalesOf('en', { localeMatcher: 'bogus' }); false } catch (e) { e instanceof RangeError }",
        "try { Intl.PluralRules.supportedLocalesOf(['en-'], {}); false } catch (e) { e instanceof RangeError }",
        "try { Intl.PluralRules.supportedLocalesOf('en', null); false } catch (e) { e instanceof TypeError }",
    ] {
        assert_true(source);
    }
    assert_true(&option_read_sweep(
        "(options) => Intl.PluralRules.supportedLocalesOf('en', options)",
    ));
}

#[test]
fn segmenter_options_segments_and_iterators() {
    assert_true(&option_read_sweep(
        "(options) => new Intl.Segmenter('en', options)",
    ));
    for source in [
        "new Intl.Segmenter().resolvedOptions().granularity === 'grapheme'",
        "new Intl.Segmenter('en', { granularity: 'word' }).resolvedOptions().granularity === 'word'",
        "new Intl.Segmenter('en', { granularity: 'sentence' }).resolvedOptions().granularity === 'sentence'",
        "try { new Intl.Segmenter('en', { granularity: 'line' }); false } catch (e) { e instanceof RangeError }",
        "try { Intl.Segmenter('en'); false } catch (e) { e instanceof TypeError }",
        "try { new Intl.Segmenter('en-'); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Segmenter('en', null); false } catch (e) { e instanceof TypeError }",
        "try { new Intl.Segmenter('en', { localeMatcher: 'bogus' }); false } catch (e) { e instanceof RangeError }",
        "try { Intl.Segmenter.supportedLocalesOf('en-'); false } catch (e) { e instanceof RangeError }",
        "try { Intl.Segmenter.supportedLocalesOf('en', null); false } catch (e) { e instanceof TypeError }",
        "try { Intl.Segmenter.supportedLocalesOf('en', { localeMatcher: 'bogus' }); false } catch (e) { e instanceof RangeError }",
        "class Sub extends Intl.Segmenter {} new Sub('en') instanceof Sub",
        "const target = new Proxy(function () {}, { get() { throw new EvalError('proto') } });
         try { Reflect.construct(Intl.Segmenter, ['en'], target); false } catch (e) { e instanceof EvalError }",
        "Intl.Segmenter.supportedLocalesOf(['en', 'fr']).length === 2
           && Intl.Segmenter.supportedLocalesOf('en', { localeMatcher: 'lookup' }).length === 1",
        // Segments: containing() normalizes its index.
        "const segments = new Intl.Segmenter('en', { granularity: 'word' }).segment('ab cd');
         segments.containing(0).segment === 'ab' && segments.containing(0).isWordLike === true
           && segments.containing(2).segment === ' ' && segments.containing(2).isWordLike === false
           && segments.containing(NaN).segment === 'ab' && segments.containing(1.9).segment === 'ab'
           && segments.containing(-1) === undefined && segments.containing(5) === undefined
           && segments.containing(Infinity) === undefined",
        "const graphemes = new Intl.Segmenter().segment('ab');
         graphemes.containing(1).segment === 'b' && !('isWordLike' in graphemes.containing(1))",
        "const list = [...new Intl.Segmenter('en', { granularity: 'word' }).segment('ab cd')];
         list.length === 3 && list[2].index === 3 && list[2].input === 'ab cd'",
        "try { new Intl.Segmenter().segment({ toString() { throw new EvalError('s') } }); false } catch (e) { e instanceof EvalError }",
        "try { new Intl.Segmenter().segment('a').containing({ valueOf() { throw new EvalError('i') } }); false } catch (e) { e instanceof EvalError }",
        // Iterator protocol details.
        "const iterator = new Intl.Segmenter().segment('a')[Symbol.iterator]();
         const first = iterator.next(); const second = iterator.next();
         first.done === false && first.value.segment === 'a' && second.done === true && second.value === undefined
           && iterator.next().done === true",
        // Receivers.
        "const segmentsProto = Object.getPrototypeOf(new Intl.Segmenter().segment('a'));
         const iteratorProto = Object.getPrototypeOf(new Intl.Segmenter().segment('a')[Symbol.iterator]());
         [undefined, 1, {}, new Intl.PluralRules()].every((receiver) => {
           for (const call of [
             () => segmentsProto.containing.call(receiver, 0),
             () => segmentsProto[Symbol.iterator].call(receiver),
             () => iteratorProto.next.call(receiver),
             () => Intl.Segmenter.prototype.segment.call(receiver, 'a'),
             () => Intl.Segmenter.prototype.resolvedOptions.call(receiver),
           ]) {
             try { call(); return false } catch (e) { if (!(e instanceof TypeError)) return false }
           }
           return true;
         })",
    ] {
        assert_true(source);
    }
}

#[test]
fn collator_reports_every_sensitivity_and_rejects_foreign_receivers() {
    for source in [
        "['base', 'accent', 'case', 'variant'].every((sensitivity) =>
           new Intl.Collator('en', { sensitivity }).resolvedOptions().sensitivity === sensitivity)",
        "new Intl.Collator('en', { usage: 'search' }).resolvedOptions().usage === 'search'",
        "['upper', 'lower', 'false'].every((caseFirst) =>
           new Intl.Collator('en', { caseFirst }).resolvedOptions().caseFirst === caseFirst)",
        "const collator = new Intl.Collator('en'); collator.compare === collator.compare && collator.compare('a', 'b') < 0",
        "const compare = new Intl.Collator('en').compare; compare.length === 2 && compare.name === ''",
        "const getter = Object.getOwnPropertyDescriptor(Intl.Collator.prototype, 'compare').get;
         const resolved = Intl.Collator.prototype.resolvedOptions;
         [undefined, 1, {}, new Intl.Segmenter()].every((receiver) => {
           for (const call of [() => getter.call(receiver), () => resolved.call(receiver)]) {
             try { call(); return false } catch (e) { if (!(e instanceof TypeError)) return false }
           }
           return true;
         })",
    ] {
        assert_true(source);
    }
    assert_true(&option_read_sweep(
        "(options) => new Intl.Collator('en', options)",
    ));
}

#[test]
fn locale_options_keywords_and_information() {
    assert_true(&option_read_sweep(
        "(options) => new Intl.Locale('en-US', options)",
    ));
    for source in [
        // Tags: strings, Locale objects and other objects.
        "new Intl.Locale('en-US').toString() === 'en-US'",
        "new Intl.Locale(new Intl.Locale('fr-CA')).toString() === 'fr-CA'",
        "new Intl.Locale({ toString() { return 'de-AT' } }).toString() === 'de-AT'",
        "try { new Intl.Locale({ toString() { throw new EvalError('t') } }); false } catch (e) { e instanceof EvalError }",
        "try { new Intl.Locale(5); false } catch (e) { e instanceof TypeError }",
        "try { new Intl.Locale({ toString() { return 'en-' } }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Locale(); false } catch (e) { e instanceof TypeError }",
        "try { Intl.Locale('en'); false } catch (e) { e instanceof TypeError }",
        "try { new Intl.Locale('en_US'); false } catch (e) { e instanceof RangeError }",
        "class Sub extends Intl.Locale {} new Sub('en') instanceof Sub",
        "const target = new Proxy(function () {}, { get() { throw new EvalError('proto') } });
         try { Reflect.construct(Intl.Locale, ['en'], target); false } catch (e) { e instanceof EvalError }",
        // Language-id options.
        "const l = new Intl.Locale('en', { language: 'fr', script: 'Latn', region: 'CA', variants: 'fonipa-1996' });
         l.language === 'fr' && l.script === 'Latn' && l.region === 'CA' && l.variants === '1996-fonipa'",
        "['language', 'script', 'region', 'variants'].every((name) => {
           try { new Intl.Locale('en', { [name]: 'x' }); return false } catch (e) { return e instanceof RangeError }
         })",
        "try { new Intl.Locale('en', { variants: '' }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Locale('en', { variants: 'fonipa-fonipa' }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Locale('en', { variants: 'fonipa-x' }); false } catch (e) { e instanceof RangeError }",
        // Unicode keyword options.
        "const k = new Intl.Locale('en', { calendar: 'gregory', collation: 'emoji', hourCycle: 'h23', caseFirst: 'upper', numeric: true, numberingSystem: 'latn', firstDayOfWeek: 'mon' });
         k.calendar === 'gregory' && k.collation === 'emoji' && k.hourCycle === 'h23' && k.caseFirst === 'upper'
           && k.numeric === true && k.numberingSystem === 'latn' && k.firstDayOfWeek === 'mon'",
        "new Intl.Locale('en', { numeric: false }).numeric === false && new Intl.Locale('en').numeric === false",
        "['0', '7'].every((day) => new Intl.Locale('en', { firstDayOfWeek: day }).firstDayOfWeek === 'sun')",
        "[['1', 'mon'], ['2', 'tue'], ['3', 'wed'], ['4', 'thu'], ['5', 'fri'], ['6', 'sat']].every(([day, name]) =>
           new Intl.Locale('en', { firstDayOfWeek: day }).firstDayOfWeek === name)",
        "try { new Intl.Locale('en', { firstDayOfWeek: '' }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Locale('en', { firstDayOfWeek: 'a' }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Locale('en', { calendar: 'a' }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Locale('en', { calendar: '' }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Locale('en', { calendar: 'toolongvalue' }); false } catch (e) { e instanceof RangeError }",
        "try { new Intl.Locale('en', { calendar: 'ab$cd' }); false } catch (e) { e instanceof RangeError }",
        // Locale-information queries.
        "new Intl.Locale('en-US').maximize().toString() === 'en-Latn-US' && new Intl.Locale('en-Latn-US').minimize().toString() === 'en'",
        "new Intl.Locale('posix').maximize().toString() === 'posix' && new Intl.Locale('posix').minimize().toString() === 'posix'",
        "new Intl.Locale('ar').getTextInfo().direction === 'rtl' && new Intl.Locale('en').getTextInfo().direction === 'ltr'",
        "const week = new Intl.Locale('en-US').getWeekInfo(); week.firstDay === 7 && Array.isArray(week.weekend)",
        "Array.isArray(new Intl.Locale('en-US').getCalendars()) && Array.isArray(new Intl.Locale('en-US').getCollations())",
        "Array.isArray(new Intl.Locale('en-US').getHourCycles()) && Array.isArray(new Intl.Locale('en-US').getNumberingSystems())",
        "Array.isArray(new Intl.Locale('en-US').getTimeZones()) && new Intl.Locale('en').getTimeZones() === undefined",
        // Receivers.
        "const proto = Intl.Locale.prototype;
         [undefined, 1, {}, new Intl.Segmenter()].every((receiver) => {
           for (const name of ['maximize', 'minimize', 'toString', 'getCalendars', 'getWeekInfo', 'getTextInfo', 'getTimeZones']) {
             try { proto[name].call(receiver); return false } catch (e) { if (!(e instanceof TypeError)) return false }
           }
           for (const name of ['baseName', 'language', 'script', 'region', 'calendar', 'numeric']) {
             try { Object.getOwnPropertyDescriptor(proto, name).get.call(receiver); return false } catch (e) { if (!(e instanceof TypeError)) return false }
           }
           return true;
         })",
    ] {
        assert_true(source);
    }
}

#[test]
fn locale_getters_report_absent_parts_as_undefined() {
    assert_true(
        "const l = new Intl.Locale('en');
         l.script === undefined && l.region === undefined && l.variants === undefined
           && l.calendar === undefined && l.collation === undefined && l.hourCycle === undefined
           && l.caseFirst === undefined && l.numberingSystem === undefined && l.firstDayOfWeek === undefined
           && l.baseName === 'en' && l.language === 'en'",
    );
}

/// Building the Intl namespace is not what these sweeps are about.
const WARM_UP: &str = "Intl.PluralRules; Intl.Segmenter; Intl.Collator; Intl.Locale;";

/// Each operation is swept on its own, after a warm-up that has built what it
/// needs (objects live on `globalThis`), so every sweep stays small and each
/// allocation the operation makes fails in turn.
#[test]
fn plural_rules_survive_every_allocation_failure() {
    let rules = "Intl.PluralRules; globalThis.rules = new Intl.PluralRules('ar', { notation: 'compact', minimumSignificantDigits: 2 });";
    sweep_each(&[
        (
            WARM_UP,
            "new Intl.PluralRules('ar', { notation: 'compact', minimumSignificantDigits: 2 })",
        ),
        (rules, "globalThis.rules.select(3)"),
        (rules, "globalThis.rules.selectRange(1, 5)"),
        (rules, "globalThis.rules.resolvedOptions()"),
        (WARM_UP, "Intl.PluralRules.supportedLocalesOf(['en'])"),
    ]);
}

#[test]
fn segmenter_survives_every_allocation_failure() {
    let segmenter =
        "Intl.Segmenter; globalThis.segmenter = new Intl.Segmenter('en', { granularity: 'word' });";
    let segments =
        "Intl.Segmenter; globalThis.segmenter = new Intl.Segmenter('en', { granularity: 'word' });
                    globalThis.segments = segmenter.segment('ab cd'); [...segments];";
    sweep_each(&[
        (WARM_UP, "new Intl.Segmenter('en', { granularity: 'word' })"),
        (segmenter, "globalThis.segmenter.segment('ab cd')"),
        (segments, "globalThis.segments.containing(1)"),
        (segments, "[...globalThis.segments]"),
        (segments, "globalThis.segmenter.resolvedOptions()"),
        (WARM_UP, "Intl.Segmenter.supportedLocalesOf(['en'])"),
    ]);
}

#[test]
fn collator_and_locale_survive_every_allocation_failure() {
    let collator =
        "Intl.Collator; globalThis.collator = new Intl.Collator('en', { sensitivity: 'base' });";
    let locale = "Intl.Locale; globalThis.locale = new Intl.Locale('en-US', { calendar: 'gregory', numeric: true });";
    sweep_each(&[
        (WARM_UP, "new Intl.Collator('en', { sensitivity: 'base' })"),
        (collator, "globalThis.collator.compare"),
        (collator, "globalThis.collator.compare('a', 'b')"),
        (collator, "globalThis.collator.resolvedOptions()"),
        (
            WARM_UP,
            "new Intl.Locale('en-US', { calendar: 'gregory', numeric: true, firstDayOfWeek: 'mon', hourCycle: 'h23', caseFirst: 'upper', variants: 'fonipa' })",
        ),
        (locale, "globalThis.locale.maximize()"),
        (locale, "globalThis.locale.minimize()"),
        (locale, "globalThis.locale.toString()"),
        (locale, "globalThis.locale.baseName"),
        (locale, "globalThis.locale.getWeekInfo()"),
        (locale, "globalThis.locale.getTextInfo()"),
        (locale, "globalThis.locale.getCalendars()"),
        (locale, "globalThis.locale.getTimeZones()"),
        (WARM_UP, "new Intl.Locale('posix').maximize()"),
    ]);
}

/// A heap ceiling fails an allocation only when nothing earlier in the run
/// demanded as much, so these start from a warm-up that has already built
/// everything else the operation touches.
#[test]
fn results_made_after_a_quiet_start_fail_cleanly_at_every_allocation() {
    let rules = "Intl.PluralRules; globalThis.rules = new Intl.PluralRules('en');";
    let segments =
        "Intl.Segmenter; globalThis.segmenter = new Intl.Segmenter('en', { granularity: 'word' });
                    globalThis.segments = segmenter.segment('ab cd');";
    let locale = "Intl.Locale; globalThis.locale = new Intl.Locale('en-US');";
    sweep_each(&[
        (rules, "globalThis.rules.resolvedOptions()"),
        (segments, "globalThis.segments.containing(1)"),
        (segments, "globalThis.segments[Symbol.iterator]()"),
        (segments, "globalThis.segmenter.resolvedOptions()"),
        (locale, "globalThis.locale.getTextInfo()"),
        (locale, "globalThis.locale.getWeekInfo()"),
    ]);
}
