[![Build status](https://travis-ci.org/brandonedens/rust-littlefs.svg?branch=master)](https://travis-ci.org/brandonedens/rust-littlefs)
[![littlefs crates.io](https://img.shields.io/crates/v/littlefs.svg)](https://crates.io/crates/littlefs)
[![littlefs-sys crates.io](https://img.shields.io/crates/v/littlefs-sys.svg)](https://crates.io/crates/littlefs-sys)

# `rust-littlefs`

> Rust wrapper around the [Little Filesystem](https://github.com/ARMmbed/littlefs).

## Description

Software is divided into two pieces:

- littlefs-sys: Crate the builds upstream LittleFS C software and makes bindings available
- littlefs: a Rust wrapper around the existing C interface

Upstream LittleFS version is currently tag v1.7.0.

## License

littlefs is licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

littlefs-sys is licensed under:

- BSD 3-Clause

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
