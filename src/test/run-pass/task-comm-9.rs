// Copyright 2012 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// xfail-fast

extern mod extra;

use std::comm;
use std::task;

pub fn main() { test00(); }

fn test00_start(c: &comm::Chan<int>, number_of_messages: int) {
    let mut i: int = 0;
    while i < number_of_messages { c.send(i + 0); i += 1; }
}

fn test00() {
    let r: int = 0;
    let mut sum: int = 0;
    let (p, ch) = comm::stream();
    let number_of_messages: int = 10;

    let mut builder = task::task();
    let result = builder.future_result();
    do builder.spawn {
        test00_start(&ch, number_of_messages);
    }

    let mut i: int = 0;
    while i < number_of_messages {
        sum += p.recv();
        info!("{:?}", r);
        i += 1;
    }

    result.recv();

    assert_eq!(sum, number_of_messages * (number_of_messages - 1) / 2);
}
