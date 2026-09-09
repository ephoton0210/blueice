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
