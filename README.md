# jmustache

<!-- languages:begin -->
**English** · [Русский](README.ru.md) · [中文](README.zh-CN.md) · [Español](README.es.md)
<!-- languages:end -->

A Rust port of [JMustache](https://github.com/samskivert/jmustache) 1.16 (`com.samskivert.mustache`), the Mustache
engine for Java, faithful to its rendering output: the same template and data render to the same bytes, errors
included.

This is an unofficial port. It is not made, endorsed or supported by the JMustache project or its author, Michael
Bayne. JMustache is BSD-3-Clause; so is this crate ([LICENSE](LICENSE), [NOTICE](NOTICE)).

Status: pre-release. The crate is not published on crates.io yet; until it is, it is used from a checkout of this
repository.

## What it is

All of JMustache: every `Compiler` option (standards mode, strict sections, null/default values, falsey empty
strings and zeros, formatters, escapers, template loaders, collectors, delimiters), partials with standalone
indentation, template inheritance (`{{< parent}}` / `{{$ block}}`), lambdas with the `Template.Fragment` API
(execute, execute with a context, execute another template, context lookup, decompile), invertible lambdas,
custom contexts and visitors. Java objects are `java_compat::lang::JValue`s; objects that describe their class
(`JObject::reflection`) are resolved exactly as `DefaultCollector` resolves them, access coercion included.

It exists for [mock-server](https://github.com/oxidestack/mock-server), a Rust reimplementation of MockServer, which
needs the output of MockServer's Mustache response templates to be byte-identical. Anyone who needs JMustache's
behaviour (its standards mode, strict sections, lambdas, template inheritance) in Rust can use it on its own.

## Install

The crate is `jmustache`. It is not published on crates.io yet; once it is:

```toml
[dependencies]
jmustache = "0.1"                   # not published yet
java-compat-rs = "0.1"              # the Java values a template sees; not published yet
```

Until then, depend on a checkout: `jmustache = { path = "../jmustache" }`, with
[java-compat-rs](https://github.com/oxidestack/java-compat-rs) checked out at `../java-compat-rs`.

## Quick start

```rust
use java_compat::lang::{JList, JMap, JValue, ListKind, MapKind};
use jmustache::Compiler;

fn main() -> Result<(), jmustache::MustacheError> {
    let template = Compiler::new().compile("Hello {{name}}!{{#items}} {{.}}{{/items}}")?;
    let data = JValue::Map(JMap::from_entries(MapKind::Hash, [
        (JValue::str("name"), JValue::str("world")),
        (JValue::str("items"), JValue::List(JList::new(ListKind::ArrayList, vec![JValue::Int(1), JValue::Int(2)]))),
    ]));
    assert_eq!(template.execute(data)?, "Hello world! 1 2");
    Ok(())
}
```

`Compiler`'s builder methods are JMustache's (`standards_mode`, `strict_sections`, `default_value`, `with_loader`,
`with_delims`, …). The API reference will be on docs.rs once the crate is published; `cargo doc --open` builds it from
a checkout.

## Guides

[Upgrading](docs/guide/upgrading.md): what each release asks of code that uses the crate. This README and the guides
are also a site, `https://oxidestack.github.io/jmustache/`, once the repository is public.

## Build and test

`upstream/` is the original JMustache source, a git submodule pinned to the ported release and read-only:

```sh
git clone --recursive https://github.com/oxidestack/jmustache.git
git clone https://github.com/oxidestack/java-compat-rs.git   # the JDK's semantics in Rust, beside it
cd jmustache
cargo test
```

The tests are JMustache's own, ported test for test: `tests/mustache.rs` (`SharedTests` and `MustacheTest`),
`tests/spec.rs` (`OfficialSpecTest` and `CustomSpecTest`, read from the YAML specs in `upstream/`) and
`tests/thread_safety.rs` (`ThreadSafetyTest`). Where this crate and JMustache disagree, JMustache is right.

`make lint` runs clippy, `make doc` builds the API documentation with rustdoc's warnings as errors, `make deny` checks
the licences against the CNCF's allowed list, `make public-check` is the gate every push passes, and `make site`
builds the documentation site (`site/README.md`) in a pinned node container.

## Getting help

A question about how the crate works can go to [DeepWiki](https://deepwiki.com/oxidestack/jmustache) first: it
answers from this repository's code and documentation, and where it disagrees with the documentation, the
documentation is right. Questions it does not settle go to
[Discussions](https://github.com/oxidestack/jmustache/discussions), bugs to
[issues](https://github.com/oxidestack/jmustache/issues). <!-- these three links need the public repository; it is not published yet -->

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md); the project's governance is in [GOVERNANCE.md](GOVERNANCE.md), vulnerability
reports go as [SECURITY.md](SECURITY.md) says.

## Licence

The code is licensed under the BSD 3-Clause licence ([LICENSE](LICENSE), `BSD-3-Clause`), as JMustache is. The
documentation — this README and the guides under `docs/` — is licensed under the
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/) licence
(`CC-BY-4.0`); its legal text is in [LICENSE-docs](LICENSE-docs).
