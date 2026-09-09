// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluejs::{RuntimeError, Value, Vm, compile, parse};

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
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for source in [
        "Intl.getCanonicalLocales('en_US')",
        "'I'.toLocaleLowerCase(['tr','no_such'])",
        "Intl.getCanonicalLocales('en-u-ca-gregory-u-nu-latn')",
        "Intl.getCanonicalLocales('de-1901-1901')",
    ] {
        assert!(matches!(evaluate(source), Err(RuntimeError::RangeError(_))), "{source}");
    }
    for source in ["Intl.getCanonicalLocales(null)", "Intl.getCanonicalLocales([4])", "Intl.getCanonicalLocales([undefined])", "''.toLocaleUpperCase([Symbol()])"] {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
    }
}

#[test]
fn locale_objects_preserve_canonical_locale_state() {
    for (source, expected) in [
        ("new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').toString()", "en-Latn-US-1901-u-ca-islamic-civil-kn"),
        ("new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').baseName", "en-Latn-US-1901"),
        ("new Intl.Locale('EN-latn-us-1901-u-ca-islamicc-kn-true').calendar", "islamic-civil"),
        ("Intl.getCanonicalLocales('en-u-ca-ethiopic-amete-alem')[0]", "en-u-ca-ethioaa"),
    ] {
        assert_eq!(evaluate(source), Ok(Value::String(expected.into())), "{source}");
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
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_) | RuntimeError::RangeError(_))), "{source}");
    }
    assert_eq!(evaluate("Intl.Locale.prototype.toString.call({})"), Err(RuntimeError::TypeError("receiver is not an Intl.Locale".into())));
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
        "let c=Intl.Collator('tr'); let f=c.compare; f('ı','I') < 0 && f.call({},'a','b') < 0 && c instanceof Intl.Collator",
        "let r=new Intl.Collator('en-u-kn-kf-upper').resolvedOptions(); r.numeric && r.caseFirst === 'upper' && r.locale === 'en-u-kf-upper-kn'",
        "let r=new Intl.Collator('en-u-kn',{numeric:false}).resolvedOptions(); !r.numeric && r.locale === 'en'",
        "Intl.Collator.supportedLocalesOf(['en','zz','sv']).join(',') === 'en,sv'",
        "Object.prototype.toString.call(new Intl.Collator()) === '[object Intl.Collator]'",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for option in ["usage:'bad'", "localeMatcher:'bad'", "sensitivity:'bad'", "caseFirst:'bad'", "collation:'bad_type'"] {
        assert!(matches!(evaluate(&format!("new Intl.Collator('en',{{{option}}})")), Err(RuntimeError::RangeError(_))), "{option}");
    }
    assert!(matches!(evaluate(r"new Intl.Collator('en',{usage:'\ud800'})"), Err(RuntimeError::RangeError(_))));
    for source in
        ["Intl.Collator.prototype.compare", "Intl.Collator.prototype.resolvedOptions.call({})", "Intl.Collator.prototype.resolvedOptions.call(1)", "new Intl.Collator('en',null)"]
    {
        assert!(matches!(evaluate(source), Err(RuntimeError::TypeError(_))), "{source}");
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
    for tag in ["", "a", "abcd", "en-", "en-u", "en--US", "en-abc", "en-t-de-1901-1901", r"en-\ud800"] {
        assert!(matches!(evaluate(&format!("Intl.getCanonicalLocales('{tag}')")), Err(RuntimeError::RangeError(_))), "{tag}");
    }
}

#[test]
fn collator_compare_cycles_survive_collection_and_are_reclaimed() {
    use blueice_bluejs::{HeapConfig, VmConfig};
    let mut vm = Vm::new(VmConfig { heap: HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: 256 * 1024 }, ..Default::default() }).unwrap();
    let run = |vm: &mut Vm, source| vm.execute(&compile(&parse(source).unwrap()).unwrap()).unwrap();
    run(&mut vm, "Intl; globalThis; 0");
    let baseline = vm.heap().stats().managed_bytes;
    let Value::Object(collator) = run(&mut vm, "globalThis.c=new Intl.Collator('sv'); globalThis.c") else { panic!("expected Collator") };
    run(&mut vm, "globalThis.f=globalThis.c.compare; delete globalThis.c; 0");
    assert!(vm.heap().contains(collator));
    assert_eq!(run(&mut vm, "for(let i=0;i<30;i++){let x={};} globalThis.f('ä','z') > 0"), Value::Bool(true));
    run(&mut vm, "delete globalThis.f; 0");
    assert!(!vm.heap().contains(collator));
    assert_eq!(vm.heap().stats().managed_bytes, baseline);
}

#[test]
fn intl_and_error_bootstrap_failures_release_partial_roots() {
    use blueice_bluejs::{HeapConfig, HeapError, VmConfig};
    let warm = compile(&parse("String; Object; 0").unwrap()).unwrap();
    for source in ["Intl", "Error", "TypeError"] {
        let code = compile(&parse(source).unwrap()).unwrap();
        for ceiling in (64000..125000).step_by(307) {
            let mut vm = Vm::new(VmConfig { heap: HeapConfig { nursery_capacity: 1, major_threshold_bytes: 256, max_heap_bytes: ceiling }, ..Default::default() }).unwrap();
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
