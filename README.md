# Rapidus

[![CircleCI](https://circleci.com/gh/maekawatoshiki/rapidus.svg?style=shield)](https://circleci.com/gh/maekawatoshiki/rapidus)
[![codecov](https://codecov.io/gh/maekawatoshiki/rapidus/branch/master/graph/badge.svg)](https://codecov.io/gh/maekawatoshiki/rapidus)
[![](http://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

A toy JavaScript engine (now aiming for ES5)

# Features 

- Small
- Partly support for Tracing-JIT compiling
- Simple REPL 

# Building from Source

## Building on Linux

1. Install Rust

  Run the command below and follow the onscreen instructions. 

```sh
curl https://sh.rustup.rs -sSf | sh
```

2. Use Rust Nightly

```sh
rustup override set nightly
```

3. Install dependencies
  - LLVM 6.0
  - (Other packages as necessary...)

```sh
# e.g. Ubuntu or Debian
apt-get install llvm-6.0
```

4. Test 

```sh
cargo test
```

- If the compilation failed because of LLVM-related errors, the following command may help.

```sh
ln -sf /usr/bin/llvm-config-6.0 /usr/bin/llvm-config
```

5. Build

```sh
cargo run --release
```

6. Run

```sh
cargo run --release example/XXX.js
```

## Building on other platforms

I don't know.

- ~~tips: If you are using macOS, you cannot use llvm installed with ``brew``. You should use macports or docker instead.~~ Now it works!

## Use DLLs written in Rust

**THIS FEATURE IS EXPERIMENTAL**



1. Make a cargo project in the directory rapidus' directory is located

```sh
$ cargo new hello --lib
$ ls
rapidus hello
```

2. Edit ``Cargo.toml``

```sh
$ cd hello
$ <YOUR EDITOR> Cargo.toml
```

```toml
# Add the followings to Cargo.toml

[dependencies]
rapidus = { path = "../rapidus" }
# other dependencies if you want...

[lib]
name = "hello"
crate_type = ["cdylib"] # try 'dylib' if it doesn't work.
```

3. Edit ``src/lib.rs``

```sh
$ <YOUR EDITOR> src/lib.rs
```

```rust
// src/lib.rs

#[macro_use]
extern crate rapidus;
use rapidus::{
   gc,
   vm::{callobj::CallObject, error::RuntimeError, value::*, vm::VM},
};

#[no_mangle]
fn initialize(vm: &mut VM, _: &Vec<Value>, _: CallObjectRef) -> Result<(), RuntimeError> {
    // make_object!() is useful
    let module_exports = make_object!(
        greet:   Value::default_builtin_function(greet),
        message: Value::String("hello".to_string())
    );

    vm.set_return_value(module_exports); // We have to return module.exports

    Ok(())
}

#[no_mangle]
fn greet(vm: &mut VM, _: &Vec<Value>, _: CallObjectRef) -> Result<(), RuntimeError> {
    println!("Hello World from Rust DLL!");

    vm.set_return_value(Value::Undefined); // Remember to return a value you want

    Ok(())
}
```

4. Let's build

```sh
$ cargo build # --release as necessary
```

5. Copy the generated DLL to rapidus' directory

```sh
$ cp ./target/debug/libhello.(so|dll|dylib) ../rapidus
$ cd ../rapidus
$ ls
libhello.(so|dll|dylib) etc...
```

6. You're ready to use it from rapidus. Let's try from REPL.

```sh
$ cargo run
> var mod = require('hello')
> mod.greet()
Hello World from Rust DLL!
> mod.message
'hello'
```

7. Now everything can be possible from Rust!

## Use Dockerfile

- Docker image: uint256/rapidus

```sh
$ docker build -t rapidus:0.1.1 .
$ docker run -it rapidus:0.1.1
```
