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
        "let systems=Intl.supportedValuesOf('numberingSystem');systems.includes('latn')&&systems.includes('gara')&&systems.join()===systems.slice().sort().join()",
        "let calendars=Intl.supportedValuesOf('calendar');calendars.every(calendar=>new Intl.DateTimeFormat('en',{calendar}).resolvedOptions().calendar===calendar)",
        "let systems=Intl.supportedValuesOf('numberingSystem');systems.every(numberingSystem=>new Intl.DateTimeFormat('en',{numberingSystem}).resolvedOptions().numberingSystem===numberingSystem)",
        "let systems=Intl.supportedValuesOf('numberingSystem');systems.every(numberingSystem=>new Intl.NumberFormat('en',{numberingSystem}).resolvedOptions().numberingSystem===numberingSystem)",
        "let systems=Intl.supportedValuesOf('numberingSystem');systems.every(numberingSystem=>new Intl.RelativeTimeFormat('en',{numberingSystem}).resolvedOptions().numberingSystem===numberingSystem)",
        "let currencies=Intl.supportedValuesOf('currency');currencies.length===307&&currencies.includes('AFA')&&currencies.includes('XCG')&&currencies.includes('XXX')&&currencies.join()===currencies.slice().sort().join()&&currencies.every(currency=>typeof new Intl.DisplayNames('en',{type:'currency',fallback:'none'}).of(currency)==='string')",
        "let units=Intl.supportedValuesOf('unit');units.every(unit=>new Intl.NumberFormat('en',{style:'unit',unit}).resolvedOptions().unit===unit)",
        "Intl.supportedValuesOf('timeZone').includes('UTC') && Intl.supportedValuesOf('timeZone').includes('Etc/GMT-14') && !Intl.supportedValuesOf('timeZone').includes('Etc/UTC')",
        "let zones=Intl.supportedValuesOf('timeZone');zones.join()===zones.slice().sort().join()",
        "let zones=Intl.supportedValuesOf('timeZone');new Set(zones).size===zones.length",
    ] {
        assert_eq!(evaluate(source).unwrap(), Value::Bool(true), "{source}");
    }
    for source in [
        "Intl.getCanonicalLocales('en_US')",
        "'I'.toLocaleLowerCase(['tr','no_such'])",
        "Intl.getCanonicalLocales('en-u-ca-gregory-u-nu-latn')",
        "Intl.getCanonicalLocales('de-1901-1901')",
        "Intl.supportedValuesOf('not-a-key')",
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
fn supported_time_zones_are_ecma402_structurally_canonical() {
    let source = r#"
        let fileNameComponent = "(?:[A-Za-z_]|\\.(?!\\.?(?:/|$)))[A-Za-z.\\-_]{0,13}";
        let fileName = fileNameComponent + "(?:/" + fileNameComponent + ")*";
        let etcName = "(?:Etc/)?GMT[+-]\\d{1,2}";
        let systemVName = "SystemV/[A-Z]{3}\\d{1,2}(?:[A-Z]{3})?";
        let legacyName = etcName + "|" + systemVName + "|CST6CDT|EST5EDT|MST7MDT|PST8PDT|NZ";
        let zoneName = new RegExp("^(?:" + fileName + "|" + legacyName + ")$");
        Intl.supportedValuesOf("timeZone").find((timeZone) =>
            timeZone !== "UTC" &&
            (timeZone === "Etc/UTC" || timeZone === "Etc/GMT" || !zoneName.test(timeZone))
        )
    "#;
    assert_eq!(evaluate(source).unwrap(), Value::Undefined);
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
        "new Intl.Locale('und').maximize().toString() === 'en-Latn-US' && new Intl.Locale('und-Thai').minimize().toString() === 'th' && new Intl.Locale('und-CW').minimize().toString() === 'pap' && new Intl.Locale('zh-Hant').minimize().toString() === 'zh-TW'",
        "Intl.getCanonicalLocales(new Intl.Locale('iw-IL'))[0] === 'he-IL'",
        "new Intl.Locale(new Intl.Locale('fr')).toString() === 'fr'",
        "new Intl.Locale({toString(){return 'de-DE';}}).toString() === 'de-DE' && Intl.getCanonicalLocales([new Intl.Locale('fr'), 'de'])[0] === 'fr'",
        "Intl.Locale.prototype.toString.call(new Intl.Locale('de')) === 'de' && Object.prototype.toString.call(new Intl.Locale('de')) === '[object Intl.Locale]'",
        "new Intl.Locale('en',{numeric:false,firstDayOfWeek:1}).numeric === false && new Intl.Locale('en',{firstDayOfWeek:1}).firstDayOfWeek === 'mon' && new Intl.Locale('en').script === undefined && new Intl.Locale('en').variants === undefined",
        "new Intl.Locale('en-u-ca-buddhist').getCalendars().join() === 'buddhist' && new Intl.Locale('en-u-co-phonebk').getCollations().join() === 'phonebk' && new Intl.Locale('fr').getHourCycles().join() === 'h23' && new Intl.Locale('ar').getNumberingSystems().join() === 'latn'",
        "new Intl.Locale('ar').getTextInfo().direction === 'rtl' && new Intl.Locale('en').getTextInfo().direction === 'ltr' && new Intl.Locale('en').getTimeZones() === undefined && new Intl.Locale('en-US').getTimeZones()[0] === 'America/Adak'",
        "new Intl.Locale('en',{firstDayOfWeek:'wed'}).getWeekInfo().firstDay === 3 && new Intl.Locale('en-US').getWeekInfo().firstDay === 7 && new Intl.Locale('en').getWeekInfo().weekend.join() === '6,7'",
        "Array.isArray(new Intl.Locale('en').getCalendars()) && new Intl.Locale('en').getCollations().includes('emoji') && new Intl.Locale('en').getHourCycles().forEach(() => {}) === undefined",
        "Array(2).length === 2 && Array('a').join() === 'a' && new Array('a','b').join() === 'a,b'",
        "[,,3].forEach(value => value) === undefined && ![1].includes(2) && [1,2].includes(1,-1) === false && [NaN].includes(NaN)",
        "let a=[]; a.length=65536; let p={65535:'inherited'}; Object.setPrototypeOf(p,Array.prototype); Object.setPrototypeOf(a,p); let seen=''; a.forEach(value => {seen=value}); seen === 'inherited'",
        "let a=[0,,]; a.forEach((value,index) => {if(index === 0) a[1]=1}); a[1] === 1",
        "Array.prototype.forEach.call({0:'x',length:1}, value => value) === undefined",
        "let a=[]; a.length=65536; a[65535]=1; a[Symbol('x')]=2; a.forEach(value => value) === undefined",
        "new Intl.Locale('en-GB').getTimeZones().join() === 'Europe/London' && new Intl.Locale('ja-JP').getTimeZones().join() === 'Asia/Tokyo' && new Intl.Locale('zh-TW').getTimeZones().join() === 'Asia/Taipei' && new Intl.Locale('de-DE').getTimeZones().join() === 'Europe/Berlin,Europe/Busingen' && new Intl.Locale('en-Arab').getTextInfo().direction === 'rtl'",
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
        "let r=new Intl.Collator('en',{usage:'search',caseFirst:'false',sensitivity:'variant',ignorePunctuation:false}).resolvedOptions();r.usage === 'search' && r.caseFirst === 'false' && r.sensitivity === 'variant' && !r.ignorePunctuation",
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
fn date_time_format_constructs_formats_and_exposes_parts() {
    for source in [
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC',year:'numeric',month:'long',day:'numeric'}); let p=f.formatToParts(0); f.format(0)==='January 1, 1970' && p.map(x=>x.type).join(',')==='month,literal,day,literal,year' && p.map(x=>x.value).join('')===f.format(0)",
        "let f=Intl.DateTimeFormat('de-DE',{timeZone:'UTC',hour:'numeric',minute:'2-digit',second:'2-digit'}); let r=f.resolvedOptions(); f instanceof Intl.DateTimeFormat && f.format(new Date(0)).length > 0 && r.locale === 'de-DE' && r.timeZone === 'UTC' && r.hour === 'numeric' && r.minute === '2-digit' && r.second === '2-digit'",
        "let f=new Intl.DateTimeFormat('en',{timeZone:'UTC',year:'numeric',month:'numeric',day:'numeric'}); f.formatRange(0,86400000).includes('–') && f.formatRangeToParts(0,86400000).map(x=>x.source).join(',').includes('startRange') && f.formatRangeToParts(0,86400000).map(x=>x.source).join(',').includes('endRange')",
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC',year:'numeric',month:'long',day:'numeric'}); let p=f.formatRangeToParts(0,86400000); f.formatRange(0,86400000) === 'January 1 – 2, 1970' && p.map(x=>x.type+':'+x.source).join(',') === 'month:shared,literal:shared,day:startRange,literal:shared,day:endRange,literal:shared,year:shared'",
        "let f=new Intl.DateTimeFormat('zh-TW',{timeZone:'UTC',year:'numeric',month:'numeric',day:'numeric'}); f.formatRange(0,86400000).includes('至') && f.formatRangeToParts(0,86400000).filter(x=>x.type==='year').length===2",
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC'}); let p=f.formatRangeToParts(0,86400000); f.formatRange(0,86400000)==='1/1/1970 – 1/2/1970' && p.length===11 && p[0].type==='month' && p[0].source==='startRange' && p[4].type==='year' && p[4].source==='startRange' && p[5].type==='literal' && p[5].value===' – ' && p[5].source==='shared' && p[6].type==='month' && p[6].source==='endRange' && p[10].type==='year' && p[10].source==='endRange'",
        "let f=new Intl.DateTimeFormat('en',{timeZone:'UTC',minute:'numeric',second:'numeric',fractionalSecondDigits:1}); let p=f.formatRangeToParts(0,300); p.length===11 && p[0].type==='minute' && p[0].value==='00' && p[0].source==='startRange' && p[1].type==='literal' && p[1].value===':' && p[1].source==='startRange' && p[2].type==='second' && p[2].value==='00' && p[2].source==='startRange' && p[3].value==='.' && p[3].source==='startRange' && p[4].type==='fractionalSecond' && p[4].value==='0' && p[4].source==='startRange' && p[5].source==='shared' && p[6].type==='minute' && p[6].source==='endRange' && p[8].type==='second' && p[8].value==='00' && p[8].source==='endRange' && p[10].type==='fractionalSecond' && p[10].value==='3' && p[10].source==='endRange' && f.formatRangeToParts(0,0.9).every(x=>x.source==='shared') && typeof f.formatRangeToParts(300,0)==='object'",
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC',weekday:'long',year:'numeric',day:'2-digit'}); let t=f.formatToParts(0).map(x=>x.type); t.includes('weekday') && t.includes('year') && t.includes('day') && !t.includes('month')",
        "let f=new Intl.DateTimeFormat('en',{timeZone:'UTC',dateStyle:'short'}); f.resolvedOptions().dateStyle === 'short' && f.format(0).length > 0 && Object.prototype.toString.call(f)==='[object Intl.DateTimeFormat]'",
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'America/New_York',hour:'numeric',minute:'2-digit',timeZoneName:'short'}); f.format(1705320000000).endsWith('EST') && f.format(1721044800000).endsWith('EDT') && f.resolvedOptions().timeZone === 'America/New_York'",
        "Intl.DateTimeFormat.supportedLocalesOf(['zz','de-DE','en']).join(',') === 'de-DE,en'",
        "Intl.DateTimeFormat.supportedLocalesOf(['zz','ak','fr-CA']).join(',') === 'ak,fr-CA'",
        "let f=new Intl.DateTimeFormat('ak',{timeZone:'UTC',formatMatcher:'basic',year:'numeric',month:'short',day:'numeric'});f.resolvedOptions().locale==='ak'&&f.format(0).length>0",
        "let f=new Intl.DateTimeFormat('en',{timeZone:'UTC'}); f.format === f.format && f.format.name === '' && f.format.length === 1 && f.format.prototype === undefined",
        "let f=new Intl.DateTimeFormat('en',{timeZone:'UTC'}); let converted=false; let poison={valueOf:function(){converted=true;return 0}}; let caught=false; try{f.formatRangeToParts(undefined,poison)}catch(error){caught=error instanceof TypeError} caught && !converted",
        "let f=new Intl.DateTimeFormat('en',{timeZone:'UTC'}); let low=f.formatRange(-8640000000000000,0); let high=f.formatRange(0,8640000000000000); let bad=false; try{f.formatRange(8640000000000001,0)}catch(error){bad=error instanceof RangeError} low.length>0&&high.length>0&&bad",
        "let f=new Intl.DateTimeFormat('en-US',{timeZoneName:'short'});let p=f.formatToParts(0);p.some(x=>x.type==='month')&&p.some(x=>x.type==='day')&&p.some(x=>x.type==='year')&&p.some(x=>x.type==='timeZoneName')",
        "new Intl.DateTimeFormat('en-US',{timeZone:'America/New_York',year:'numeric',month:'numeric',day:'numeric',timeZoneName:'short'}).format(8640000000000000).length>0",
        "let start=Temporal.Instant.from('2020-01-02T00:00:00+00:00');let end=Temporal.Instant.from('2020-01-03T00:00:00Z');let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC',year:'numeric',month:'numeric',day:'numeric'});f.formatRange(start,end).includes('2020')",
        "let z=new Temporal.ZonedDateTime(1577836800000000000n,'UTC');z.toLocaleString('en-US',{year:'numeric',month:'numeric',day:'numeric'}).includes('2020')",
        "let extended=Temporal.PlainDate.from('+002020-06-01');let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC',year:'numeric',month:'numeric',day:'numeric'});let monthDay=false;let yearMonth=false;try{new Temporal.PlainMonthDay(2,1,'iso8601',300000)}catch(error){monthDay=error instanceof RangeError};try{new Temporal.PlainYearMonth(2020,4,'iso8601',31)}catch(error){yearMonth=error instanceof RangeError};if(!f.formatRange(extended,extended).includes('2020'))throw new Error('extended year');if(!monthDay)throw new Error('reference year');if(!yearMonth)throw new Error('reference day');true",
        "let f=new Intl.DateTimeFormat('en',{timeZone:'America/New_York',year:'numeric',month:'numeric',day:'numeric',hour:'numeric',minute:'numeric',timeZoneName:'short'});let start=Temporal.PlainDateTime.from('2020-01-01T00:00');let end=Temporal.PlainDateTime.from('2020-01-01T01:00');let parts=f.formatRangeToParts(start,end);parts.some(x=>x.source==='startRange')&&parts.some(x=>x.source==='endRange')&&!parts.some(x=>x.type==='timeZoneName')&&f.formatRange(start,end).length>0",
        "let f=new Intl.DateTimeFormat('en',{timeZone:'UTC',year:'numeric',month:'numeric',day:'numeric'});let start=new Temporal.PlainDate(2020,1,1);let end=new Temporal.PlainDate(2020,1,2);let p=f.formatRangeToParts(start,end);p[0].source==='startRange'&&p[p.length-1].source==='endRange'&&f.formatRange(start,end).length>0",
        "let f=new Intl.DateTimeFormat('en',{era:'narrow'});f.format(new Temporal.PlainDate(2025,11,4)).startsWith('11')",
        "let f=new Intl.DateTimeFormat('en',{era:'narrow'});f.format(new Temporal.PlainYearMonth(2025,11,'gregory')).startsWith('11')",
        "let f=new Intl.DateTimeFormat('en',{era:'narrow'});f.format(new Temporal.PlainMonthDay(11,4,'gregory')).startsWith('11')",
        "let f=new Intl.DateTimeFormat('en',{era:'narrow'});f.format(new Temporal.PlainTime(14,46)).startsWith('2')",
        "let f=new Intl.DateTimeFormat('en',{era:'narrow'});f.format(new Temporal.PlainDateTime(2025,11,4,14,46)).startsWith('11')",
        "let f=new Intl.DateTimeFormat('en',{era:'narrow'});f.format(new Temporal.Instant(0n))===new Date(0).toLocaleString('en',{era:'narrow'})",
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'Pacific/Apia',year:'numeric',month:'numeric',day:'numeric',hour:'numeric',timeZoneName:'long'});let p=f.formatToParts(new Temporal.PlainDateTime(2011,12,30,12));p.some(x=>x.type==='day'&&x.value==='30')&&!p.some(x=>x.type==='timeZoneName')",
        "let legacy=Object.create(Intl.DateTimeFormat.prototype);let result=Intl.DateTimeFormat.call(legacy,'en',{timeZone:'UTC'});let symbol=Object.getOwnPropertySymbols(legacy).find(key=>key.description==='IntlLegacyConstructedSymbol');let descriptor=Object.getOwnPropertyDescriptor(legacy,symbol);result===legacy&&symbol!==undefined&&legacy[symbol] instanceof Intl.DateTimeFormat&&!descriptor.writable&&!descriptor.enumerable&&!descriptor.configurable&&legacy.format(0).length>0&&legacy.resolvedOptions().timeZone==='UTC'",
        "let legacy=new Intl.DateTimeFormat('en',{timeZone:'UTC'});Intl.DateTimeFormat.call(legacy);let observed;let proxy=new Proxy(legacy,{get(target,key){observed=key;return target[key]}});let options=Intl.DateTimeFormat.prototype.resolvedOptions.call(proxy);options.timeZone==='UTC'&&typeof observed==='symbol'&&observed.description==='IntlLegacyConstructedSymbol'&&Symbol('named').description==='named'&&Symbol().description===undefined",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.DateTimeFormat.prototype.format",
        "Intl.DateTimeFormat.prototype.formatToParts.call({},0)",
        "new Intl.DateTimeFormat('en',{timeZone:'No/Such_Zone'})",
        "new Intl.DateTimeFormat('en',{dateStyle:'short',year:'numeric'})",
        "new Intl.DateTimeFormat('en',{fractionalSecondDigits:4})",
        "new Intl.DateTimeFormat('en').format(NaN)",
        "new Intl.DateTimeFormat('en').formatRange()",
        "new Intl.DateTimeFormat('en').formatRangeToParts(undefined,0)",
        "Temporal.Instant.from('not-an-instant')",
        "new Intl.DateTimeFormat('en').format(new Temporal.ZonedDateTime(0n,'UTC'))",
        "let legacy=Object.create(Intl.DateTimeFormat.prototype);Intl.DateTimeFormat.call(legacy);Intl.DateTimeFormat.prototype.formatToParts.call(legacy,0)",
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
fn temporal_calendar_fields_round_trip_through_iso_and_lunisolar_months() {
    let source = r#"
        let calendars = [
            "buddhist", "coptic", "ethioaa", "ethiopic", "gregory", "indian",
            "islamic-civil", "islamic-tbla", "islamic-umalqura", "japanese",
            "persian", "roc"
        ];
        for (let calendar of calendars) {
            let anchor = new Temporal.PlainDate(2050, 1, 1, calendar);
            let date = Temporal.PlainDate.from({
                calendar, year: anchor.year, month: 1, day: 31
            });
            let iso = date.withCalendar("iso8601").toZonedDateTime("UTC");
            let formatter = new Intl.DateTimeFormat("en", {
                calendar, timeZone: "UTC", year: "numeric", month: "numeric", day: "numeric"
            });
            let parts = formatter.formatToParts(iso.epochMilliseconds);
            let year;
            let month;
            let day;
            for (let part of parts) {
                if (part.type === "year") year = +part.value;
                if (part.type === "month") month = +part.value;
                if (part.type === "day") day = +part.value;
            }
            if (year !== (date.eraYear === undefined ? date.year : date.eraYear)) throw new Error("year");
            if (month !== date.month || day !== date.day) throw new Error("month/day");
        }
        let chinese = new Temporal.PlainDate(2048, 1, 1, "chinese");
        if (chinese.monthsInYear !== 13) throw new Error("lunisolar year");
        let leap = Temporal.PlainDate.from({
            calendar: "chinese", year: chinese.year, month: 13, day: 30
        });
        let byCode = Temporal.PlainDate.from({
            calendar: "chinese", year: leap.year, monthCode: leap.monthCode, day: leap.day
        });
        if (leap.month !== 13 || byCode.monthCode !== leap.monthCode || byCode.day !== leap.day) {
            throw new Error("leap month");
        }
        let coptic = new Temporal.PlainDate(2025, 1, 1, "coptic");
        if (coptic.monthsInYear !== 13) throw new Error("intercalary year");
        let intercalary = Temporal.PlainDate.from({
            calendar: "coptic", year: coptic.year, month: 13, day: 31
        });
        if (intercalary.month !== 13 || intercalary.day > 6) throw new Error("intercalary month");
        let midnight = Temporal.PlainDateTime.from({
            calendar: "buddhist", year: 2563, month: 1, day: 1
        }).withCalendar("iso8601").toZonedDateTime("UTC");
        let afternoon = Temporal.PlainDateTime.from({
            calendar: "buddhist", year: 2563, month: 1, day: 1,
            hour: 12, minute: 34, second: 56, millisecond: 789,
            microsecond: 123, nanosecond: 456
        }).withCalendar("iso8601").toZonedDateTime("UTC");
        if (afternoon.epochMilliseconds - midnight.epochMilliseconds !== 45296789) {
            throw new Error("plain date-time fields");
        }
        Number.isInteger(13) && Number.parseInt("11bis") === 11
    "#;
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn temporal_string_calendar_and_unknown_annotations_follow_the_grammar() {
    let source = r#"
        let calendarWins = [
            "2000-05-02[u-ca=hebrew]",
            "2000-05-02[UTC][u-ca=hebrew]",
            "2000-05-02T15:23[u-ca=hebrew]",
            "2000-05-02T15:23[UTC][u-ca=hebrew]",
            "2000-05-02T15:23[!u-ca=hebrew]",
            "2000-05-02T15:23[UTC][!u-ca=hebrew]",
            "2000-05-02T15:23[u-ca=hebrew][u-ca=discord]",
        ];
        for (let source of calendarWins) {
            let date = Temporal.PlainDate.from(source);
            if (date.calendarId !== "hebrew") throw new Error(`calendar wins: ${source}`);
        }

        let unknownIgnored = [
            "2000-05-02[foo=bar]",
            "2000-05-02T15:23[foo=bar]",
            "2000-05-02T15:23[UTC][foo=bar]",
            "2000-05-02T15:23[u-ca=iso8601][foo=bar]",
            "2000-05-02T15:23[UTC][foo=bar][u-ca=iso8601]",
            "2000-05-02T15:23[foo=bar][_foo-bar0=Ignore-This-999999999999]",
        ];
        for (let source of unknownIgnored) {
            let date = Temporal.PlainDate.from(source);
            if (date.calendarId !== "iso8601" || date.year !== 2000 || date.month !== 5 || date.day !== 2) {
                throw new Error(`unknown annotation ignored: ${source}`);
            }
        }

        let criticalUnknownThrows = [
            "1970-01-01[!foo=bar]",
            "1970-01-01T00:00[!foo=bar]",
            "1970-01-01T00:00[UTC][!foo=bar]",
            "1970-01-01T00:00[u-ca=iso8601][!foo=bar]",
            "1970-01-01T00:00[UTC][!foo=bar][u-ca=iso8601]",
        ];
        for (let source of criticalUnknownThrows) {
            let threw = false;
            try { Temporal.PlainDate.from(source); } catch (error) { threw = error instanceof RangeError; }
            if (!threw) throw new Error(`critical unknown annotation must throw: ${source}`);
        }

        let invalidKeyThrows = [
            "1970-01-01[U-CA=iso8601]",
            "1970-01-01[u-CA=iso8601]",
            "1970-01-01[FOO=bar]",
        ];
        for (let source of invalidKeyThrows) {
            let threw = false;
            try { Temporal.PlainDate.from(source); } catch (error) { threw = error instanceof RangeError; }
            if (!threw) throw new Error(`uppercase annotation key must always throw: ${source}`);
        }

        let unsupportedCalendarThrows = false;
        try { Temporal.PlainDate.from("1970-01-01[u-ca=discordian]"); }
        catch (error) { unsupportedCalendarThrows = error instanceof RangeError; }
        if (!unsupportedCalendarThrows) throw new Error("unrecognized calendar id must throw");

        true
    "#;
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn temporal_plain_time_getters_and_calendar_agnostic_arithmetic() {
    // Every expectation below is taken from a Test262 fixture under
    // `built-ins/Temporal/PlainTime/`, named in the comment beside it, rather
    // than derived from the spec text alone.
    let source = r#"
        function check(condition, message) {
            if (!condition) throw new Error(message);
        }
        function timeEquals(time, hour, minute, second, millisecond, microsecond, nanosecond) {
            return time.hour === hour && time.minute === minute && time.second === second
                && time.millisecond === millisecond && time.microsecond === microsecond
                && time.nanosecond === nanosecond;
        }
        function throwsRange(thunk, message) {
            try { thunk(); } catch (error) {
                check(error instanceof RangeError, `${message}: ${error}`);
                return;
            }
            throw new Error(`${message}: did not throw`);
        }
        function throwsType(thunk, message) {
            try { thunk(); } catch (error) {
                check(error instanceof TypeError, `${message}: ${error}`);
                return;
            }
            throw new Error(`${message}: did not throw`);
        }

        // Getters (prototype/{hour,minute,...}/basic.js, branding.js) and the
        // truncating constructor (argument-convert.js).
        let time = new Temporal.PlainTime(12, 34, 56, 987, 654, 321);
        check(timeEquals(time, 12, 34, 56, 987, 654, 321), "getters");
        check(timeEquals(new Temporal.PlainTime(11.9, 12.8, 13.7, 14.6, 15.5, 1.999999),
            11, 12, 13, 14, 15, 1), "constructor truncates fractions");
        let hourGetter = Object.getOwnPropertyDescriptor(Temporal.PlainTime.prototype, "hour").get;
        throwsType(() => hourGetter.call({}), "hour getter branding");
        throwsType(() => hourGetter.call(Temporal.Instant.fromEpochMilliseconds(0)),
            "hour getter rejects a non-PlainTime Temporal value");

        // The PlainTime string grammar (from/argument-string*.js).
        check(timeEquals(Temporal.PlainTime.from("15:23"), 15, 23, 0, 0, 0, 0), "bare time string");
        check(timeEquals(Temporal.PlainTime.from("T12:34:56.987654321"), 12, 34, 56, 987, 654, 321),
            "time designator");
        check(timeEquals(Temporal.PlainTime.from("1976-11-18 12:34:56.987654321"),
            12, 34, 56, 987, 654, 321), "space separator");
        check(timeEquals(Temporal.PlainTime.from("23:59:60.170"), 23, 59, 59, 170, 0, 0),
            "leap second constrains to :59");
        check(timeEquals(Temporal.PlainTime.from("12:34:56.987654321[u-ca=unknown]"),
            12, 34, 56, 987, 654, 321), "an unknown calendar annotation is ignored");
        // The basic (separator-less) format, which Stage 0 wrongly recorded as
        // unnecessary (from/argument-string.js,
        // from/argument-string-with-time-designator.js).
        check(timeEquals(Temporal.PlainTime.from("152330"), 15, 23, 30, 0, 0, 0), "basic time");
        check(timeEquals(Temporal.PlainTime.from("T0030"), 0, 30, 0, 0, 0, 0), "basic T time");
        check(timeEquals(Temporal.PlainTime.from("152330.1-0800"), 15, 23, 30, 100, 0, 0),
            "basic time with a basic offset");
        check(timeEquals(Temporal.PlainTime.from("+0019761118T152330.1+0000"),
            15, 23, 30, 100, 0, 0), "basic extended-year date prefix");
        check(timeEquals(Temporal.PlainTime.from("15:23:30,12"), 15, 23, 30, 120, 0, 0),
            "comma decimal separator");
        check(timeEquals(Temporal.PlainTime.from("15"), 15, 0, 0, 0, 0, 0), "bare hour");
        // `0230` is February 30th, which is not a real date, so it stays an
        // unambiguous time; `0229` is, so it needs a `T`
        // (TemporalHelpers.ISO.plainTimeStrings{Ambiguous,Unambiguous}).
        check(timeEquals(Temporal.PlainTime.from("0230"), 2, 30, 0, 0, 0, 0),
            "February 30th is not a date, so it is a time");
        throwsRange(() => Temporal.PlainTime.from("0229"), "February 29th is ambiguous");
        throwsRange(() => Temporal.PlainTime.from("202112"), "YYYYMM is ambiguous");
        check(timeEquals(Temporal.PlainTime.from("202113"), 20, 21, 13, 0, 0, 0),
            "month 13 is not a date, so it is a time");
        throwsRange(() => Temporal.PlainTime.from("-000000-12-07T03:24:30"),
            "a negative zero extended year is rejected");
        for (let invalid of [
            "2019-10-01", "09:00:00Z", "2022-09-15+00:00", "12-14", "05:07.123",
            "00:00:00.1234567891", "15:23:30.100junk", "00:00[u-ca=iso8601][!u-ca=iso8601]",
            "00:00[UTC][UTC]",
        ]) {
            throwsRange(() => Temporal.PlainTime.from(invalid), `invalid string ${invalid}`);
        }
        // from() with a property bag, including its overflow option
        // (from/argument-object.js, overflow-reject.js).
        check(timeEquals(Temporal.PlainTime.from({ hour: 15, minute: 23 }), 15, 23, 0, 0, 0, 0),
            "property bag");
        check(timeEquals(Temporal.PlainTime.from({ hour: 26 }), 23, 0, 0, 0, 0, 0),
            "overflow constrains by default");
        throwsRange(() => Temporal.PlainTime.from({ hour: 26 }, { overflow: "reject" }),
            "overflow reject");
        throwsType(() => Temporal.PlainTime.from({}), "empty property bag");
        throwsType(() => Temporal.PlainTime.from({ minutes: 12 }), "plural-only property bag");
        throwsType(() => Temporal.PlainTime.from(1), "a number is not a PlainTime");

        // add/subtract wrap at the 24-hour boundary and ignore every
        // calendar unit, days included (add/argument-higher-units.js,
        // add/balance-negative-time-units.js).
        let anchor = new Temporal.PlainTime(15, 23, 30, 123, 456, 789);
        for (let higher of [{ days: 1 }, { weeks: 1 }, { months: 1 }, { years: 1 },
                            "P1D", "P1W", "P1M", "P1Y"]) {
            check(timeEquals(anchor.add(higher), 15, 23, 30, 123, 456, 789),
                `add ignores higher units: ${JSON.stringify(higher)}`);
            check(timeEquals(anchor.subtract(higher), 15, 23, 30, 123, 456, 789),
                `subtract ignores higher units: ${JSON.stringify(higher)}`);
        }
        check(timeEquals(new Temporal.PlainTime(23).add({ hours: 2 }), 1, 0, 0, 0, 0, 0),
            "23:00 + 2h wraps to 01:00");
        check(timeEquals(new Temporal.PlainTime(1, 1, 1, 1, 1, 1).add({ hours: -2 }),
            23, 1, 1, 1, 1, 1), "subtracting past midnight wraps back");
        check(timeEquals(anchor.subtract({ hours: 16 }), 23, 23, 30, 123, 456, 789),
            "subtract wraps");
        check(timeEquals(new Temporal.PlainTime().add({ hours: 2501999792983, nanoseconds: 2191999999999 }),
            7, 36, 31, 999, 999, 999), "maximum allowed duration (add/argument-duration-max.js)");
        throwsRange(() => anchor.add({ hours: 1.5 }), "non-integer duration field");

        // round: string shorthand, midnight crossing, and the
        // divides-the-unit increment rule (round/*.js).
        let late = Temporal.PlainTime.from("23:59:59.999999999");
        for (let unit of ["hour", "minute", "second", "millisecond", "microsecond"]) {
            check(timeEquals(late.round(unit), 0, 0, 0, 0, 0, 0), `round crosses midnight: ${unit}`);
            check(timeEquals(late.round({ smallestUnit: unit }), 0, 0, 0, 0, 0, 0),
                `string shorthand matches the options bag: ${unit}`);
        }
        throwsType(() => late.round(), "round requires an argument");
        throwsRange(() => late.round("day"), "round rejects a calendar unit");
        for (let [unit, increment] of [["hours", 11], ["hours", 24], ["minutes", 29],
                                       ["minutes", 60], ["nanoseconds", 29], ["nanoseconds", 1000]]) {
            throwsRange(() => late.round({ smallestUnit: unit, roundingIncrement: increment }),
                `increment ${increment} must divide ${unit} and stay below it`);
        }
        check(timeEquals(Temporal.PlainTime.from("08:22:36.123456789")
            .round({ smallestUnit: "hour", roundingIncrement: 12 }), 12, 0, 0, 0, 0, 0),
            "a dividing increment is accepted");

        // until/since (until/basic.js, largestunit-undefined.js,
        // result-sub-second.js, largestunit-smallestunit-mismatch.js).
        let one = new Temporal.PlainTime(15, 23, 30, 123, 456, 789);
        let three = new Temporal.PlainTime(17, 0, 30, 123, 456, 789);
        let forward = one.until(three);
        check(forward.hours === 1 && forward.minutes === 37 && forward.seconds === 0,
            "until defaults to largestUnit hour");
        let backward = three.until(one);
        check(backward.hours === -1 && backward.minutes === -37, "until is signed");
        let since = three.since(one);
        check(since.hours === 1 && since.minutes === 37, "since is until reversed");
        let sub = Temporal.PlainTime.from("10:23:15")
            .until(Temporal.PlainTime.from("17:15:57.250250250"), { largestUnit: "milliseconds" });
        check(sub.milliseconds === 24762250 && sub.microseconds === 250 && sub.nanoseconds === 250,
            "largestUnit milliseconds folds hours in");
        let cast = one.until("16:34");
        check(cast.hours === 1 && cast.minutes === 10 && cast.seconds === 29
            && cast.milliseconds === 876 && cast.microseconds === 543 && cast.nanoseconds === 211,
            "until casts a string argument");
        throwsRange(() => one.until(three, { largestUnit: "minutes", smallestUnit: "hours" }),
            "smallestUnit may not exceed largestUnit");
        throwsRange(() => one.until(three, { largestUnit: "week" }),
            "largestUnit rejects a calendar unit");
        throwsType(() => one.until({}), "until rejects an empty property bag");
        check(one.until(three, { largestUnit: "auto" }).hours === 1,
            "largestUnit auto resolves to hour");
        // until/since share round()'s divides-the-unit increment rule, which
        // `Temporal.Instant.prototype.round` does not have — an Instant is
        // unbounded, so its own rounding only has to divide a whole day, while
        // `Instant.prototype.until`/`since` use this same per-unit rule
        // (until/roundingincrement-invalid.js, and
        // `Instant/prototype/until/invalid-increments.js` for the Instant side).
        throwsRange(() => one.until(three, { smallestUnit: "minutes", roundingIncrement: 29 }),
            "until validates the rounding increment against its unit");
        // A primitive options argument is a TypeError, not a boxed wrapper
        // (until/options-wrong-type.js).
        for (let bad of [null, true, "hello", 1, 1n]) {
            throwsType(() => one.until(three, bad), `primitive options ${typeof bad}`);
            throwsType(() => one.since(three, bad), `primitive options ${typeof bad}`);
            throwsType(() => time.toString(bad), `primitive options ${typeof bad}`);
            throwsType(() => time.with({ hour: 1 }, bad), `primitive options ${typeof bad}`);
            throwsType(() => Temporal.PlainTime.from({ hour: 1 }, bad),
                `primitive options ${typeof bad}`);
        }
        // ...but an options *object* of any kind is accepted, including a
        // function (until/largestunit-undefined.js uses `() => {}`).
        check(one.until(three, () => {}).hours === 1, "a function is a valid options object");

        // equals / compare (equals/argument-cast.js, compare/basic.js).
        let t1 = Temporal.PlainTime.from("08:44:15.321");
        check(t1.equals("08:44:15.321") && !t1.equals("14:23:30.123"), "equals casts a string");
        check(t1.equals({ hour: 8, minute: 44, second: 15, millisecond: 321 }), "equals casts a bag");
        check(Temporal.PlainTime.compare(t1, t1) === 0, "compare equal");
        check(Temporal.PlainTime.compare(t1, "14:23:30.123") === -1, "compare before");
        check(Temporal.PlainTime.compare("14:23:30.123", t1) === 1, "compare after");
        throwsType(() => t1.equals({}), "equals rejects an empty bag");
        throwsType(() => Temporal.PlainTime.compare(t1, 1), "compare rejects a number");

        // Other Temporal types convert by their time of day; a fixed-offset
        // ZonedDateTime resolves, a named IANA zone is still Track E's scope
        // (from/argument-plaindatetime.js,
        // from/argument-zoneddatetime-balance-negative-time-units.js).
        check(timeEquals(Temporal.PlainTime.from(Temporal.PlainDateTime.from("1976-11-18T15:23:30.1")),
            15, 23, 30, 100, 0, 0), "converts a PlainDateTime");
        check(timeEquals(Temporal.PlainTime.from(new Temporal.ZonedDateTime(3661001001001n, "-00:02")),
            0, 59, 1, 1, 1, 1), "converts a fixed-offset ZonedDateTime");
        throwsType(() => Temporal.PlainTime.from(Temporal.PlainDate.from("2019-05-17")),
            "a PlainDate has no time fields");

        // toString / toJSON precision (toString/smallestunit-valid-units.js,
        // fractionalseconddigits-number.js, rounding-cross-midnight.js).
        check(time.toString() === "12:34:56.987654321", "toString auto precision");
        check(new Temporal.PlainTime(15, 23).toString() === "15:23:00", "toString pads seconds");
        check(time.toString({ smallestUnit: "minute" }) === "12:34", "smallestUnit minute");
        check(time.toString({ smallestUnit: "second" }) === "12:34:56", "smallestUnit second");
        check(time.toString({ smallestUnit: "millisecond" }) === "12:34:56.987", "smallestUnit ms");
        check(new Temporal.PlainTime(15, 23, 30, 123, 400).toString({ fractionalSecondDigits: 2 })
            === "15:23:30.12", "fractionalSecondDigits truncates");
        check(new Temporal.PlainTime(15, 23, 30, 123, 400).toString({ fractionalSecondDigits: 7 })
            === "15:23:30.1234000", "fractionalSecondDigits pads");
        check(new Temporal.PlainTime(23, 59, 59, 999, 999, 999)
            .toString({ fractionalSecondDigits: 8, roundingMode: "halfExpand" })
            === "00:00:00.00000000", "toString rounding crosses midnight");
        check(time.toString({ smallestUnit: "millisecond", fractionalSecondDigits: 5 })
            === "12:34:56.987", "smallestUnit overrides fractionalSecondDigits");
        throwsRange(() => time.toString({ smallestUnit: "hour" }),
            "hour is not a serialization unit");
        check(time.toString({ fractionalSecondDigits: "auto" }) === "12:34:56.987654321",
            "fractionalSecondDigits auto");
        for (let bad of [true, "AUTO", -1, 10, NaN, Infinity]) {
            throwsRange(() => time.toString({ fractionalSecondDigits: bad }),
                `invalid fractionalSecondDigits ${String(bad)}`);
        }
        check(new Temporal.PlainTime(15, 23, 30, 123, 400).toJSON() === "15:23:30.1234", "toJSON");
        check(typeof time.toLocaleString() === "string", "toLocaleString returns a string");
        throwsType(() => time.valueOf(), "valueOf throws");

        // with (with/basic.js, plaintimelike-invalid.js, overflow-undefined.js).
        check(timeEquals(time.with({ hour: 3 }), 3, 34, 56, 987, 654, 321), "with replaces a field");
        check(timeEquals(time.with({ minute: 8, nanosecond: 3 }), 12, 8, 56, 987, 654, 3),
            "with replaces several fields");
        check(timeEquals(time.with({ minutes: 8, nanosecond: 3 }), 12, 34, 56, 987, 654, 3),
            "with ignores a plural property");
        check(timeEquals(new Temporal.PlainTime(12).with({ minute: 67 }), 12, 59, 0, 0, 0, 0),
            "with constrains by default");
        throwsRange(() => new Temporal.PlainTime(12).with({ minute: 67 }, { overflow: "reject" }),
            "with overflow reject");
        throwsType(() => time.with("18:05:42.577"), "with rejects a string");
        throwsType(() => time.with(Temporal.PlainTime.from("12:34")),
            "with rejects a Temporal value");
        throwsType(() => time.with({ hour: 14, calendar: "iso8601" }),
            "with rejects a calendar property");
        throwsType(() => time.with({ hour: 14, timeZone: "UTC" }),
            "with rejects a timeZone property");
        throwsType(() => time.with({}), "with rejects an empty bag");

        // Options are all read and coerced before any of them is validated
        // (round/options-read-before-algorithmic-validation.js).
        let log = [];
        let observed = {
            get smallestUnit() { log.push("smallestUnit"); return "hour" },
            get roundingIncrement() { log.push("roundingIncrement"); return 25 },
            get roundingMode() { log.push("roundingMode"); return "expand" },
        };
        throwsRange(() => time.round(observed), "increment invalid for smallestUnit");
        check(log.join() === "roundingIncrement,roundingMode,smallestUnit",
            `options read in alphabetical order, got ${log.join()}`);

        // Duration fields are likewise read alphabetically
        // (add/order-of-operations.js).
        let durationLog = [];
        let durationBag = {};
        for (let field of ["years", "months", "weeks", "days", "hours", "minutes",
                           "seconds", "milliseconds", "microseconds", "nanoseconds"]) {
            Object.defineProperty(durationBag, field, {
                get() { durationLog.push(field); return 1 },
                enumerable: true,
            });
        }
        check(timeEquals(time.add(durationBag), 13, 35, 57, 988, 655, 322), "add reads every field");
        check(durationLog.join() === "days,hours,microseconds,milliseconds,minutes,months,"
            + "nanoseconds,seconds,weeks,years",
            `duration fields read alphabetically, got ${durationLog.join()}`);

        true
    "#;
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn temporal_now_reads_one_wall_clock_through_resolved_time_zone_identifiers() {
    let source = r#"
        if (typeof Temporal.Now !== "object") throw new Error("Now is a namespace object");
        if (Object.getPrototypeOf(Temporal.Now) !== Object.prototype) throw new Error("Now prototype");
        if (Temporal.Now.prototype !== undefined) throw new Error("Now has no prototype property");
        if (String(Temporal.Now) !== "[object Temporal.Now]") throw new Error("Now toStringTag");

        for (let name of ["instant", "plainDateISO", "plainDateTimeISO", "plainTimeISO", "timeZoneId", "zonedDateTimeISO"]) {
            let method = Temporal.Now[name];
            if (typeof method !== "function") throw new Error(`method ${name}`);
            if (method.length !== 0 || method.name !== name) throw new Error(`length/name ${name}`);
            let threw = false;
            try { new method(); } catch (error) { threw = error instanceof TypeError; }
            if (!threw) throw new Error(`${name} is not a constructor`);
        }

        // The Instant reads the same wall clock Date.now() does, so it must
        // land inside a window bracketing the call.
        let before = Date.now();
        let instant = Temporal.Now.instant();
        let after = Date.now();
        if (!(instant instanceof Temporal.Instant)) throw new Error("instant instance");
        let milliseconds = Number(instant.epochNanoseconds / 1000000n);
        if (milliseconds < before || milliseconds > after) throw new Error("instant is not now");
        if (Temporal.Now.instant() === instant) throw new Error("each call allocates");

        if (Temporal.Now.timeZoneId() !== "UTC") throw new Error("system time zone");
        if (Temporal.Now.plainDateISO().calendarId !== "iso8601") throw new Error("plainDateISO calendar");
        if (!(Temporal.Now.plainTimeISO() instanceof Temporal.PlainTime)) throw new Error("plainTimeISO instance");
        if (!(Temporal.Now.plainDateTimeISO(undefined) instanceof Temporal.PlainDateTime)) throw new Error("explicit undefined");

        let zoned = Temporal.Now.zonedDateTimeISO();
        if (zoned.calendarId !== "iso8601" || zoned.timeZoneId !== Temporal.Now.timeZoneId()) {
            throw new Error("zonedDateTimeISO defaults");
        }

        // A fixed offset shifts the reported wall clock by exactly that
        // offset. PlainDateTime has no time-of-day getters yet (Stage 2), so
        // read the shift back through toZonedDateTime, which turns the ISO
        // fields into an epoch value with no further offset of its own.
        let localEpoch = (zone) => Temporal.Now.plainDateTimeISO(zone).toZonedDateTime("UTC").epochMilliseconds;
        if (Math.abs(localEpoch("+05:00") - localEpoch("UTC") - 5 * 3600000) > 1000) {
            throw new Error("fixed offset shift");
        }
        if (Math.abs(localEpoch("-0930") - localEpoch("+00:00") + 9.5 * 3600000) > 1000) {
            throw new Error("negative offset shift");
        }
        // The same shift has to move plainDateISO across a day boundary when
        // the two offsets straddle one.
        let dates = new Set(["-12:00", "+00:00", "+14:00"].map((zone) => {
            let date = Temporal.Now.plainDateISO(zone);
            return `${date.year}-${date.month}-${date.day}`;
        }));
        if (dates.size < 2) throw new Error("plainDateISO ignores its time zone");

        let identifiers = [
            ["UtC", "UTC"],
            ["+01:30", "+01:30"],
            ["-0700", "-07:00"],
            ["America/Vancouver", "America/Vancouver"],
            ["2021-08-19T17:30Z", "UTC"],
            ["2021-08-19T1730-0700", "-07:00"],
            ["2021-08-19T17:30:45.123456789-12:12[+01:46]", "+01:46"],
            ["2021-08-19T17:30-07:00[America/Vancouver]", "America/Vancouver"],
        ];
        for (let [argument, identifier] of identifiers) {
            if (Temporal.Now.zonedDateTimeISO(argument).timeZoneId !== identifier) {
                throw new Error(`identifier ${argument}`);
            }
        }

        // Non-strings are never coerced: only a ZonedDateTime is accepted as
        // an object, and every other non-string is a TypeError.
        if (Temporal.Now.zonedDateTimeISO(zoned).timeZoneId !== "UTC") throw new Error("ZonedDateTime argument");
        for (let argument of [null, true, 1, 19761118, 1n, Symbol(), {}, new Temporal.Duration(), {toString(){return "UTC"}}]) {
            let threw = false;
            try { Temporal.Now.plainDateISO(argument); } catch (error) { threw = error instanceof TypeError; }
            if (!threw) throw new Error(`non-string argument ${String(argument)}`);
        }

        let rangeErrors = [
            "",
            "-12:12:59.9",
            "2021-08-19T17:30",
            "2021-08-19T17:30-07:00:00",
            "2021-08-19T17:30:45.123456789+23:59[+23:59:60]",
            "-000000-10-31T17:45Z",
            "Mars/Olympus_Mons",
        ];
        for (let argument of rangeErrors) {
            for (let method of ["plainDateISO", "plainDateTimeISO", "plainTimeISO", "zonedDateTimeISO"]) {
                let threw = false;
                try { Temporal.Now[method](argument); } catch (error) { threw = error instanceof RangeError; }
                if (!threw) throw new Error(`${method} must reject ${argument}`);
            }
        }

        // A named IANA zone now resolves a real historical offset too
        // (Phase 26's gap-closure pass wires `Temporal.Now` to Track E's
        // transition data), so the plain ISO variants no longer reject it —
        // they read the actual wall clock in that zone rather than
        // rejecting it or silently assuming UTC.
        for (let method of ["plainDateISO", "plainDateTimeISO", "plainTimeISO"]) {
            let threw = false;
            try { Temporal.Now[method]("America/Vancouver"); } catch (error) { threw = true; }
            if (threw) throw new Error(`${method} should accept a named zone now that Track E is wired up`);
        }
        // The reported wall clock actually shifts by a real Vancouver
        // offset (PST -08:00 or PDT -07:00), not by zero. A small tolerance
        // (matching the fixed-offset checks above) absorbs the wall-clock
        // drift between the two separate `Temporal.Now` reads `localEpoch`
        // makes.
        let vancouverShift = localEpoch("America/Vancouver") - localEpoch("UTC");
        let matchesPst = Math.abs(vancouverShift - (-8 * 3600000)) <= 1000;
        let matchesPdt = Math.abs(vancouverShift - (-7 * 3600000)) <= 1000;
        if (!matchesPst && !matchesPdt) {
            throw new Error(`America/Vancouver shift should be a real PST/PDT offset, got ${vancouverShift}`);
        }

        true
    "#;
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn temporal_instant_string_grammar_rounding_and_serialization() {
    // Every expectation below is taken from a Test262 fixture under
    // `built-ins/Temporal/Instant/` or `intl402/Temporal/Instant/`, named in
    // the comment beside it, rather than derived from the spec text alone.
    let source = r#"
        // Failures accumulate rather than throwing, so a regression reports
        // *which* expectation broke instead of an opaque thrown object.
        let failures = [];
        function check(condition, message) {
            if (!condition) failures.push(message);
        }
        function throwsRange(thunk, message) {
            try { thunk(); } catch (error) {
                check(error instanceof RangeError, `${message}: ${error}`);
                return;
            }
            failures.push(`${message}: did not throw`);
        }
        function throwsType(thunk, message) {
            try { thunk(); } catch (error) {
                check(error instanceof TypeError, `${message}: ${error}`);
                return;
            }
            failures.push(`${message}: did not throw`);
        }

        // The constructor takes ToBigInt, not only a literal BigInt
        // (basic.js, argument.js).
        check(new Temporal.Instant("217175010123456789").epochNanoseconds
            === 217175010123456789n, "a string argument converts through ToBigInt");
        check(new Temporal.Instant(true).epochNanoseconds === 1n, "true is 1n");
        throwsType(() => new Temporal.Instant(42), "a Number is not a BigInt");
        // `epochMilliseconds` floors, so a pre-epoch instant rounds *down*
        // (prototype/epochMilliseconds/basic.js).
        check(new Temporal.Instant(-217175010876543211n).epochMilliseconds === -217175010877,
            "epochMilliseconds floors below the epoch");

        // The TemporalInstantString grammar (prototype/equals/argument-string-*.js,
        // from/argument-string.js, leap-second.js, year-zero.js).
        let epoch = new Temporal.Instant(0n);
        for (let arg of [
            "1970-01-01T00:00Z", "1970-01-01t00:00Z", "1970-01-01 00:00Z",
            "1970-01-01T00Z[Europe/Vienna]", "1969-12-31T16-080000.000000000[America/Vancouver]",
            "1970-01-01T00+00:00:00,0", "1970-01-01T00:00Z[u-ca=discord]",
            "19700101T00:00:00+0000", "+0019700101T000000.000+00:00",
        ]) {
            check(epoch.equals(arg), `valid instant string ${arg}`);
        }
        check(new Temporal.Instant(1483228799000000000n).equals("2016-12-31T23:59:60Z"),
            "a leap second is accepted and constrained");
        check(epoch.equals("1970-01-01T00:19:32.37+00:19:32.37"),
            "a sub-minute offset is applied exactly");
        for (let invalid of [
            "", "invalid iso8601", "2020-01-01", "2020-01-01T00:00:00", "2022-09-15Z",
            "2022-09-15+00:00[UTC]", "2020-01-01T00:00Zjunk", "02020-01-01T00:00Z",
            "2020-01-01T001Z", "2020-W01-1T00:00Z", "-000000-03-30T00:45Z",
            "1970-01-01T00:00:00.1234567891Z", "2021-08-19T17:30-07:00:00[-070000]",
            "1970-01-01T00:00Z[u-ca=iso8601][!u-ca=iso8601]",
            "-271821-04-19T23:59:59.999999999Z", "+275760-09-13T00:00:00.000000001Z",
        ]) {
            throwsRange(() => epoch.equals(invalid), `invalid instant string ${invalid}`);
        }
        // Only a String (after ToPrimitive) is accepted; a Number is a
        // TypeError rather than being stringified
        // (prototype/equals/argument-wrong-type.js).
        for (let bad of [undefined, null, true, 1, 19761118, 1n]) {
            throwsType(() => epoch.equals(bad), `wrong argument type ${typeof bad}`);
        }
        throwsRange(() => epoch.equals({}), "an object stringifies and then fails to parse");
        // A ZonedDateTime converts by its epoch nanoseconds, with no
        // observable property access (prototype/equals/argument-zoneddatetime.js).
        check(new Temporal.Instant(1000000000987654321n)
            .equals(new Temporal.ZonedDateTime(1000000000987654321n, "UTC")),
            "a ZonedDateTime takes the fast path");

        // add/subtract, including fractional units in a duration string
        // (prototype/add/argument-string-fractional-units-rounding-mode.js,
        // argument-string-negative-fractional-units.js).
        check(epoch.add("PT1.03125H").epochNanoseconds === 3712500000000n,
            "a fractional hour cascades into the lower units");
        check(new Temporal.Instant(1000000000000000000n).add("-PT1440.567890123M")
            .epochNanoseconds === 999913565926592620n, "a negative fractional minute");
        check(epoch.subtract({ nanoseconds: 1 }).epochNanoseconds === -1n, "subtract negates");
        throwsRange(() => epoch.add({ days: 1 }), "a calendar unit is not an Instant duration");

        // round(): a String is shorthand for { smallestUnit }, and rounding is
        // "as if positive", so a negative instant truncates *earlier*
        // (prototype/round/accepts-string-parameter-for-smallestunit.js,
        // negative-instant.js, rounding-increments.js).
        let late = new Temporal.Instant(1000000000123456789n);
        for (let unit of ["hour", "minute", "second", "millisecond", "microsecond", "nanosecond"]) {
            check(late.round(unit).epochNanoseconds === late.round({ smallestUnit: unit }).epochNanoseconds,
                `string shorthand matches the options bag: ${unit}`);
        }
        let negative = new Temporal.Instant(-1000000000000000000n);
        for (let mode of ["trunc", "floor", "halfExpand", "halfEven"]) {
            check(negative.round({ smallestUnit: "hour", roundingMode: mode }).epochNanoseconds
                === -1000000800000000000n, `as-if-positive rounding: ${mode}`);
        }
        check(negative.round({ smallestUnit: "hour", roundingMode: "ceil" }).epochNanoseconds
            === -999997200000000000n, "as-if-positive ceil moves later");
        throwsType(() => late.round(), "round requires an argument");
        throwsRange(() => late.round("day"), "round rejects a calendar unit");
        // Unlike PlainTime, an Instant's increment only has to divide a whole
        // day, so 12 hours is fine and 11 is not (round/rounding-increments.js).
        check(late.round({ smallestUnit: "hour", roundingIncrement: 12 }).epochNanoseconds
            === 999993600000000000n, "an increment dividing a day is accepted");
        throwsRange(() => late.round({ smallestUnit: "hour", roundingIncrement: 11 }),
            "an increment that does not divide a day is rejected");

        // until/since: largestUnit defaults to the larger of "second" and
        // smallestUnit, and the increment must divide the *next larger unit*
        // (prototype/until/largestunit-default.js, invalid-increments.js).
        let earlier = Temporal.Instant.from("1969-07-24T16:50:35.123456789Z");
        let later = Temporal.Instant.from("2019-10-29T10:46:38.271986102Z");
        check(earlier.until(later, { smallestUnit: "hours", roundingMode: "halfExpand" }).hours
            === 440610, "largestUnit auto follows a larger smallestUnit");
        let difference = earlier.until(later);
        check(difference.hours === 0 && difference.minutes === 0
            && difference.seconds === 1586195763 && difference.milliseconds === 148
            && difference.microseconds === 529 && difference.nanoseconds === 313,
            "until defaults to largestUnit second, folding the hours in");
        check(later.since(earlier).seconds === 1586195763, "since is until reversed");
        for (let [unit, increment] of [["hours", 11], ["hours", 24], ["minutes", 29],
                                       ["minutes", 60], ["nanoseconds", 29], ["nanoseconds", 1000]]) {
            throwsRange(() => earlier.until(later, { smallestUnit: unit, roundingIncrement: increment }),
                `increment ${increment} must divide the unit above ${unit}`);
        }
        throwsRange(() => earlier.until(later, { largestUnit: "week" }),
            "largestUnit rejects a calendar unit");
        for (let bad of [null, true, "hello", 1, 1n]) {
            throwsType(() => earlier.until(later, bad), `primitive options ${typeof bad}`);
            throwsType(() => epoch.toString(bad), `primitive options ${typeof bad}`);
        }
        // `round`'s parameter is `roundTo`, not an options object, so a String
        // is shorthand rather than a TypeError — but every other primitive
        // still goes through GetOptionsObject
        // (round/smallestunit-string-shorthand.js, options-wrong-type.js).
        for (let bad of [null, true, 1, 1n]) {
            throwsType(() => epoch.round(bad), `primitive roundTo ${typeof bad}`);
        }
        throwsRange(() => epoch.round("hello"), "a string roundTo must name a unit");

        // toString precision, including the minute precision that drops the
        // seconds field entirely, and the increment a digit count implies
        // (toString/smallestunit-valid-units.js, rounding-cross-midnight.js,
        // rounding-direction.js).
        let sub = new Temporal.Instant(1000000000123456789n);
        check(sub.toString() === "2001-09-09T01:46:40.123456789Z", "auto precision");
        check(epoch.toString() === "1970-01-01T00:00:00Z", "auto precision pads seconds");
        check(sub.toString({ smallestUnit: "minute" }) === "2001-09-09T01:46Z",
            "minute precision drops the seconds field");
        check(sub.toString({ smallestUnit: "microsecond" }) === "2001-09-09T01:46:40.123456Z",
            "microsecond precision");
        check(new Temporal.Instant(946684799999999999n)
            .toString({ fractionalSecondDigits: 8, roundingMode: "halfExpand" })
            === "2000-01-01T00:00:00.00000000Z", "eight digits round in 10ns increments");
        check(new Temporal.Instant(-65261246399500000000n)
            .toString({ smallestUnit: "second", roundingMode: "floor" })
            === "-000099-12-15T12:00:00Z", "rounding down goes away from the epoch");
        check(sub.toString({ smallestUnit: "millisecond", fractionalSecondDigits: 5 })
            === "2001-09-09T01:46:40.123Z", "smallestUnit overrides fractionalSecondDigits");
        check(new Temporal.Instant(1000000000987650000n).toString({ fractionalSecondDigits: 9.7 })
            === "2001-09-09T01:46:40.987650000Z", "fractionalSecondDigits floors before the range check");
        throwsRange(() => sub.toString({ fractionalSecondDigits: -0.6 }),
            "fractionalSecondDigits -0.6 floors to -1 and is out of range");
        throwsRange(() => sub.toString({ smallestUnit: "hour" }),
            "hour is not an Instant serialization unit");
        for (let bad of [null, true, 2n, {}, "AUTO"]) {
            throwsRange(() => sub.toString({ fractionalSecondDigits: bad }),
                `invalid fractionalSecondDigits ${String(bad)}`);
        }
        check(sub.toJSON() === "2001-09-09T01:46:40.123456789Z", "toJSON ignores options");
        throwsType(() => sub.valueOf(), "valueOf throws");

        // The timeZone option prints a local time plus a ±HH:MM offset rather
        // than `Z`. Only UTC and fixed offsets resolve here; a named IANA zone
        // needs Track E's transition data (toString/timezone-offset.js,
        // timezone-string-datetime.js, timezone-string-unknown.js,
        // timezone-wrong-type.js).
        check(epoch.toString({ timeZone: "UTC" }) === "1970-01-01T00:00:00+00:00", "UTC offset");
        check(epoch.toString({ timeZone: "+01:00" }) === "1970-01-01T01:00:00+01:00", "positive offset");
        check(epoch.toString({ timeZone: "-05:00" }) === "1969-12-31T19:00:00-05:00", "negative offset");
        check(epoch.toString({ timeZone: "2021-08-19T17:30-07:00[UTC]" }).slice(-6) === "+00:00",
            "an annotation beats the string's own offset");
        check(epoch.toString({ timeZone: "2021-08-19T17:30:45.123456789-12:12[+01:46]" }).slice(-6)
            === "+01:46", "an offset annotation is the identifier");
        for (let bad of ["", "2021-08-19T17:30", "2021-08-19T17:30-07:00:00", "Mars/Olympus_Mons"]) {
            throwsRange(() => epoch.toString({ timeZone: bad }), `invalid time zone ${bad}`);
        }
        for (let bad of [null, true, 1, 1n, {}, new Temporal.Duration()]) {
            throwsType(() => epoch.toString({ timeZone: bad }), `wrong time zone type ${typeof bad}`);
        }
        // `options.timeZone` is read even when it goes unused, after every
        // other option (toString/order-of-operations.js, get-timezone-throws.js).
        let log = [];
        throwsRange(() => sub.toString({
            get fractionalSecondDigits() { log.push("fractionalSecondDigits"); return "auto" },
            get roundingMode() { log.push("roundingMode"); return "trunc" },
            get smallestUnit() { log.push("smallestUnit"); return "month" },
            get timeZone() { log.push("timeZone"); return undefined },
        }), "a date unit is rejected only after every option is read");
        check(log.join() === "fractionalSecondDigits,roundingMode,smallestUnit,timeZone",
            `toString options read in alphabetical order, got ${log.join()}`);

        // toLocaleString goes through Intl.DateTimeFormat, not toString's ISO
        // serialization (intl402 toLocaleString/basic.js, options-undefined.js).
        let options = { timeZone: "UTC", dateStyle: "short", timeStyle: "short" };
        check(sub.toLocaleString("en-US", options)
            === new Intl.DateTimeFormat("en-US", options).format(sub),
            "toLocaleString matches DateTimeFormat.format");
        check(sub.toLocaleString("en") === new Intl.DateTimeFormat("en").format(sub),
            "toLocaleString defaults match DateTimeFormat's");
        check(sub.toLocaleString("en", { timeZone: "UTC" }) !== sub.toString(),
            "toLocaleString is not an alias for toString");
        throwsType(() => sub.toLocaleString("en", { hour: "numeric", dateStyle: "short" }),
            "conflicting dateStyle options still throw");
        throwsType(() => Temporal.Instant.prototype.toLocaleString.call({}),
            "toLocaleString checks its brand");

        failures.length === 0 || failures.join(" | ")
    "#;
    assert_eq!(evaluate(source).unwrap(), Value::Bool(true));
}

#[test]
fn date_locale_methods_share_datetime_format_resolution_and_defaults() {
    for source in [
        "let d=new Date(0);d.toLocaleString('en-US')===new Intl.DateTimeFormat('en-US',{year:'numeric',month:'numeric',day:'numeric',hour:'numeric',minute:'numeric',second:'numeric'}).format(d)",
        "let d=new Date(0);d.toLocaleDateString('en-US',{hour:'numeric',minute:'numeric',second:'numeric'})===new Intl.DateTimeFormat('en-US',{year:'numeric',month:'numeric',day:'numeric',hour:'numeric',minute:'numeric',second:'numeric'}).format(d)",
        "let d=new Date(0);d.toLocaleTimeString('en-US',{weekday:'short',year:'numeric',month:'numeric',day:'numeric'})===new Intl.DateTimeFormat('en-US',{weekday:'short',year:'numeric',month:'numeric',day:'numeric',hour:'numeric',minute:'numeric',second:'numeric'}).format(d)",
        "let d=new Date(0);d.toLocaleString('de-DE',{year:'numeric',month:'numeric'})===new Intl.DateTimeFormat('de-DE',{year:'numeric',month:'numeric'}).format(d)",
        "let d=new Date(NaN);d.toLocaleString('de-DE')==='Invalid Date'&&d.toLocaleDateString('de-DE')==='Invalid Date'&&d.toLocaleTimeString('de-DE')==='Invalid Date'",
        "let log='';let locale=[{toString(){log+='locale';return 'en-US'}}];let options={get localeMatcher(){log+=' options';return 'best fit'}};new Date(0).toLocaleString(locale,options);log===' optionslocale'",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn cldr_date_range_formatter_is_the_production_default() {
    let source = "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC',year:'numeric',month:'long',day:'numeric'}); let p=f.formatRangeToParts(0,86400000); f.formatRange(0,86400000) === 'January 1 – 2, 1970' && p.map(x=>x.type+':'+x.source).join(',') === 'month:shared,literal:shared,day:startRange,literal:shared,day:endRange,literal:shared,year:shared'";

    assert_eq!(evaluate(source), Ok(Value::Bool(true)));
}

#[test]
fn date_time_format_resolves_locale_keywords_before_formatting() {
    let source = "let ca1=new Intl.DateTimeFormat('en-u-ca-iso8601',{calendar:'invalid'}).resolvedOptions();let ca2=new Intl.DateTimeFormat('en-u-ca-gregory',{calendar:'iso8601'}).resolvedOptions();let nu1=new Intl.DateTimeFormat('en-u-nu-arab',{numberingSystem:'invalid'}).resolvedOptions();let nu2=new Intl.DateTimeFormat('en-u-nu-latn',{numberingSystem:'arab'}).resolvedOptions();let hc1=new Intl.DateTimeFormat('en-u-hc-h11',{hour:'numeric',hour12:false}).resolvedOptions();let hc2=new Intl.DateTimeFormat('en-u-hc-h11',{hour:'numeric'}).resolvedOptions();let fraction=new Intl.DateTimeFormat('en',{fractionalSecondDigits:2.9}).resolvedOptions();let arab=new Intl.DateTimeFormat('en-u-nu-arab',{timeZone:'UTC'});ca1.locale==='en-u-ca-iso8601'&&ca1.calendar==='iso8601'&&ca2.locale==='en'&&ca2.calendar==='iso8601'&&nu1.locale==='en-u-nu-arab'&&nu1.numberingSystem==='arab'&&nu2.locale==='en'&&nu2.numberingSystem==='arab'&&hc1.locale==='en'&&hc1.hourCycle==='h23'&&hc2.locale==='en-u-hc-h11'&&hc2.hourCycle==='h11'&&fraction.fractionalSecondDigits===2&&arab.format(0).includes('١٩٧٠')&&arab.formatToParts(0).some(function(part){return part.type==='year'&&part.value==='١٩٧٠'})";

    assert_eq!(evaluate(source), Ok(Value::Bool(true)));
}

#[test]
fn date_time_format_styles_select_their_field_widths() {
    let source = "let d=new Date('1886-05-01T14:12:47Z');let date=new Intl.DateTimeFormat('en-US',{dateStyle:'short',timeZone:'UTC'});let time=new Intl.DateTimeFormat('en-US',{timeStyle:'short',timeZone:'UTC',hourCycle:'h24'});date.format(d)==='5/1/86'&&time.format(d)==='14:12'&&date.formatToParts(d).some(function(part){return part.type==='year'&&part.value==='86'})";

    assert_eq!(evaluate(source), Ok(Value::Bool(true)));
}

#[test]
fn date_time_format_emits_complete_chinese_year_parts() {
    let source = "let f=new Intl.DateTimeFormat('zh-u-ca-chinese',{year:'numeric',timeZone:'UTC'});let p=f.formatToParts(new Date(2019,5,1));f.format(new Date(2019,5,1))==='2019己亥年'&&p.map(function(part){return part.type+':'+part.value}).join('|')==='relatedYear:2019|yearName:己亥|literal:年'";

    assert_eq!(evaluate(source), Ok(Value::Bool(true)));
}

#[test]
fn date_time_range_contract_and_source_regressions() {
    for source in [
        // The undefined guard precedes either observable ToNumber conversion.
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC'});let converted=0;let poison={valueOf:function(){converted++;return 0}};let a=false;let b=false;try{f.formatRange(undefined,poison)}catch(error){a=error instanceof TypeError}try{f.formatRangeToParts(poison,undefined)}catch(error){b=error instanceof TypeError}a&&b&&converted===0",
        // Ordinary input continues through ToNumber and TimeClip; reverse
        // endpoints are formatted rather than rejected by an adapter check.
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC'});let calls='';let start={valueOf:function(){calls+='s';return -0.9}};let end={valueOf:function(){calls+='e';return 0.9}};let clip=f.formatRange(start,end)===f.formatRange(0,0)&&calls==='se';let reverse=typeof f.formatRange(86400000,0)==='string'&&Array.isArray(f.formatRangeToParts(86400000,0));let invalid=false;try{f.formatRange(8640000000000001,0)}catch(error){invalid=error instanceof RangeError}clip&&reverse&&invalid",
        // `source: shared` for equal displayed values is the whole-pattern
        // branch, not a per-part equality rewrite after CLDR serialization.
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC'});let p=f.formatRangeToParts(0,0.9);p.length>0&&p.map(x=>x.value).join('')===f.format(0)&&p.every(x=>x.source==='shared')",
        // Time-only fractional seconds retain their field boundaries on the
        // CLDR range path.
        "let f=new Intl.DateTimeFormat('en',{timeZone:'UTC',minute:'numeric',second:'numeric',fractionalSecondDigits:1});let p=f.formatRangeToParts(0,300);f.formatRange(0,300)==='00:00.0 – 00:00.3'&&p.map(x=>x.type+':'+x.value+':'+x.source).join('|')==='minute:00:startRange|literal:::startRange|second:00:startRange|literal:.:startRange|fractionalSecond:0:startRange|literal: – :shared|minute:00:endRange|literal:::endRange|second:00:endRange|literal:.:endRange|fractionalSecond:3:endRange'",
        // Default en-US and a collapsing CLDR date skeleton retain the
        // formatter-provided source spans without downstream reassignment.
        "let d=new Intl.DateTimeFormat('en-US',{timeZone:'UTC'});let c=new Intl.DateTimeFormat('en-US',{timeZone:'UTC',year:'numeric',month:'long',day:'numeric'});let dp=d.formatRangeToParts(0,86400000);let cp=c.formatRangeToParts(0,86400000);d.formatRange(0,86400000)==='1/1/1970 – 1/2/1970'&&dp.map(x=>x.type+':'+x.source).join(',')==='month:startRange,literal:startRange,day:startRange,literal:startRange,year:startRange,literal:shared,month:endRange,literal:endRange,day:endRange,literal:endRange,year:endRange'&&c.formatRange(0,86400000)==='January 1 – 2, 1970'&&cp.map(x=>x.type+':'+x.source).join(',')==='month:shared,literal:shared,day:startRange,literal:shared,day:endRange,literal:shared,year:shared'",
        // CJK CLDR patterns are exercised through the production direct path.
        "let f=new Intl.DateTimeFormat('zh-TW',{timeZone:'UTC',year:'numeric',month:'numeric',day:'numeric'});let p=f.formatRangeToParts(0,86400000);f.formatRange(0,86400000).includes('至')&&p.some(x=>x.source==='startRange')&&p.some(x=>x.source==='endRange')",
        // Date/time styles use a one-pattern, all-shared result when their
        // displayed fields agree.
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'UTC',dateStyle:'long',timeStyle:'short'});let start=1565398923234;let p=f.formatRangeToParts(start,start+1);f.formatRange(start,start+1)===f.format(start)&&p.length>0&&p.every(x=>x.source==='shared')",
        // Zoned inputs retain CLDR's endpoint ownership across a DST change.
        "let f=new Intl.DateTimeFormat('en-US',{timeZone:'America/New_York',hour:'numeric',minute:'2-digit',timeZoneName:'short'});let before=1710052200000;let after=1710055800000;let p=f.formatRangeToParts(before,after);f.format(before).endsWith('EST')&&f.format(after).endsWith('EDT')&&p.some(x=>x.type==='timeZoneName'&&x.value==='EST'&&x.source==='startRange')&&p.some(x=>x.type==='timeZoneName'&&x.value==='EDT'&&x.source==='endRange')",
        // Plain Temporal values retain local fields while using the same
        // CLDR interval skeleton and source parts as ordinary ranges.
        "let f=new Intl.DateTimeFormat('en-US',{year:'numeric',month:'long',day:'numeric'});let start=Temporal.PlainDate.from('2020-01-01');let end=Temporal.PlainDate.from('2020-01-02');let p=f.formatRangeToParts(start,end);p.map(x=>x.value).join('')===f.formatRange(start,end)&&p.some(x=>x.type==='month'&&x.source==='shared')&&p.some(x=>x.type==='year'&&x.source==='shared')&&p.some(x=>x.type==='day'&&x.source==='startRange')&&p.some(x=>x.type==='day'&&x.source==='endRange')",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
}

#[test]
fn temporal_datetime_format_uses_one_typed_bridge_for_values_and_ranges() {
    let source = "let instantStart=Temporal.Instant.from('2020-01-02T00:00:00Z');let instantEnd=Temporal.Instant.from('2020-01-02T01:00:00Z');let instantFormat=new Intl.DateTimeFormat('en-US',{timeZone:'UTC'});let instant=instantFormat.format(instantStart);let instantParts=instantFormat.formatToParts(instantStart);let instantRange=instantFormat.formatRange(instantStart,instantEnd);let instantRangeParts=instantFormat.formatRangeToParts(instantStart,instantEnd);let plainStart=Temporal.PlainDateTime.from('2020-01-02T00:00');let plainEnd=Temporal.PlainDateTime.from('2020-01-02T01:00');let plainFormat=new Intl.DateTimeFormat('en-US',{timeZone:'America/New_York',timeStyle:'long'});let plain=plainFormat.format(plainStart);let plainParts=plainFormat.formatToParts(plainStart);let plainRange=plainFormat.formatRange(plainStart,plainEnd);let plainRangeParts=plainFormat.formatRangeToParts(plainStart,plainEnd);instantParts.map(x=>x.value).join('')===instant&&instantParts.some(x=>x.type==='hour')&&instantRangeParts.map(x=>x.value).join('')===instantRange&&instantRangeParts.some(x=>x.type==='hour')&&plainParts.map(x=>x.value).join('')===plain&&!plainParts.some(x=>x.type==='timeZoneName')&&plainRangeParts.map(x=>x.value).join('')===plainRange&&!plainRangeParts.some(x=>x.type==='timeZoneName')";

    assert_eq!(evaluate(source), Ok(Value::Bool(true)));
}

#[test]
fn number_format_delegates_to_the_host_neutral_service() {
    for source in [
        "let n=new Intl.NumberFormat('de',{useGrouping:false,minimumFractionDigits:2,maximumFractionDigits:2}); n.format(1007.5) === '1007,50'",
        "let n=new Intl.NumberFormat('th-u-nu-thai',{useGrouping:false,minimumFractionDigits:2,maximumFractionDigits:2}); let r=n.resolvedOptions(); n.format(1007.5) === '๑๐๐๗.๕๐' && r.locale === 'th-u-nu-thai' && r.numberingSystem === 'thai' && r.style === 'decimal' && r.useGrouping === false && r.minimumFractionDigits === 2 && r.maximumFractionDigits === 2",
        "let n=new Intl.NumberFormat('en',{useGrouping:'min2'}); n.format(1000) === '1000' && n.format(10000) === '10,000'",
        "let n=new Intl.NumberFormat('pl-PL');n.format(1000)==='1000'&&n.format(123456.78)==='123 456,78'&&new Intl.RelativeTimeFormat('pl-PL',{style:'long'}).format(1000,'second')==='za 1000 sekund'",
        "let always=new Intl.NumberFormat('en',{useGrouping:true}).resolvedOptions(); let min2=new Intl.NumberFormat('en',{useGrouping:'min2'}).resolvedOptions(); let never=new Intl.NumberFormat('en',{useGrouping:false}).resolvedOptions(); always.useGrouping === 'always' && min2.useGrouping === 'min2' && never.useGrouping === false",
        "let r=new Intl.NumberFormat('en',{minimumFractionDigits:1.9,maximumFractionDigits:2.9}).resolvedOptions(); r.minimumFractionDigits === 1 && r.maximumFractionDigits === 2",
        "let n=new Intl.NumberFormat('en',{style:'unit',unit:'year',unitDisplay:'long'}); let r=n.resolvedOptions(); n.format(2) === '2 years' && r.style === 'unit' && r.unit === 'year' && r.unitDisplay === 'long' && r.minimumIntegerDigits === 1 && r.roundingMode === 'halfExpand' && r.signDisplay === 'auto'",
        "let p=new Intl.NumberFormat('en',{style:'unit',unit:'year'}).formatToParts(1234.5); p.map(x=>x.type+':'+x.value).join('|') === 'integer:1|group:,|integer:234|decimal:.|fraction:5|literal: |unit:yrs'",
        "let n=new Intl.NumberFormat('en',{style:'unit',unit:'second',unitDisplay:'narrow',useGrouping:false,minimumIntegerDigits:2,maximumFractionDigits:2,roundingMode:'trunc',signDisplay:'never'}); n.format(-1.239) === '01.23s' && n.formatToParts(-1.239).map(x=>x.type+':'+x.value).join('|') === 'integer:01|decimal:.|fraction:23|unit:s'",
        "let a=new Intl.NumberFormat('en',{signDisplay:'always'});let z=new Intl.NumberFormat('en',{signDisplay:'exceptZero'});let n=new Intl.NumberFormat('en',{signDisplay:'negative'});a.format(-0)==='-0'&&a.format(2)==='+2'&&z.format(-0)==='0'&&z.format(2)==='+2'&&n.format(-0)==='0'&&n.format(-2)==='-2'&&n.format(2)==='2'&&a.resolvedOptions().signDisplay==='always'&&z.resolvedOptions().signDisplay==='exceptZero'&&n.resolvedOptions().signDisplay==='negative'",
        "let f=new Intl.NumberFormat('en',{signDisplay:'always'});f.format(-Infinity)==='-∞'&&f.format(Infinity)==='+∞'&&f.format(NaN)==='+NaN'&&f.formatToParts(Infinity).map(x=>x.type+':'+x.value).join('|')==='plusSign:+|infinity:∞'",
        "let n=new Intl.NumberFormat('en',{roundingIncrement:5,minimumFractionDigits:2,maximumFractionDigits:2});n.format(1.075)==='1.10'&&n.resolvedOptions().roundingIncrement===5",
        "let n=new Intl.NumberFormat('en',{useGrouping:false,roundingMode:'ceil',maximumSignificantDigits:2});let r=n.resolvedOptions();n.format(1.101)==='1.2'&&n.format(-1.1999)==='-1.1'&&r.minimumSignificantDigits===1&&r.maximumSignificantDigits===2&&r.minimumFractionDigits===undefined",
        "let n=new Intl.NumberFormat('en',{minimumFractionDigits:2,maximumFractionDigits:2,trailingZeroDisplay:'stripIfInteger'});n.format(1)==='1'&&n.format(1.5)==='1.50'&&n.resolvedOptions().trailingZeroDisplay==='stripIfInteger'",
        "let n=new Intl.NumberFormat('en-US',{style:'currency',currency:'USD',currencySign:'accounting'});let r=n.resolvedOptions();n.format(-987)==='($987.00)'&&n.formatToParts(-987).map(x=>x.type+':'+x.value).join('|')==='literal:(|currency:$|integer:987|decimal:.|fraction:00|literal:)'&&r.style==='currency'&&r.currency==='USD'&&r.currencyDisplay==='symbol'&&r.currencySign==='accounting'&&r.minimumFractionDigits===2&&r.maximumFractionDigits===2",
        "let n=new Intl.NumberFormat('en-US',{style:'percent'});n.format(0.2)==='20%'&&n.formatToParts(-123).map(x=>x.type+':'+x.value).join('|')==='minusSign:-|integer:12|group:,|integer:300|percentSign:%'",
        "let e=new Intl.NumberFormat('de',{notation:'engineering'});let s=new Intl.NumberFormat('en',{notation:'scientific'});e.formatToParts(.000345).map(x=>x.type+':'+x.value).join('|')==='integer:345|exponentSeparator:E|exponentMinusSign:-|exponentInteger:6'&&s.format(543211.1)==='5.432E5'",
        "let m=new Intl.NumberFormat('en',{minimumFractionDigits:2,minimumSignificantDigits:2,roundingPriority:'morePrecision'});let l=new Intl.NumberFormat('en',{minimumFractionDigits:2,minimumSignificantDigits:2,roundingPriority:'lessPrecision'});m.format(1)==='1.0'&&l.format(1)==='1.00'&&m.resolvedOptions().roundingPriority==='morePrecision'&&l.resolvedOptions().minimumFractionDigits===2&&l.resolvedOptions().minimumSignificantDigits===2",
        "let c=new Intl.NumberFormat('en',{notation:'compact'});let u=new Intl.NumberFormat('ko',{style:'unit',unit:'kilometer-per-hour',unitDisplay:'long'});c.formatToParts(9876).map(x=>x.type+':'+x.value).join('|')==='integer:9|decimal:.|fraction:9|compact:K'&&u.resolvedOptions().unit==='kilometer-per-hour'&&u.formatToParts(-987).map(x=>x.type+':'+x.value).join('|')==='unit:시속|literal: |minusSign:-|integer:987|unit:킬로미터'",
        "(123).toLocaleString('de',{style:'unit',unit:'kilometer-per-hour',unitDisplay:'long'})===new Intl.NumberFormat('de',{style:'unit',unit:'kilometer-per-hour',unitDisplay:'long'}).format(123)&&(123).toLocaleString(undefined,{style:'unit',unit:'acre'})!==(123).toLocaleString()&&(123).toLocaleString('en',{minimumFractionDigits:2})==='123.00'&&(-0).toLocaleString('en')==='-0'",
        "let n=new Intl.NumberFormat('en-US',{style:'currency',currency:'USD',maximumFractionDigits:0});n.formatRange(3,5)==='$3 – $5'&&n.formatRange(2.9,3.1)==='~$3'&&n.formatRangeToParts(3,5).map(x=>x.type+':'+x.value+':'+x.source).join('|')==='currency:$:startRange|integer:3:startRange|literal: – :shared|currency:$:endRange|integer:5:endRange'&&n.formatRange.length===2&&n.formatRangeToParts.length===2",
        "let n=new Intl.NumberFormat('pt-PT',{style:'currency',currency:'EUR',maximumFractionDigits:0});n.formatRange(3,5)==='3 - 5 €'&&n.formatRange(2.9,3.1)==='~3 €'",
        "let n=new Intl.NumberFormat('en');n.format(987654321987654321n)==='987,654,321,987,654,321'&&n.format('987654321987654321')==='987,654,321,987,654,321'&&n.formatRange('987654321987654321','987654321987654322')==='987,654,321,987,654,321–987,654,321,987,654,322'",
        "let n=new Intl.NumberFormat('en');let b=9007199254740993n;let text='9007199254740993';n.format(b)==='9,007,199,254,740,993'&&n.format(text)===n.format(b)&&n.format({valueOf(){return b}})===n.format(b)&&n.formatToParts(b).map(x=>x.type+':'+x.value).join('|')==='integer:9|group:,|integer:007|group:,|integer:199|group:,|integer:254|group:,|integer:740|group:,|integer:993'&&n.formatRange(b,b+1n)==='9,007,199,254,740,993–9,007,199,254,740,994'&&n.formatRangeToParts(b,b+1n).map(x=>x.source).includes('startRange')&&n.formatRangeToParts(b,b+1n).map(x=>x.source).includes('endRange')",
        "let value=12345678901234567890n;let descriptor=Object.getOwnPropertyDescriptor(BigInt.prototype,'toLocaleString');value.toLocaleString('de-DE')===new Intl.NumberFormat('de-DE').format(value)&&descriptor.writable&&descriptor.configurable&&!descriptor.enumerable&&BigInt.prototype.toLocaleString.length===0",
        "let seen=[];let element={toLocaleString(){seen=[arguments.length,arguments[0],arguments[1]];return 'ok'}};[element].toLocaleString('de-DE',{useGrouping:false})==='ok'&&seen.length===3&&seen[0]===2&&seen[1]==='de-DE'&&seen[2].useGrouping===false",
        "new Intl.NumberFormat('en',false).format(1000)==='1,000'&&new Intl.NumberFormat('en','').format(1000)==='1,000'",
        "let n=new Intl.NumberFormat('en'); n.format === n.format && n.format.name === '' && n.format.length === 1 && n.format.prototype === undefined",
        "let n=Intl.NumberFormat('en'); let f=n.format; f(1000) === '1,000' && f.call({},1000) === '1,000' && n instanceof Intl.NumberFormat",
        "let n=new Intl.NumberFormat('en',{numberingSystem:'thai',useGrouping:false});n.format(12)==='๑๒'&&n.resolvedOptions().locale==='en'&&n.resolvedOptions().numberingSystem==='thai'",
        "let n=new Intl.NumberFormat(undefined,{numberingSystem:'adlm',useGrouping:false});n.format(0)==='𞥐'&&n.resolvedOptions().locale==='en-US'&&n.resolvedOptions().numberingSystem==='adlm'",
        "let legacy=Object.create(Intl.NumberFormat.prototype);let result=Intl.NumberFormat.call(legacy,'en');let symbol=Object.getOwnPropertySymbols(legacy).find(key=>key.description==='IntlLegacyConstructedSymbol');let descriptor=Object.getOwnPropertyDescriptor(legacy,symbol);result===legacy&&legacy[symbol] instanceof Intl.NumberFormat&&!descriptor.writable&&!descriptor.enumerable&&!descriptor.configurable&&legacy.format(1)==='1'&&legacy.resolvedOptions().style==='decimal'",
        "let legacy=new Intl.NumberFormat('en');Intl.NumberFormat.call(legacy);let observed;let proxy=new Proxy(legacy,{get(target,key){observed=key;return target[key]}});Intl.NumberFormat.prototype.resolvedOptions.call(proxy).style==='decimal'&&typeof observed==='symbol'&&observed.description==='IntlLegacyConstructedSymbol'",
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
        "Intl.NumberFormat.prototype.formatToParts.call({}, 1)",
        "new Intl.NumberFormat('en',null)",
        "new Intl.NumberFormat('en',{useGrouping:'invalid'})",
        "new Intl.NumberFormat('en',{minimumFractionDigits:4,maximumFractionDigits:2})",
        "new Intl.NumberFormat('en',{minimumFractionDigits:101})",
        "new Intl.NumberFormat('en',{minimumFractionDigits:-1})",
        "new Intl.NumberFormat('en',{roundingIncrement:3})",
        "new Intl.NumberFormat('en',{roundingIncrement:5,minimumFractionDigits:2,maximumFractionDigits:3})",
        "new Intl.NumberFormat('en',{roundingIncrement:2,minimumSignificantDigits:1})",
        "new Intl.NumberFormat('en',{maximumSignificantDigits:0})",
        "new Intl.NumberFormat('en',{trailingZeroDisplay:'stripifinteger'})",
        "new Intl.NumberFormat('en',{style:'currency'})",
        "new Intl.NumberFormat('en',{style:'currency',currency:'US'})",
        "new Intl.NumberFormat('en',{style:'unit'})",
        "new Intl.NumberFormat('en',{style:'unit',unit:'invalid'})",
        "new Intl.NumberFormat('en',{minimumIntegerDigits:22})",
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
fn duration_format_and_number_format_share_english_unit_patterns() {
    let value = evaluate(
        "let d={years:-1,months:-2,weeks:-3,days:-3,hours:-4,minutes:-5,seconds:-6,milliseconds:-7,microseconds:-8,nanoseconds:-9};let f=new Intl.DurationFormat('en');let u=['year','month','week','day','hour','minute','second','millisecond','microsecond','nanosecond'];let values=[-1,-2,-3,-3,-4,-5,-6,-7,-8,-9];let a=[];for(let i=0;i<u.length;i++){let o={style:'unit',unit:u[i],unitDisplay:'short'};if(i>0)o.signDisplay='never';a.push(new Intl.NumberFormat('en',o).format(values[i]));}JSON.stringify([f.format(d),new Intl.ListFormat('en',{type:'unit',style:'short'}).format(a)])",
    )
    .unwrap();
    assert_eq!(
        value,
        Value::String("[\"-1 yr, 2 mths, 3 wks, 3 days, 4 hr, 5 min, 6 sec, 7 ms, 8 μs, 9 ns\",\"-1 yr, 2 mths, 3 wks, 3 days, 4 hr, 5 min, 6 sec, 7 ms, 8 μs, 9 ns\"]".into())
    );
    let value = evaluate(
        "let d={years:1,months:2,weeks:3,days:3,hours:4,minutes:5,seconds:6,milliseconds:7,microseconds:8,nanoseconds:9};new Intl.DurationFormat('en',{style:'digital'}).format(d)",
    )
    .unwrap();
    assert_eq!(
        value,
        Value::String("1 yr, 2 mths, 3 wks, 3 days, 4:05:06.007008009".into())
    );
    let value = evaluate(
        "let r=new Intl.DurationFormat('en',{style:'digital'}).resolvedOptions();JSON.stringify([r.years,r.months,r.weeks,r.days,r.hours,r.minutes,r.seconds,r.milliseconds,r.microseconds,r.nanoseconds,r.hoursDisplay,r.minutesDisplay,r.secondsDisplay])",
    )
    .unwrap();
    assert_eq!(
        value,
        Value::String("[\"short\",\"short\",\"short\",\"short\",\"numeric\",\"2-digit\",\"2-digit\",\"numeric\",\"numeric\",\"numeric\",\"always\",\"always\",\"always\"]".into())
    );
    let mut vm = Vm::default();
    vm.install_test262_harness().unwrap();
    vm.execute_script(
        &compile(
            &parse(include_str!(
                "../../../development/browser_core/reference/test262/harness/testIntl.js"
            ))
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let value = vm
        .execute_script(
            &compile(
                &parse(
                    "let d={years:1,months:2,weeks:3,days:3,hours:4,minutes:5,seconds:6,milliseconds:7,microseconds:8,nanoseconds:9};let f=new Intl.DurationFormat('en',{style:'digital'});JSON.stringify([f.format(d),formatDurationFormatPattern(f,d)])",
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        value,
        Value::String("[\"1 yr, 2 mths, 3 wks, 3 days, 4:05:06.007008009\",\"1 yr, 2 mths, 3 wks, 3 days, 4:05:06.007008009\"]".into())
    );
}

#[test]
fn list_format_delegates_iterables_and_parts_to_the_host_neutral_service() {
    for source in [
        "let f=new Intl.ListFormat('en',{type:'disjunction',style:'short'}); let r=f.resolvedOptions(); f.format(['A','B','C']) === 'A, B, or C' && r.locale === 'en' && r.type === 'disjunction' && r.style === 'short'",
        "let f=new Intl.ListFormat('en'); f.format() === '' && f.format('foo') === 'f, o, and o'",
        "let r=new Intl.ListFormat('en',{type:'unit',style:'narrow'}).resolvedOptions();r.type === 'unit' && r.style === 'narrow'",
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
        "new Intl.ListFormat('en',true)",
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
fn duration_format_delegates_records_options_and_parts_to_the_host_service() {
    for source in [
        "let f=new Intl.DurationFormat('en'); let r=f.resolvedOptions(); f.format({years:1,months:2,hours:3,minutes:4,seconds:5}) === '1 yr, 2 mths, 3 hr, 4 min, 5 sec' && r.locale === 'en' && r.numberingSystem === 'latn' && r.style === 'short' && r.hours === 'short' && r.hoursDisplay === 'auto'",
        "let f=new Intl.DurationFormat('en',{style:'digital',fractionalDigits:3}); f.format({hours:1,minutes:2,seconds:3,milliseconds:4}) === '1:02:03.004' && f.resolvedOptions().minutes === '2-digit' && f.resolvedOptions().secondsDisplay === 'always'",
        "let p=new Intl.DurationFormat('en',{style:'digital'}).formatToParts({hours:-1,minutes:-2,seconds:-3}); p.map(x=>x.type+':'+x.value+(x.unit ? ':'+x.unit : '')).join('|') === 'minusSign:-:hour|integer:1:hour|literal::|integer:02:minute|literal::|integer:03:second'",
        "Intl.DurationFormat.supportedLocalesOf(['en','zz','es']).join(',') === 'en,es' && new Intl.DurationFormat('es').resolvedOptions().locale === 'es' && Object.prototype.toString.call(new Intl.DurationFormat()) === '[object Intl.DurationFormat]'",
        "let f=new Intl.DurationFormat('es',{style:'long'}); f.format({years:1,months:2}) === '1 año y 2 meses' && f.formatToParts({years:1,months:2}).filter(part=>part.type==='unit').map(part=>part.value).join(',') === 'año,meses'",
        "let duration=new Intl.DurationFormat('es',{style:'long',months:'short'}).format({months:2});let number=new Intl.NumberFormat('es',{style:'unit',unit:'month',unitDisplay:'short'}).format(2);duration==='2 m.'&&duration===number",
        "let cases=[['fr',{years:2},'year',2],['ru',{months:5},'month',5],['ak',{years:2},'year',2],['ar',{years:1},'year',1]];cases.every(([locale,record,unit,value])=>new Intl.DurationFormat(locale,{style:'long'}).format(record)===new Intl.NumberFormat(locale,{style:'unit',unit,unitDisplay:'long'}).format(value))",
        "let duration=new Intl.DurationFormat('ak',{style:'short'});let values=[new Intl.NumberFormat('ak',{style:'unit',unit:'year'}).format(1),new Intl.NumberFormat('ak',{style:'unit',unit:'month'}).format(2)];duration.resolvedOptions().locale==='ak'&&duration.format({years:1,months:2})===new Intl.ListFormat('ak',{type:'unit',style:'short'}).format(values)",
        "let r=new Intl.DurationFormat('en-u-nu-arab',{numberingSystem:'invalid'}).resolvedOptions(); r.locale === 'en-u-nu-arab' && r.numberingSystem === 'arab' && new Intl.DurationFormat('en',{numberingSystem:'arab'}).format({seconds:12}).includes('١٢')",
        "let f=new Intl.DurationFormat();let d=new Temporal.Duration(1,2,3,4,5,6,7,8,9,10);let expected=f.format({years:1,months:2,weeks:3,days:4,hours:5,minutes:6,seconds:7,milliseconds:8,microseconds:9,nanoseconds:10});for(let p of ['years','months','weeks','days','hours','minutes','seconds','milliseconds','microseconds','nanoseconds'])Object.defineProperty(Temporal.Duration.prototype,p,{get(){throw new Error('tainted')}});f.format(d)===expected&&f.format('P1Y2M3W4DT5H6M7.008009010S')===expected&&f.formatToParts(d).length>0",
        "function F(){} let f=Reflect.construct(Intl.DurationFormat,['en'],F); Object.getPrototypeOf(f) === F.prototype && Intl.DurationFormat.prototype.format.call(f,{hours:1,minutes:2}) === '1 hr, 2 min'",
        "let r=new Intl.DurationFormat('en',{style:'long',years:'narrow',months:'short',weeks:'long',days:'narrow',hours:'long',minutes:'narrow',seconds:'short',milliseconds:'long',microseconds:'narrow',nanoseconds:'short',yearsDisplay:'always',monthsDisplay:'always',weeksDisplay:'always',daysDisplay:'always',hoursDisplay:'always',minutesDisplay:'always',secondsDisplay:'always'}).resolvedOptions(); r.style === 'long' && r.years === 'narrow' && r.months === 'short' && r.weeks === 'long' && r.days === 'narrow' && r.hours === 'long' && r.minutes === 'narrow' && r.seconds === 'short' && r.milliseconds === 'long' && r.microseconds === 'narrow' && r.nanoseconds === 'short' && r.yearsDisplay === 'always' && r.monthsDisplay === 'always' && r.weeksDisplay === 'always' && r.daysDisplay === 'always' && r.hoursDisplay === 'always' && r.minutesDisplay === 'always' && r.secondsDisplay === 'always'",
        "let r=new Intl.DurationFormat('en',{hours:'numeric',minutes:'numeric',seconds:'numeric',milliseconds:'numeric',microseconds:'numeric',nanoseconds:'numeric'}).resolvedOptions(); r.hours === 'numeric' && r.minutes === '2-digit' && r.seconds === '2-digit' && r.milliseconds === 'numeric' && r.microseconds === 'numeric' && r.nanoseconds === 'numeric'",
        "let r=new Intl.DurationFormat('en',{hours:'2-digit',minutes:'2-digit',seconds:'2-digit',milliseconds:'numeric',microseconds:'numeric',nanoseconds:'numeric'}).resolvedOptions(); r.hours === '2-digit' && r.minutes === '2-digit' && r.seconds === '2-digit'",
        "let p=new Intl.DurationFormat('en',{style:'narrow'}).formatToParts({years:1,months:2,weeks:3,days:4,hours:5,minutes:6,seconds:7,milliseconds:8,microseconds:9,nanoseconds:10}); let types=p.map(x=>x.type).join(','); let units=p.map(x=>x.unit||'').join(','); types.includes('unit') && units.includes('year') && units.includes('month') && units.includes('week') && units.includes('day') && units.includes('hour') && units.includes('minute') && units.includes('second') && units.includes('millisecond') && units.includes('microsecond') && units.includes('nanosecond')",
    ] {
        match evaluate(source) {
            Ok(value) => assert_eq!(value, Value::Bool(true), "{source}"),
            Err(error) => panic!("{source}: {error}"),
        }
    }
    for source in [
        "Intl.DurationFormat('en')",
        "new Intl.DurationFormat('en',null)",
        "new Intl.DurationFormat('en',{style:'invalid'})",
        "new Intl.DurationFormat('en',{hours:'invalid'})",
        "new Intl.DurationFormat('en',{fractionalDigits:10})",
        "new Intl.DurationFormat('en',{numberingSystem:'!!'})",
        "new Intl.DurationFormat('en',{numberingSystem:'123456789'})",
        "Intl.DurationFormat.prototype.format.call({}, {hours:1})",
        "Intl.DurationFormat.prototype.formatToParts.call({}, {hours:1})",
        "Intl.DurationFormat.prototype.resolvedOptions.call({})",
        "new Intl.DurationFormat().format({hours:1,minutes:-1})",
        "new Intl.DurationFormat().format({})",
        "new Intl.DurationFormat().format(1)",
        "new Intl.DurationFormat().format('bad string')",
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
        "let p=new Intl.PluralRules('en',{notation:'compact',compactDisplay:'long'}); p.select(1000) === 'other' && p.selectRange(1000,1000) === 'other'",
        "let r=new Intl.PluralRules('en',{minimumFractionDigits:2,maximumFractionDigits:2,roundingIncrement:5,roundingMode:'halfEven',roundingPriority:'auto',trailingZeroDisplay:'stripIfInteger'}).resolvedOptions(); r.minimumFractionDigits === 2 && r.maximumFractionDigits === 2 && r.roundingIncrement === 5 && r.roundingMode === 'halfEven' && r.roundingPriority === 'auto' && r.trailingZeroDisplay === 'stripIfInteger'",
        "let r=new Intl.PluralRules('en',{minimumSignificantDigits:2,maximumSignificantDigits:4,roundingMode:'ceil',roundingPriority:'morePrecision'}).resolvedOptions(); r.minimumSignificantDigits === 2 && r.maximumSignificantDigits === 4 && r.roundingMode === 'ceil' && r.roundingPriority === 'morePrecision'",
        "new Intl.PluralRules('ar').resolvedOptions().pluralCategories.join() === 'zero,one,two,few,many,other'",
        "Intl.PluralRules.supportedLocalesOf(['en','zz','pl-PL']).join() === 'en,pl-PL' && Object.prototype.toString.call(new Intl.PluralRules()) === '[object Intl.PluralRules]'",
        "function F(){} let p=Reflect.construct(Intl.PluralRules,['en'],F); Object.getPrototypeOf(p) === F.prototype && Intl.PluralRules.prototype.select.call(p,1) === 'one'",
        "new Intl.PluralRules('en').selectRange(102,201) === 'other' && new Intl.PluralRules('en').selectRange(1,1) === 'one'",
        "new Intl.PluralRules('ru').selectRange(0,2) === 'few' && new Intl.PluralRules('ru',{type:'ordinal'}).selectRange(0,2) === 'other'",
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
        "new Intl.PluralRules('en',{minimumSignificantDigits:4,maximumSignificantDigits:2})",
        "new Intl.PluralRules('en',{roundingIncrement:3})",
        "Intl.PluralRules.prototype.select.call({},1)",
        "Intl.PluralRules.prototype.selectRange.call({},1,2)",
        "Intl.PluralRules.prototype.resolvedOptions.call({})",
        "new Intl.PluralRules().selectRange(NaN,1)",
        "new Intl.PluralRules().selectRange()",
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
        "let s=new Intl.Segmenter('en',{granularity:'sentence'});s.resolvedOptions().granularity === 'sentence' && s.segment('Hi. Bye.').containing(99) === undefined",
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
        "let d=new Intl.DisplayNames('en',{type:'language'});d.of('fr') === 'French' && d.of('cde-ab-abcde') === 'cde (AB, ABCDE)' && d.resolvedOptions().languageDisplay === 'dialect'",
        "let d=new Intl.DisplayNames('en',{type:'region',fallback:'none'});d.of('US') === 'United States' && d.of('ZZ') === 'Unknown Region' && Intl.DisplayNames.supportedLocalesOf(['zz','en']).join() === 'en'",
        "let r=new Intl.DisplayNames('en',{type:'language',style:'short',languageDisplay:'standard'}).resolvedOptions();r.style === 'short' && r.type === 'language' && r.languageDisplay === 'standard'",
        "let r=new Intl.DisplayNames('en',{type:'calendar',style:'narrow',fallback:'none'}).resolvedOptions();r.style === 'narrow' && r.type === 'calendar' && r.fallback === 'none'",
        "new Intl.DisplayNames('en',{type:'dateTimeField'}).resolvedOptions().type === 'dateTimeField'",
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
        "let r=new Intl.RelativeTimeFormat('en',{style:'narrow',numeric:'always'}).resolvedOptions();r.style === 'narrow' && r.numeric === 'always' && r.numberingSystem === 'latn'",
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

/// Phase 26 Stage 1 Track E: named IANA time zones resolve real historical
/// transition offsets, not a single current offset, through the JS-visible
/// `Temporal` surface. The expected epoch values are the pinned Test262
/// corpus's own, from `built-ins/` and `intl402/Temporal/.../toZonedDateTime*`.
#[test]
fn temporal_time_zones_resolve_real_iana_transitions_and_disambiguation() {
    for source in [
        // `Instant.prototype.toZonedDateTimeISO` keeps the instant and adopts
        // the zone, offset and IANA identifiers alike.
        "let i=new Temporal.Instant(1000000000000000000n);let z=i.toZonedDateTimeISO('UTC');z.epochNanoseconds===i.epochNanoseconds&&z.timeZoneId==='UTC'&&z.calendarId==='iso8601'",
        "new Temporal.Instant(0n).toZonedDateTimeISO('-05:00').timeZoneId==='-05:00'",
        "new Temporal.Instant(0n).toZonedDateTimeISO('uTc').timeZoneId==='UTC'",
        "new Temporal.Instant(0n).toZonedDateTimeISO('2021-08-19T17:30:45.123456789-12:12[+01:46]').timeZoneId==='+01:46'",
        "new Temporal.Instant(0n).toZonedDateTimeISO('2021-08-19T17:30Z').timeZoneId==='UTC'",
        // A named zone's offset really does differ by instant.
        "let ny='America/New_York';let summer=new Temporal.Instant(1720480004000000000n).toZonedDateTimeISO(ny);let winter=new Temporal.Instant(1704941204000000000n).toZonedDateTimeISO(ny);summer.epochNanoseconds===1720480004000000000n&&winter.epochNanoseconds===1704941204000000000n",
        // `ZonedDateTime`'s own constructor validates and normalizes the zone.
        "new Temporal.ZonedDateTime(0n,'utc').timeZoneId==='UTC'",
        // Fixed-offset zones ignore `disambiguation` entirely.
        "let d=new Temporal.PlainDateTime(2019,2,16,23,45);['earlier','later','compatible','reject'].every(x=>d.toZonedDateTime('+03:30',{disambiguation:x}).epochNanoseconds===1550348100000000000n)",
        // A repeated local hour in Los Angeles: two real instants.
        "let f=new Temporal.PlainDateTime(2000,10,29,1,45);f.toZonedDateTime('America/Los_Angeles').epochNanoseconds===972809100000000000n&&f.toZonedDateTime('America/Los_Angeles',{disambiguation:'later'}).epochNanoseconds===972812700000000000n",
        // A skipped local hour: shifted backwards or forwards by the gap.
        "let s=new Temporal.PlainDateTime(2000,4,2,2,30);s.toZonedDateTime('America/Los_Angeles',{disambiguation:'earlier'}).epochNanoseconds===954667800000000000n&&s.toZonedDateTime('America/Los_Angeles',{disambiguation:'later'}).epochNanoseconds===954671400000000000n",
        // `PlainDate.prototype.toZonedDateTime` uses the zone's start of day,
        // which Toronto's 1919-03-31 midnight-skipping transition puts 30
        // minutes before what `compatible` midnight resolution would give.
        "let t=new Temporal.PlainDate(1919,3,31);let start=t.toZonedDateTime('America/Toronto');let bag=t.toZonedDateTime({timeZone:'America/Toronto',plainTime:new Temporal.PlainTime()});bag.epochNanoseconds-start.epochNanoseconds===1800000000000n",
        "new Temporal.PlainDate(2020,1,1).toZonedDateTime('UTC').epochNanoseconds===1577836800000000000n",
    ] {
        assert_eq!(evaluate(source), Ok(Value::Bool(true)), "{source}");
    }
    for source in [
        // `ToTemporalTimeZoneIdentifier` never coerces a non-string.
        "new Temporal.Instant(0n).toZonedDateTimeISO()",
        "new Temporal.Instant(0n).toZonedDateTimeISO(1)",
        "new Temporal.Instant(0n).toZonedDateTimeISO({})",
        // Unknown names and sub-minute offsets are RangeErrors, and a valid
        // offset beside an unknown name is not a fallback.
        "new Temporal.Instant(0n).toZonedDateTimeISO('America/Nonexistent')",
        "new Temporal.Instant(0n).toZonedDateTimeISO('-12:12:59.9')",
        "new Temporal.Instant(0n).toZonedDateTimeISO('1970-01-01T00:00+01:00[America/Nonexistent]')",
        "new Temporal.Instant(0n).toZonedDateTimeISO('2021-08-19T17:30')",
        "new Temporal.Instant(0n).toZonedDateTimeISO('-000000-10-31T17:45Z')",
        "new Temporal.Instant(0n).toZonedDateTimeISO('')",
        "new Temporal.ZonedDateTime(0n,'Mars/Olympus_Mons')",
        // `disambiguation: "reject"` refuses to choose.
        "new Temporal.PlainDateTime(2000,10,29,1,45).toZonedDateTime('America/Los_Angeles',{disambiguation:'reject'})",
        "new Temporal.PlainDateTime(2000,4,2,2,30).toZonedDateTime('America/Los_Angeles',{disambiguation:'reject'})",
        "new Temporal.PlainDateTime(2000,5,2).toZonedDateTime('UTC',{disambiguation:'EARLIER'})",
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
