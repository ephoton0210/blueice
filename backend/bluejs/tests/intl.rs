// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{compile, parse, RuntimeError, Value, Vm};

fn evaluate(source: &str) -> Result<Value, RuntimeError> {
    Vm::default().execute(&compile(&parse(source).unwrap()).unwrap())
}

#[test]
fn locale_casing_and_canonicalization() {
    for source in [
        "'Iİ'.toLocaleLowerCase('tr') === 'ıi'",
        "'iı'.toLocaleUpperCase('az') === 'İI'",
        r"'I\u0301'.toLocaleLowerCase('lt') === 'i\u0307\u0301'",
        r"'I\ud800İ'.toLocaleLowerCase('tr') === 'ı\ud800i'",
        "Intl.getCanonicalLocales(['EN-us','en-US','iw']).join(',') === 'en-US,he'",
        "Intl.getCanonicalLocales({length:3,1:'de'}).join(',') === 'de'",
        "Intl.getCanonicalLocales().length === 0 && Intl.getCanonicalLocales(42).length === 0",
        "typeof Intl === 'object' && Object.prototype.toString.call(Intl) === '[object Intl]'",
        "Intl.getCanonicalLocales('und-u-ks-primary')[0] === 'und-u-ks-level1'",
        "Intl.getCanonicalLocales('und-u-ks-tertiary')[0] === 'und-u-ks-level3'",
        "Intl.getCanonicalLocales('und-u-ms-imperial')[0] === 'und-u-ms-uksystem'",
        "Intl.getCanonicalLocales('und-u-tz-eire')[0] === 'und-u-tz-iedub'",
        "Intl.getCanonicalLocales('und-u-kn-yes')[0] === 'und-u-kn'",
        "Intl.getCanonicalLocales('posix')[0] === 'posix' && new Intl.Locale('posix').maximize().toString() === 'posix'",
        "Intl.getCanonicalLocales('und-Latn-t-und-hani-m0-names')[0] === 'und-Latn-t-und-hani-m0-prprname'",
        "Intl.getCanonicalLocales('und-u-kb-yes,und-u-kc-yes'.split(','))[0] === 'und-u-kb' && Intl.getCanonicalLocales('und-u-kc-yes')[0] === 'und-u-kc'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for source in [
        "Intl.getCanonicalLocales('en_US')",
        "'I'.toLocaleLowerCase(['tr','no_such'])",
        "Intl.getCanonicalLocales('en-u-ca-gregory-u-nu-latn')",
        "Intl.getCanonicalLocales('de-1901-1901')",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::RangeError(_))),
            "{source}"
        );
    }
    for source in [
        "Intl.getCanonicalLocales(null)",
        "Intl.getCanonicalLocales([4])",
        "Intl.getCanonicalLocales([undefined])",
        "''.toLocaleUpperCase([Symbol()])",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

#[test]
fn locale_objects_preserve_canonical_locale_state() {
    for (source, expected) in [
        (
            "new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').toString()",
            "en-Latn-US-1901-u-ca-islamic-civil-kn",
        ),
        (
            "new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').baseName",
            "en-Latn-US-1901",
        ),
        (
            "new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').calendar",
            "islamic-civil",
        ),
        (
            "Intl.getCanonicalLocales('en-u-ca-ethiopic-amete-alem')[0]",
            "en-u-ca-ethioaa",
        ),
    ] {
        assert_eq!(
            evaluate(source),
            Ok(Value::String(expected.into())),
            "{source}"
        );
    }
    for source in [
        "let l=new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true'); l.toString() === 'en-Latn-US-1901-u-ca-islamic-civil-kn' && l.baseName === 'en-Latn-US-1901' && l.language === 'en' && l.script === 'Latn' && l.region === 'US' && l.variants === '1901' && l.calendar === 'islamic-civil' && l.numeric",
        "let l=new Intl.Locale('de',{language:'fr',script:'Latn',region:'CA',variants:'fonipa-1901',calendar:'gregory',collation:'phonebk',hourCycle:'h23',caseFirst:'upper',numeric:true,numberingSystem:'latn'}); l.toString() === 'fr-Latn-CA-1901-fonipa-u-ca-gregory-co-phonebk-hc-h23-kf-upper-kn-nu-latn' && l.caseFirst === 'upper' && l.hourCycle === 'h23' && l.collation === 'phonebk' && l.numberingSystem === 'latn'",
        "new Intl.Locale('zh').maximize().toString() === 'zh-Hans-CN' && new Intl.Locale('zh-Hans-CN').minimize().toString() === 'zh'",
        "Intl.getCanonicalLocales(new Intl.Locale('iw-IL'))[0] === 'he-IL'",
        "new Intl.Locale(new Intl.Locale('fr')).toString() === 'fr'",
        "new Intl.Locale({toString(){return 'de-DE';}}).toString() === 'de-DE' && Intl.getCanonicalLocales([new Intl.Locale('fr'), 'de'])[0] === 'fr'",
        "Intl.Locale.prototype.toString.call(new Intl.Locale('de')) === 'de' && Object.prototype.toString.call(new Intl.Locale('de')) === '[object Intl.Locale]'",
        "new Intl.Locale('en',{numeric:false,firstDayOfWeek:1}).numeric === false && new Intl.Locale('en',{firstDayOfWeek:1}).firstDayOfWeek === 'mon' && new Intl.Locale('en').script === undefined && new Intl.Locale('en').variants === undefined",
        "new Intl.Locale('en-u-ca-buddhist').getCalendars().join() === 'buddhist' && new Intl.Locale('en-u-co-phonebk').getCollations().join() === 'phonebk' && new Intl.Locale('fr').getHourCycles().join() === 'h23' && new Intl.Locale('ar').getNumberingSystems().join() === 'arab'",
        "new Intl.Locale('ar').getTextInfo().direction === 'rtl' && new Intl.Locale('en').getTextInfo().direction === 'ltr' && new Intl.Locale('en').getTimeZones() === undefined && new Intl.Locale('en-US').getTimeZones()[0] === 'America/Adak'",
        "new Intl.Locale('en',{firstDayOfWeek:'wed'}).getWeekInfo().firstDay === 3 && new Intl.Locale('en-US').getWeekInfo().firstDay === 7 && new Intl.Locale('en').getWeekInfo().weekend.join() === '6,7'",
        "Array.isArray(new Intl.Locale('en').getCalendars()) && new Intl.Locale('en').getCollations().includes('emoji') && new Intl.Locale('en').getHourCycles().forEach(() => {}) === undefined",
        "Array(2).length === 2 && Array('a').join() === 'a' && new Array('a','b').join() === 'a,b'",
        "[,,3].forEach(value => value) === undefined && ![1].includes(2) && [1,2].includes(1,-1) === false && [NaN].includes(NaN)",
        "let a=[]; a.length=65536; let p={65535:'inherited'}; Object.setPrototypeOf(p,Array.prototype); Object.setPrototypeOf(a,p); let seen=''; a.forEach(value => {seen=value}); seen === 'inherited'",
        "let a=[0,,]; a.forEach((value,index) => {if(index === 0) a[1]=1}); a[1] === 1",
        "Array.prototype.forEach.call({0:'x',length:1}, value => value) === undefined",
        "let a=[]; a.length=65536; a[65535]=1; a[Symbol('x')]=2; a.forEach(value => value) === undefined",
        "new Intl.Locale('en-GB').getTimeZones().join() === 'Europe/London' && new Intl.Locale('ja-JP').getTimeZones().join() === 'Asia/Tokyo' && new Intl.Locale('zh-TW').getTimeZones().join() === 'Asia/Taipei' && new Intl.Locale('de-DE').getTimeZones().join() === 'Etc/UTC' && new Intl.Locale('en-Arab').getTextInfo().direction === 'rtl'",
        "new Intl.Locale('en',{firstDayOfWeek:'thu'}).getWeekInfo().firstDay === 4 && new Intl.Locale('en',{firstDayOfWeek:'fri'}).getWeekInfo().firstDay === 5 && new Intl.Locale('en',{firstDayOfWeek:'sat'}).getWeekInfo().firstDay === 6 && new Intl.Locale('en',{firstDayOfWeek:'sun'}).getWeekInfo().firstDay === 7",
        "Object.getPrototypeOf(Intl.Locale) === Function.prototype && Object.isExtensible(new Intl.Locale('en')) && Object.isExtensible(1) === false",
        "let log=''; let o={get language(){log+='l';return 'de';},get script(){log+='s';return 'Latn';},get region(){log+='r';return 'DE';},get variants(){log+='v';return '1901';},get calendar(){log+='c';return 'gregory';},get collation(){log+='o';return 'phonebk';},get hourCycle(){log+='h';return 'h23';},get caseFirst(){log+='f';return 'upper';},get numeric(){log+='n';return true;},get numberingSystem(){log+='u';return 'latn';}}; new Intl.Locale('en',o); log === 'lsrvcohfnu'",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.Locale('en')",
        "new Intl.Locale()",
        "new Intl.Locale(1)",
        "new Intl.Locale('en',null)",
        "new Intl.Locale('en',{language:'abcd'})",
        "new Intl.Locale('en',{script:'lat'})",
        "new Intl.Locale('en',{region:'USA'})",
        "new Intl.Locale('en',{variants:'bad'})",
        "new Intl.Locale('en',{variants:''})",
        "new Intl.Locale('en',{variants:'fonipa-fonipa'})",
        "new Intl.Locale('en',{calendar:'no'})",
        "new Intl.Locale('en',{firstDayOfWeek:'mo'})",
        "new Intl.Locale('en',{caseFirst:'invalid'})",
        "Intl.Locale.prototype.toString.call({})",
        "Intl.Locale.prototype.maximize.call({})",
        "Intl.Locale.prototype.getCalendars.call({})",
        "Intl.Locale.prototype.getCollations.call({})",
        "Intl.Locale.prototype.getHourCycles.call({})",
        "Intl.Locale.prototype.getNumberingSystems.call({})",
        "Intl.Locale.prototype.getTextInfo.call({})",
        "Intl.Locale.prototype.getTimeZones.call({})",
        "Intl.Locale.prototype.getWeekInfo.call({})",
        "[1].forEach(1)",
    ] {
        assert!(
            matches!(
                evaluate(source),
                Err(RuntimeError::TypeError(_) | RuntimeError::RangeError(_))
            ),
            "{source}"
        );
    }
    assert_eq!(
        evaluate("Intl.Locale.prototype.toString.call({})"),
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.Locale".into()
        ))
    );
    assert_eq!(
        evaluate("let g=Object.getOwnPropertyDescriptor(Intl.Locale.prototype,'language').get; g.call({})"),
        Err(RuntimeError::TypeError("receiver is not an Intl.Locale".into()))
    );
    assert_eq!(
        evaluate("let g=Object.getOwnPropertyDescriptor(Intl.Locale.prototype,'language').get; g.call(1)"),
        Err(RuntimeError::TypeError("receiver is not an Intl.Locale".into()))
    );
}

#[test]
fn collator_options_and_bound_comparison() {
    for source in [
        "'ä'.localeCompare('z','sv') > 0 && 'ä'.localeCompare('z','de') < 0",
        "'2'.localeCompare('10','en',{numeric:true}) < 0",
        "'é'.localeCompare('e','en',{sensitivity:'base'}) === 0",
        "'a-b'.localeCompare('ab','en',{ignorePunctuation:true}) === 0",
        "let c=new Intl.Collator('en'); c.compare === c.compare && c.compare.name === '' && c.compare.length === 2 && c.compare.prototype === undefined",
        "Object.getOwnPropertyNames(new Intl.Collator().compare).join() === 'length,name'",
        "let c=Intl.Collator('tr'); let f=c.compare; f('ı','I') < 0 && f.call({},'a','b') < 0 && c instanceof Intl.Collator",
        "['AE','Ä'].sort(new Intl.Collator('de',{usage:'search'}).compare).join() === 'AE,Ä'",
        "let r=new Intl.Collator('en-u-kn-kf-upper').resolvedOptions(); r.numeric && r.caseFirst === 'upper' && r.locale === 'en-u-kf-upper-kn'",
        "let r=new Intl.Collator('en-u-kn',{numeric:false}).resolvedOptions(); !r.numeric && r.locale === 'en'",
        "Intl.Collator.supportedLocalesOf(['en','zz','sv']).join(',') === 'en,sv'",
        "Intl.Collator.supportedLocalesOf(['zz','de-AT-u-co-phonebk','en'],{localeMatcher:'best fit'}).join(',') === 'de-AT-u-co-phonebk,en'",
        "new Intl.Collator(['zz','de-AT-u-co-phonebk']).resolvedOptions().locale === 'de-AT-u-co-phonebk'",
        "Object.prototype.toString.call(new Intl.Collator()) === '[object Intl.Collator]'",
        "let n=new Intl.NumberFormat();let d=Intl.DateTimeFormat();n!==d && n instanceof Intl.NumberFormat && d instanceof Intl.DateTimeFormat && Intl.Collator.call(n)!==n",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for option in [
        "usage:'bad'",
        "localeMatcher:'bad'",
        "sensitivity:'bad'",
        "caseFirst:'bad'",
        "collation:'bad_type'",
    ] {
        assert!(
            matches!(
                evaluate(&format!("new Intl.Collator('en',{{{option}}})")),
                Err(RuntimeError::RangeError(_))
            ),
            "{option}"
        );
    }
    assert!(matches!(
        evaluate(r"new Intl.Collator('en',{usage:'\ud800'})"),
        Err(RuntimeError::RangeError(_))
    ));
    for source in [
        "Intl.Collator.prototype.compare",
        "Intl.Collator.prototype.resolvedOptions.call({})",
        "Intl.Collator.prototype.resolvedOptions.call(1)",
        "new Intl.Collator('en',null)",
    ] {
        assert!(
            matches!(evaluate(source), Err(RuntimeError::TypeError(_))),
            "{source}"
        );
    }
}

#[test]
fn number_format_delegates_to_the_host_neutral_decimal_service() {
    for source in [
        "let n=new Intl.NumberFormat('de',{useGrouping:false,minimumFractionDigits:2,maximumFractionDigits:2}); n.format(1007.5) === '1007,50'",
        "let n=new Intl.NumberFormat('th-u-nu-thai',{useGrouping:false,minimumFractionDigits:2,maximumFractionDigits:2}); let r=n.resolvedOptions(); n.format(1007.5) === '๑๐๐๗.๕๐' && r.locale === 'th-u-nu-thai' && r.numberingSystem === 'thai' && r.style === 'decimal' && r.useGrouping === 'false' && r.minimumFractionDigits === 2 && r.maximumFractionDigits === 2",
        "let n=new Intl.NumberFormat('en',{useGrouping:'min2'}); n.format(1000) === '1000' && n.format(10000) === '10,000'",
        "let always=new Intl.NumberFormat('en',{useGrouping:true}).resolvedOptions(); let min2=new Intl.NumberFormat('en',{useGrouping:'min2'}).resolvedOptions(); let never=new Intl.NumberFormat('en',{useGrouping:'false'}).resolvedOptions(); always.useGrouping === 'always' && min2.useGrouping === 'min2' && never.useGrouping === 'false'",
        "let r=new Intl.NumberFormat('en',{minimumFractionDigits:1.9,maximumFractionDigits:2.9}).resolvedOptions(); r.minimumFractionDigits === 1 && r.maximumFractionDigits === 2",
        "let n=new Intl.NumberFormat('en'); n.format === n.format && n.format.name === '' && n.format.length === 1 && n.format.prototype === undefined",
        "let n=Intl.NumberFormat('en'); let f=n.format; f(1000) === '1,000' && f.call({},1000) === '1,000' && n instanceof Intl.NumberFormat",
        "Intl.NumberFormat.supportedLocalesOf(['en','zz','de-AT-u-nu-thai']).join(',') === 'en,de-AT-u-nu-thai'",
        "Object.prototype.toString.call(new Intl.NumberFormat()) === '[object Intl.NumberFormat]'",
        "function F(){} let n=Reflect.construct(Intl.NumberFormat,['de'],F); let f=Object.getOwnPropertyDescriptor(Intl.NumberFormat.prototype,'format').get.call(n); Object.getPrototypeOf(n) === F.prototype && f(5) === '5'",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.NumberFormat.prototype.format",
        "Intl.NumberFormat.prototype.resolvedOptions.call({})",
        "Intl.NumberFormat.prototype.resolvedOptions.call(1)",
        "new Intl.NumberFormat('en',null)",
        "new Intl.NumberFormat('en',{useGrouping:'invalid'})",
        "new Intl.NumberFormat('en',{minimumFractionDigits:4,maximumFractionDigits:2})",
        "new Intl.NumberFormat('en',{minimumFractionDigits:101})",
        "new Intl.NumberFormat('en',{minimumFractionDigits:-1})",
        "new Intl.NumberFormat('en').format(NaN)",
    ] {
        assert!(
            matches!(
                evaluate(source),
                Err(RuntimeError::TypeError(_) | RuntimeError::RangeError(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn list_format_delegates_iterables_and_parts_to_the_host_neutral_service() {
    for source in [
        "let f=new Intl.ListFormat('en',{type:'disjunction',style:'short'}); let r=f.resolvedOptions(); f.format(['A','B','C']) === 'A, B, or C' && r.locale === 'en' && r.type === 'disjunction' && r.style === 'short'",
        "let f=new Intl.ListFormat('en'); f.format() === '' && f.format('foo') === 'f, o, and o'",
        "new Intl.ListFormat('es').format(['España','Suiza','Italia'].values()) === 'España, Suiza e Italia'",
        "let p=new Intl.ListFormat('en').formatToParts(['A','B','C']); p.map(x=>x.type+':'+x.value).join('|') === 'element:A|literal:, |element:B|literal:, and |element:C'",
        "Intl.ListFormat.supportedLocalesOf(['en','zz','es-AR']).join(',') === 'en,es-AR' && Object.prototype.toString.call(new Intl.ListFormat()) === '[object Intl.ListFormat]'",
        "function F(){} let f=Reflect.construct(Intl.ListFormat,['en'],F); Object.getPrototypeOf(f) === F.prototype && Intl.ListFormat.prototype.format.call(f,['A','B']) === 'A and B'",
        "let closed=false; let list={ [Symbol.iterator](){return { next(){return {done:false,value:{toString(){throw 1}}}}, return(){closed=true;return {}}}}}; try { new Intl.ListFormat().format(list) } catch (_) {} closed",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.ListFormat.prototype.format.call({},[])",
        "Intl.ListFormat.prototype.formatToParts.call({},[])",
        "Intl.ListFormat.prototype.resolvedOptions.call({})",
        "new Intl.ListFormat('en',null)",
        "new Intl.ListFormat('en',{type:'invalid'})",
        "new Intl.ListFormat('en',{style:'invalid'})",
        "new Intl.ListFormat().format([1])",
    ] {
        assert!(
            matches!(
                evaluate(source),
                Err(RuntimeError::TypeError(_) | RuntimeError::RangeError(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn plural_rules_delegates_selection_and_resolved_options_to_the_host_service() {
    for source in [
        "let p=new Intl.PluralRules('en'); let r=p.resolvedOptions(); p.select(1) === 'one' && p.select(2) === 'other' && p.select(NaN) === 'other' && r.locale === 'en' && r.type === 'cardinal' && r.notation === 'standard' && r.minimumIntegerDigits === 1 && r.minimumFractionDigits === 0 && r.maximumFractionDigits === 3 && r.pluralCategories.join() === 'one,other'",
        "let p=new Intl.PluralRules('en',{type:'ordinal'}); p.select(1) === 'one' && p.select(2) === 'two' && p.select(3) === 'few' && p.select(11) === 'other'",
        "let r=new Intl.PluralRules('en',{notation:'compact',compactDisplay:'long'}).resolvedOptions(); r.notation === 'compact' && r.compactDisplay === 'long' && new Intl.PluralRules('en',{compactDisplay:'long'}).resolvedOptions().compactDisplay === undefined",
        "Intl.PluralRules.supportedLocalesOf(['en','zz','pl-PL']).join() === 'en,pl-PL' && Object.prototype.toString.call(new Intl.PluralRules()) === '[object Intl.PluralRules]'",
        "function F(){} let p=Reflect.construct(Intl.PluralRules,['en'],F); Object.getPrototypeOf(p) === F.prototype && Intl.PluralRules.prototype.select.call(p,1) === 'one'",
        "new Intl.PluralRules('en').selectRange(102,201) === 'other' && new Intl.PluralRules('en').selectRange(1,1) === 'one'",
        "let log='';let o={get localeMatcher(){log+='l';return 'lookup'},get type(){log+='t';return 'cardinal'},get notation(){log+='n';return 'standard'},get compactDisplay(){log+='c';return 'short'},get minimumIntegerDigits(){log+='i';return 1},get minimumFractionDigits(){log+='f';return 0},get maximumFractionDigits(){log+='F';return 3},get minimumSignificantDigits(){log+='s';return undefined},get maximumSignificantDigits(){log+='S';return undefined},get roundingIncrement(){log+='r';return 1},get roundingMode(){log+='m';return 'halfExpand'},get roundingPriority(){log+='p';return 'auto'},get trailingZeroDisplay(){log+='z';return 'auto'}};new Intl.PluralRules('en',o);log === 'ltncifFsSrmpz'",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.PluralRules('en')",
        "new Intl.PluralRules('en',null)",
        "new Intl.PluralRules('en',{type:'invalid'})",
        "new Intl.PluralRules('en',{notation:'invalid'})",
        "new Intl.PluralRules('en',{roundingMode:'invalid'})",
        "Intl.PluralRules.prototype.select.call({},1)",
        "Intl.PluralRules.prototype.selectRange.call({},1,2)",
        "Intl.PluralRules.prototype.resolvedOptions.call({})",
        "new Intl.PluralRules().selectRange(NaN,1)",
    ] {
        assert!(
            matches!(
                evaluate(source),
                Err(RuntimeError::TypeError(_) | RuntimeError::RangeError(_))
            ),
            "{source}"
        );
    }
    assert_eq!(
        evaluate(
            "['ar','en','fa','fr','gv','ko','sl'].map(locale=>new Intl.PluralRules(locale).resolvedOptions().pluralCategories.join()).join('|')",
        ),
        Ok(Value::String(
            "zero,one,two,few,many,other|one,other|one,other|one,many,other|one,two,few,many,other|other|one,two,few,other".into(),
        ))
    );
}

#[test]
fn segmenter_preserves_utf16_segments_and_exposes_the_segments_protocol() {
    for source in [
        "let s=new Intl.Segmenter('en',{granularity:'word'});let r=s.resolvedOptions();let values=[];for(let item of s.segment('A 2!'))values.push(item.segment+':'+item.isWordLike);values.join('|') === 'A:true| :false|2:true|!:false' && r.locale === 'en' && r.granularity === 'word'",
        "let segments=new Intl.Segmenter().segment('\\ud800A');let first=segments.containing(0);first.segment === '\\ud800' && first.index === 0 && first.input === '\\ud800A' && !Object.hasOwn(first,'isWordLike') && segments.containing(99) === undefined",
        "let iterator=new Intl.Segmenter().segment('ab')[Symbol.iterator]();let a=iterator.next();let b=iterator.next();let done=iterator.next();a.value.segment === 'a' && b.value.segment === 'b' && done.done && Object.prototype.toString.call(iterator) === '[object Segmenter String Iterator]'",
        "Intl.Segmenter.supportedLocalesOf(['en','zz','ja-JP']).join() === 'en,ja-JP' && Object.prototype.toString.call(new Intl.Segmenter()) === '[object Intl.Segmenter]'",
        "function F(){}let s=Reflect.construct(Intl.Segmenter,['en'],F);Object.getPrototypeOf(s) === F.prototype && Intl.Segmenter.prototype.segment.call(s,'A').containing(0).segment === 'A'",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.Segmenter('en')",
        "new Intl.Segmenter('en',null)",
        "new Intl.Segmenter('en',{granularity:'invalid'})",
        "Intl.Segmenter.prototype.segment.call({},'A')",
        "Intl.Segmenter.prototype.resolvedOptions.call({})",
        "let segments=new Intl.Segmenter().segment('A');Object.getPrototypeOf(segments).containing.call({},0)",
        "let iterator=new Intl.Segmenter().segment('A')[Symbol.iterator]();Object.getPrototypeOf(iterator).next.call({})",
    ] {
        assert!(
            matches!(
                evaluate(source),
                Err(RuntimeError::TypeError(_) | RuntimeError::RangeError(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn display_names_uses_host_data_and_preserves_its_ecma402_boundary() {
    for source in [
        "let d=new Intl.DisplayNames('en',{type:'language'});d.of('fr') === 'French' && d.of('cde-ab-abcde') === 'cde-AB-abcde' && d.resolvedOptions().languageDisplay === 'dialect'",
        "let d=new Intl.DisplayNames('en',{type:'region',fallback:'none'});d.of('US') === 'United States' && d.of('ZZ') === undefined && Intl.DisplayNames.supportedLocalesOf(['zz','en']).join() === 'en'",
        "Object.prototype.toString.call(new Intl.DisplayNames('en',{type:'currency'})) === '[object Intl.DisplayNames]' && Intl.DisplayNames.length === 2",
        "function F(){}let d=Reflect.construct(Intl.DisplayNames,['en',{type:'script'}],F);Object.getPrototypeOf(d) === F.prototype && Intl.DisplayNames.prototype.of.call(d,'latn') === 'Latin'",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.DisplayNames('en',{type:'language'})",
        "new Intl.DisplayNames('en',null)",
        "new Intl.DisplayNames('en',{})",
        "new Intl.DisplayNames('en',{type:'unknown'})",
        "Intl.DisplayNames.prototype.of.call({},'en')",
        "Intl.DisplayNames.prototype.resolvedOptions.call({})",
        "new Intl.DisplayNames('en',{type:'region'}).of('U')",
    ] {
        assert!(
            matches!(
                evaluate(source),
                Err(RuntimeError::TypeError(_) | RuntimeError::RangeError(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn relative_time_format_delegates_patterns_and_parts_to_the_host_service() {
    for source in [
        "let r=new Intl.RelativeTimeFormat('en',{numeric:'auto'});r.format(-0,'day') === 'today' && r.format(-1,'day') === 'yesterday' && r.format(2,'hour') === 'in 2 hours'",
        "let r=new Intl.RelativeTimeFormat('pl',{style:'short'});r.format(-2,'years') === '2 lata temu' && r.formatToParts(123456.78,'second').map(p=>p.type+':'+p.value).join('|') === 'literal:za |integer:123|group: |integer:456|decimal:,|fraction:78|literal: sek.'",
        "let r=new Intl.RelativeTimeFormat('en-u-nu-latn',{numberingSystem:'arab'});r.resolvedOptions().locale === 'en' && r.resolvedOptions().numberingSystem === 'arab' && r.format(12,'second').includes('١٢')",
        "Intl.RelativeTimeFormat.supportedLocalesOf(['zz','pl','en']).join() === 'pl,en' && Object.prototype.toString.call(new Intl.RelativeTimeFormat()) === '[object Intl.RelativeTimeFormat]'",
        "function F(){}let r=Reflect.construct(Intl.RelativeTimeFormat,['en'],F);Object.getPrototypeOf(r) === F.prototype && Intl.RelativeTimeFormat.prototype.format.call(r,1,'day') === 'in 1 day'",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.RelativeTimeFormat('en')",
        "new Intl.RelativeTimeFormat('en',null)",
        "new Intl.RelativeTimeFormat('en',{style:'unknown'})",
        "new Intl.RelativeTimeFormat('en',{numberingSystem:'ab'})",
        "Intl.RelativeTimeFormat.prototype.format.call({},1,'day')",
        "Intl.RelativeTimeFormat.prototype.formatToParts.call({},1,'day')",
        "Intl.RelativeTimeFormat.prototype.resolvedOptions.call({})",
        "new Intl.RelativeTimeFormat().format(Infinity,'day')",
        "new Intl.RelativeTimeFormat().format(0,'century')",
    ] {
        assert!(
            matches!(
                evaluate(source),
                Err(RuntimeError::TypeError(_) | RuntimeError::RangeError(_))
            ),
            "{source}"
        );
    }
}

#[test]
fn locale_conversions_are_observable_and_ordered() {
    assert_eq!(evaluate("let log=''; let o={get usage(){log+='u';},get localeMatcher(){log+='l';},get collation(){log+='c';},get numeric(){log+='n';},get caseFirst(){log+='f';},get sensitivity(){log+='s';},get ignorePunctuation(){log+='p';}}; new Intl.Collator('en',o); log").unwrap(), Value::String("ulcnfsp".into()));
    assert_eq!(
        evaluate(
            "let log=''; String.prototype.localeCompare.call({toString(){log+='a';return 'a';}},{toString(){log+='b';return 'b';}},[{toString(){log+='l';return 'en';}}]); log"
        )
        .unwrap(),
        Value::String("abl".into())
    );
}

#[test]
fn collation_extensions_defaults_and_constructor_prototypes() {
    for source in [
        "new Intl.Collator('de-u-co-phonebk').resolvedOptions().collation === 'phonebk'",
        "new Intl.Collator('en',{collation:'emoji'}).resolvedOptions().collation === 'emoji'",
        "new Intl.Collator('en',{collation:'unknown'}).resolvedOptions().collation === 'default'",
        "new Intl.Collator('zh',{collation:'trad'}).resolvedOptions().collation === 'default'",
        "new Intl.Collator('en',{usage:'search'}).resolvedOptions().usage === 'search'",
        "new Intl.Collator('en',{caseFirst:'lower'}).resolvedOptions().caseFirst === 'lower'",
        "function F(){} let c=Reflect.construct(Intl.Collator,[],F); Object.getPrototypeOf(c) === F.prototype",
        "Intl.Collator.prototype.resolvedOptions.call(Reflect.construct(Intl.Collator,[],function(){})).locale === 'en-US'",
        "let p={0:'fr'}; let list=Object.create(p); list.length=1; Intl.getCanonicalLocales(list)[0] === 'fr'",
        "'é'.localeCompare('e','en',{sensitivity:'accent'}) > 0 && 'a'.localeCompare('A','en',{sensitivity:'case'}) < 0",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for tag in [
        "",
        "a",
        "abcd",
        "en-",
        "en-u",
        "en--US",
        "en-abc",
        "en-t-de-1901-1901",
        r"en-\ud800",
    ] {
        assert!(
            matches!(
                evaluate(&format!("Intl.getCanonicalLocales('{tag}')")),
                Err(RuntimeError::RangeError(_))
            ),
            "{tag}"
        );
    }
}

#[test]
fn collator_compare_cycles_survive_collection_and_are_reclaimed() {
    use blueice_bluejs::{HeapConfig, VmConfig};
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 256,
            // The mandatory Intl namespace now owns all ECMA-402 service
            // constructors.  Keep the intentionally tiny nursery and major
            // threshold (which exercise collection) while leaving enough
            // room for that normative global surface.
            max_heap_bytes: 512 * 1024,
        },
        ..Default::default()
    })
    .unwrap();
    let run = |vm: &mut Vm, source| {
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap()
    };
    run(&mut vm, "Intl; globalThis; 0");
    let baseline = vm.heap().stats().managed_bytes;
    let Value::Object(collator) = run(
        &mut vm,
        "globalThis.c=new Intl.Collator('sv'); globalThis.c",
    ) else {
        panic!("expected Collator")
    };
    run(
        &mut vm,
        "globalThis.f=globalThis.c.compare; delete globalThis.c; 0",
    );
    assert!(vm.heap().contains(collator));
    assert_eq!(
        run(
            &mut vm,
            "for(let i=0;i<30;i++){let x={};} globalThis.f('ä','z') > 0"
        ),
        Value::Bool(true)
    );
    run(&mut vm, "delete globalThis.f; 0");
    assert!(!vm.heap().contains(collator));
    assert_eq!(vm.heap().stats().managed_bytes, baseline);
}

#[test]
fn number_format_bound_format_cycles_survive_collection_and_are_reclaimed() {
    use blueice_bluejs::{HeapConfig, VmConfig};
    let mut vm = Vm::new(VmConfig {
        heap: HeapConfig {
            nursery_capacity: 1,
            major_threshold_bytes: 256,
            // See the corresponding Collator collection test above.
            max_heap_bytes: 512 * 1024,
        },
        ..Default::default()
    })
    .unwrap();
    let run = |vm: &mut Vm, source| {
        vm.execute(&compile(&parse(source).unwrap()).unwrap())
            .unwrap()
    };
    run(&mut vm, "Intl; globalThis; 0");
    let baseline = vm.heap().stats().managed_bytes;
    let Value::Object(formatter) = run(
        &mut vm,
        "globalThis.n=new Intl.NumberFormat('de',{minimumFractionDigits:2}); globalThis.n",
    ) else {
        panic!("expected NumberFormat")
    };
    run(
        &mut vm,
        "globalThis.f=globalThis.n.format; delete globalThis.n; 0",
    );
    assert!(vm.heap().contains(formatter));
    assert_eq!(
        run(
            &mut vm,
            "for(let i=0;i<30;i++){let x={};} globalThis.f(7) === '7,00'"
        ),
        Value::Bool(true)
    );
    run(&mut vm, "delete globalThis.f; 0");
    assert!(!vm.heap().contains(formatter));
    assert_eq!(vm.heap().stats().managed_bytes, baseline);
}

#[test]
fn intl_and_error_bootstrap_failures_release_partial_roots() {
    use blueice_bluejs::{HeapConfig, HeapError, VmConfig};
    let warm = compile(&parse("String; Object; 0").unwrap()).unwrap();
    for source in ["Intl", "Error", "TypeError"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        for ceiling in (64000..125000).step_by(307) {
            let mut vm = Vm::new(VmConfig {
                heap: HeapConfig {
                    nursery_capacity: 1,
                    major_threshold_bytes: 256,
                    max_heap_bytes: ceiling,
                },
                ..Default::default()
            })
            .unwrap();
            if vm.execute(&warm).is_err() {
                continue;
            }
            let mut previous = None;
            for _ in 0..2 {
                match vm.execute(&code) {
                    Ok(_) => break,
                    Err(RuntimeError::Heap(HeapError::HeapLimitExceeded { .. })) => {
                        let bytes = vm.heap().stats().managed_bytes;
                        if let Some(previous) = previous {
                            assert_eq!(bytes, previous, "{source} at {ceiling}");
                        }
                        previous = Some(bytes);
                    }
                    error => panic!("{error:?}"),
                }
            }
        }
    }
}
