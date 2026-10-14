// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: {log(value: unknown): void};

function snapshot(completion: string, closing: string) {
  let gets = 0;
  let calls = 0;
  const iterator: any = {
    next() { return {value: 42, done: false}; },
    get return() {
      gets++;
      if (closing === "getter-throw") throw "get-close";
      if (closing === "absent") return undefined;
      return function () {
        calls++;
        if (closing === "call-throw") throw "call-close";
        if (closing === "primitive") return 0;
        return {value: 0, done: true};
      };
    },
    [Symbol.iterator]() { return this; }
  };
  function work() {
    for (const value of iterator) {
      if (completion === "throw") throw "body";
      if (completion === "return") return "returned";
      break;
    }
    return "break";
  }
  let outcome;
  try { outcome = work(); }
  catch (error) { outcome = error instanceof TypeError ? "type" : String(error); }
  return [completion, closing, outcome, gets, calls].join("|");
}

for (const completion of ["break", "return", "throw"]) {
  for (const closing of ["getter-throw", "call-throw", "primitive", "object", "absent"]) {
    console.log(snapshot(completion, closing));
  }
}

function beforeStartReturning() {
  let entered = 0;
  function* sequence() { entered++; yield 1; }
  const iterator: any = sequence();
  const returned = iterator.return(42);
  const next = iterator.next();
  return ["start-return", entered, returned.value, returned.done, next.value, next.done].join("|");
}

function beforeStartThrowing() {
  let entered = 0;
  function* sequence() { entered++; yield 1; }
  const iterator: any = sequence();
  let outcome;
  try { iterator.throw("body"); }
  catch (error) { outcome = String(error); }
  const next = iterator.next();
  return ["start-throw", entered, outcome, next.value, next.done].join("|");
}

function reentrantNext() {
  let iterator: any;
  function* sequence() {
    try { iterator.next(); }
    catch (error) { yield error instanceof TypeError ? "type" : "other"; }
  }
  iterator = sequence();
  const first = iterator.next();
  const second = iterator.next();
  return ["reentrant", first.value, first.done, second.value, second.done].join("|");
}

function completedControls() {
  function* sequence() { return 20; }
  const iterator: any = sequence();
  const first = iterator.next();
  const returned = iterator.return(42);
  let outcome;
  try { iterator.throw("body"); }
  catch (error) { outcome = String(error); }
  const next = iterator.next();
  return ["completed", first.value, returned.value, returned.done, outcome, next.value, next.done].join("|");
}

console.log(beforeStartReturning());
console.log(beforeStartThrowing());
console.log(reentrantNext());
console.log(completedControls());

async function asyncSnapshot(completion: string, closing: string) {
  let gets = 0;
  let calls = 0;
  let settled = 0;
  const iterator: any = {
    next() { return Promise.resolve({value: 42, done: false}); },
    get return() {
      gets++;
      if (closing === "getter-throw") throw "get-close";
      if (closing === "absent") return undefined;
      return function () {
        calls++;
        if (closing === "call-throw") throw "call-close";
        if (closing === "reject") {
          return Promise.reject("await-close").catch(error => {
            settled++;
            throw error;
          });
        }
        return Promise.resolve(closing === "primitive" ? 0 : {value: 0, done: true})
          .then(result => { settled++; return result; });
      };
    },
    [Symbol.asyncIterator]() { return this; }
  };
  async function work() {
    for await (const value of iterator) {
      if (completion === "throw") throw "body";
      if (completion === "return") return "returned";
      break;
    }
    return "break";
  }
  let outcome;
  try { outcome = await work(); }
  catch (error) { outcome = error instanceof TypeError ? "type" : String(error); }
  return ["async", completion, closing, outcome, gets, calls, settled].join("|");
}

async function runAsync() {
  for (const completion of ["break", "return", "throw"]) {
    for (const closing of ["getter-throw", "call-throw", "reject", "primitive", "object", "absent"]) {
      console.log(await asyncSnapshot(completion, closing));
    }
  }
}

void runAsync();
export {};
