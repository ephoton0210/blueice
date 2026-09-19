use blueice_bluejs::{compile, parse, Value, Vm};
#[test]
fn probe() {
    let path = std::env::var("PROBE").unwrap();
    let source = std::fs::read_to_string(path).unwrap();
    let r = Vm::default().execute(&compile(&parse(&source).unwrap()).unwrap());
    match r {
        Ok(Value::String(s)) => println!("PROBE-OK\n{}", s.to_utf8().unwrap()),
        Ok(v) => println!("PROBE-OK {v:?}"),
        Err(e) => println!("PROBE-ERR {e:?}"),
    }
}
