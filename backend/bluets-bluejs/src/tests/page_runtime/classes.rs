// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Classes page-realm regressions.

use super::*;

const CLASS_SOURCE: &str = "\
class Base {
  constructor(n: number) {}
  add(a: number): number { return a + 1; }
  static id(a: number): number { return a; }
  twice(a: number): number { return this.add(a) + this.add(a); }
}
class Derived extends Base {
  constructor() { super(1); }
  add(a: number): number { return super.add(a) * 10; }
  static id(a: number): number { return super.id(a) + 100; }
}
const d = new Derived();
d.twice(2) + Derived.id(5);";

#[test]
fn direct_page_classes_construct_call_methods_inherit_and_use_super() {
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, CLASS_SOURCE)]),
        class_options(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    // Derived.add(2) is (2 + 1) * 10, called twice through `this`, plus
    // Derived.id(5) is 5 + 100; Node runs the same program to 165.
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(165.0)
    );
}

#[test]
fn direct_page_class_fields_initialize_in_order_and_run_like_node() {
    let source = "class Counter { \
        count: number = 1; static total: number = 10; readonly id: number = 7; tag?: string; \
        constructor() { this.count = this.count + 1; } \
        step(): number { this.count = this.count + this.id; return this.count; } } \
        class Sub extends Counter { extra: number = 5; static made: number = Counter.total + 1; } \
        const s = new Sub(); \
        s.step() + s.extra + Sub.made + Counter.total + (s.tag === undefined ? 1000 : 0);";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        class_options(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    // Node runs the same program (types erased) to 1035.
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(1035.0)
    );
}

#[test]
fn direct_page_class_field_initializers_map_to_the_original_class() {
    let source = "class Holder { a: number = 1; static s: number = 2; m(): number { return this.a; } } new Holder().m() + Holder.s;";
    let class_end = source.find(" new Holder").unwrap();
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        class_options(),
    )
    .unwrap();
    let mut runtime = bluejs::BlueJsPageRuntime::default();
    runtime.open_realm(7, origin()).unwrap();
    let mut debug = DirectDebugRegistry::default();
    let attachment = artifact
        .attach_debug_in_page_realm(&mut runtime, 7, &origin(), &mut debug)
        .unwrap();
    attachment
        .safe_point_map
        .validate_against(runtime.program_registry(), attachment.handle)
        .unwrap();
    // Each closure the class produced is mapped to the whole class: the
    // synthesized constructor, the method, and the instance and static field
    // initializers.
    let class_units: std::collections::BTreeSet<_> = attachment
        .safe_point_map
        .entries
        .iter()
        .filter(|entry| {
            entry.code_unit.ordinal() > 0
                && attachment
                    .safe_point_map
                    .source_span_for_safe_point(entry.code_unit.ordinal(), entry.bytecode_offset)
                    .is_some_and(|mapped| (mapped.start_byte, mapped.end_byte) == (0, class_end))
        })
        .map(|entry| entry.code_unit.ordinal())
        .collect();
    assert_eq!(
        class_units.len(),
        4,
        "constructor, method, instance initializer, static initializer: {class_units:?}"
    );
    // The script still runs to the value Node computes (1 + 2).
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(8, origin()).unwrap();
    let plain = owner.attach_script(&artifact, 8, &origin()).unwrap();
    assert_eq!(
        owner.execute_program(8, &plain).unwrap(),
        bluejs::Value::Number(3.0)
    );
}

#[test]
fn direct_page_private_and_protected_members_run_like_node() {
    let source = "class Vault { \
        private balance: number = 0; protected owner: number = 2; public label: number = 3; \
        private static count: number = 0; protected static prefix: number = 100; \
        private bump(step: number): number { this.balance = this.balance + step; return this.balance; } \
        protected who(): number { return this.owner + Vault.prefix; } \
        public open(): number { Vault.count = Vault.count + 1; return this.bump(1) + Vault.count; } \
        protected constructor(seed: number) { this.balance = seed; } \
        static make(seed: number): Vault { return new Vault(seed); } } \
        class Branch extends Vault { constructor() { super(10); } \
        describe(): number { return this.who() + this.label + super.who(); } } \
        const branch = new Branch(); \
        branch.open() + branch.describe() + Vault.make(5).open();";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        class_options(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    // Node runs the same program (accessibility erased) to 227.
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(227.0)
    );
}

#[test]
fn direct_page_parameter_properties_assign_after_super_like_node() {
    let source = "class Base { z: number = 5; \
        constructor(public x: number, private y: number, protected readonly w: number = 3) { \
          this.z = this.z + this.x; } \
        sum(): number { return this.x + this.y + this.w + this.z; } } \
        class Derived extends Base { r: number = 9; \
        constructor(public q: number, private hidden: number) { \
          const seed: number = q + 1; super(seed, 2); \
          this.r = this.r + this.q + this.hidden; } \
        total(): number { return this.sum() + this.r + this.hidden; } } \
        const b = new Base(1, 2); const d = new Derived(4, 5); \
        b.sum() + d.total() + d.q + d.x;";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        class_options(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    // Node runs the same program, properties written out by hand, to 64.
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(64.0)
    );
}

#[test]
fn direct_page_accessors_get_set_static_and_inherit_like_node() {
    let source = "class Temperature { \
        private _celsius: number = 0; static made: number = 0; \
        get celsius(): number { return this._celsius; } \
        set celsius(value: number) { this._celsius = value; } \
        get fahrenheit(): number { return this._celsius * 2 + 32; } \
        set kelvin(value: number) { this._celsius = value - 273; } \
        static get unit(): number { Temperature.made = Temperature.made + 1; return Temperature.made; } \
        constructor(start: number) { this.celsius = start; } } \
        class Warm extends Temperature { \
        get celsius(): number { return super.celsius + 1; } \
        set celsius(value: number) { super.celsius = value; } } \
        const t = new Temperature(10); t.celsius = t.celsius + 5; t.kelvin = 300; \
        const w = new Warm(20); \
        t.celsius + t.fahrenheit + Temperature.unit + Temperature.unit + w.celsius;";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        class_options(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    // Node runs the same program (annotations erased) to 27 + 86 + 1 + 2 + 21.
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(137.0)
    );
}

#[test]
fn direct_page_private_names_fields_methods_accessors_and_brand_checks_run_like_node() {
    let source = "class Counter { \
        #count: number = 0; static #made: number = 0; label: number = 1; \
        #bump(step: number): number { this.#count = this.#count + step; return this.#count; } \
        get #double(): number { return this.#count * 2; } \
        set #double(value: number) { this.#count = value / 2; } \
        static #make(): Counter { Counter.#made = Counter.#made + 1; return new Counter(); } \
        static has(value: any): boolean { return #count in value; } \
        run(other: Counter): number { this.#double = 8; \
          return this.#bump(1) + other.#count + this.#double + Counter.#make().#count; } \
        static made(): number { return Counter.#made; } } \
        class Sub extends Counter { #count: number = 100; sub(): number { return this.#count; } } \
        const c = new Counter(); \
        c.run(new Counter()) + Counter.made() + new Sub().sub() + (Counter.has(c) ? 1000 : 0) + (Counter.has({}) ? 10000 : 0);";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        class_options(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    // Node runs the same program (annotations erased) to 1116.
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(1116.0)
    );
}

#[test]
fn direct_page_static_blocks_and_fields_initialize_in_source_order_like_node() {
    let source = "class Order { \
        static log: number = 0; \
        static a: number = Order.note(1); \
        static { Order.note(10); const local: number = this.a + 100; Order.log = Order.log + local; } \
        static b: number = Order.note(2); \
        static #hidden: number = Order.note(3); \
        static { Order.log = Order.log * 2 + Order.#hidden + this.b; } \
        instance: number = Order.note(4); \
        static note(step: number): number { Order.log = Order.log * 10 + step; return step; } } \
        class Child extends Order { static extra: number = Order.a + 1; \
        static { Order.log = Order.log + this.extra + super.a; } } \
        const before: number = Order.log; new Order(); new Child(); \
        before + Order.log;";
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        class_options(),
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    // Node runs the same program (annotations erased) to 2449698.
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(2449698.0)
    );
}

#[test]
fn direct_page_assign_semantics_lowers_fields_like_typescript_and_node() {
    // With `useDefineForClassFields: false` an instance field is a constructor
    // assignment (after `super(..)`), a static field a static block, and a field
    // with no initializer does not exist. The constructor is written before
    // the fields and a class with no constructor gets one.
    let source = "class Base { \
        constructor(public p: number) { this.p = this.p + 1; } \
        x: number = 1; y?: number; static s: number = 10; \
        static { Base.s = Base.s + 1; } } \
        class Derived extends Base { z: number = this.x + 5; static t: number = Base.s + 1; \
        constructor() { super(2); this.z = this.z + 100; } } \
        class Plain extends Base { w: number = this.x + this.p; } \
        const d = new Derived(); const p = new Plain(7); \
        d.p * 1000000 + d.x * 100000 + d.z * 100 + Derived.t + (('y' in d) ? 1 : 0) + p.w * 10000000 + Base.s;";
    let mut options = class_options();
    options.use_define_for_class_fields = Some(false);
    let artifact = compile_direct_script(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        options,
    )
    .unwrap();
    let mut owner = DirectPageRealmOwner::default();
    owner.open_realm(7, origin()).unwrap();
    let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
    // Node runs the assign-semantics program (`this.x = 1` and so on, written
    // out by hand) to 93110623.
    assert_eq!(
        owner.execute_program(7, &attachment).unwrap(),
        bluejs::Value::Number(93110623.0)
    );
}

#[test]
fn direct_page_define_semantics_keeps_the_field_that_assign_semantics_drops() {
    // The same program as the assign-semantics test: only whether `y`, a field
    // with no initializer, exists on the instance differs (+1 in the sum), and
    // `this.p = p` for a parameter property still runs before the constructor
    // body. Node runs the define form (native fields) to 93110624.
    let source = "class Base { \
        constructor(public p: number) { this.p = this.p + 1; } \
        x: number = 1; y?: number; static s: number = 10; \
        static { Base.s = Base.s + 1; } } \
        class Derived extends Base { z: number = this.x + 5; static t: number = Base.s + 1; \
        constructor() { super(2); this.z = this.z + 100; } } \
        class Plain extends Base { w: number = this.x + this.p; } \
        const d = new Derived(); const p = new Plain(7); \
        d.p * 1000000 + d.x * 100000 + d.z * 100 + Derived.t + (('y' in d) ? 1 : 0) + p.w * 10000000 + Base.s;";
    for (define, expected) in [(true, 93110624.0), (false, 93110623.0)] {
        let mut options = class_options();
        options.use_define_for_class_fields = Some(define);
        let artifact = compile_direct_script(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            options,
        )
        .unwrap();
        let mut owner = DirectPageRealmOwner::default();
        owner.open_realm(7, origin()).unwrap();
        let attachment = owner.attach_script(&artifact, 7, &origin()).unwrap();
        assert_eq!(
            owner.execute_program(7, &attachment).unwrap(),
            bluejs::Value::Number(expected),
            "define = {define}"
        );
    }
}
