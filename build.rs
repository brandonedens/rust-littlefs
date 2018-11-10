// Copyright 2018 by Brandon Edens.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//
// Author: Brandon Edens <brandonedens@gmail.com>
// Date: 2018-11-10

fn main() {
    cc::Build::new()
        .file("lfs.c")
        .compile("lfs-sys")
}

