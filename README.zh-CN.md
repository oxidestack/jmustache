# jmustache

<!-- languages:begin -->
[English](README.md) · [Русский](README.ru.md) · **中文** · [Español](README.es.md)
<!-- languages:end -->
<!-- translation of README.md: 2e3220f3894b -->

[JMustache](https://github.com/samskivert/jmustache) 1.16（`com.samskivert.mustache`，Java 的 Mustache 引擎）的 Rust
移植，忠实于渲染输出：同样的模板和数据渲染出同样的字节，错误也一样。

这是一个非官方移植，并非由 JMustache 项目或其作者 Michael Bayne 制作、认可或支持。JMustache 采用 BSD-3-Clause
许可，本 crate 也是如此（[LICENSE](LICENSE)、[NOTICE](NOTICE)）。

状态：尚未发布。本 crate 还没有发布到 crates.io；在此之前，请从本仓库的本地检出使用它。

## 简介

JMustache 的全部功能：`Compiler` 的每个选项（standards mode、strict sections、null 值与默认值、把空字符串和零视为
假值、格式化器、转义器、模板加载器、collector、分隔符），带 standalone 缩进的 partial，模板继承
（`{{< parent}}` / `{{$ block}}`），带 `Template.Fragment` API 的 lambda（execute、带上下文 execute、execute
另一个模板、上下文查找、decompile），可反转的 lambda，自定义上下文和 visitor。Java 对象是
`java_compat::lang::JValue`；能描述自身类的对象（`JObject::reflection`）的解析方式与 `DefaultCollector` 完全相同，
包括 access coercion。

它是为 [mock-server](https://github.com/oxidestack/mock-server) 而写的。mock-server 是 MockServer 的 Rust 重新实现，
要求 MockServer 的 Mustache 响应模板输出逐字节一致。任何需要在 Rust 中获得 JMustache 行为（standards mode、strict
sections、lambda、模板继承）的人都可以单独使用它。

## 安装

crate 名为 `jmustache`。它尚未发布到 crates.io；发布之后：

```toml
[dependencies]
jmustache = "0.1"                   # not published yet
java-compat-rs = "0.1"              # the Java values a template sees; not published yet
```

在此之前，请依赖本地检出：`jmustache = { path = "../jmustache" }`，并把
[java-compat-rs](https://github.com/oxidestack/java-compat-rs) 检出到 `../java-compat-rs`。

## 快速开始

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

`Compiler` 的构建方法与 JMustache 相同（`standards_mode`、`strict_sections`、`default_value`、`with_loader`、
`with_delims` 等）。crate 发布后，API 参考将位于 docs.rs；在本地检出中可用 `cargo doc --open` 生成。

## 指南

[升级](docs/i18n/zh/guide/upgrading.md)：每个版本对使用本 crate 的代码有哪些要求。仓库公开后，本 README 和这些指南也会以
站点形式提供：`https://oxidestack.github.io/jmustache/`。

## 构建与测试

`upstream/` 是 JMustache 的原始源码，一个固定在所移植版本上的只读 git 子模块：

```sh
git clone --recursive https://github.com/oxidestack/jmustache.git
git clone https://github.com/oxidestack/java-compat-rs.git   # the JDK's semantics in Rust, beside it
cd jmustache
cargo test
```

测试是 JMustache 自己的测试，逐个移植：`tests/mustache.rs`（`SharedTests` 和 `MustacheTest`）、`tests/spec.rs`
（`OfficialSpecTest` 和 `CustomSpecTest`，读取 `upstream/` 中的 YAML 规范）以及 `tests/thread_safety.rs`
（`ThreadSafetyTest`）。本 crate 与 JMustache 不一致时，以 JMustache 为准。

`make lint` 运行 clippy，`make doc` 构建 API 文档并把 rustdoc 的警告视为错误，`make deny` 对照 CNCF 允许的许可证列表检查
许可证，`make public-check` 是每次 push 都要通过的检查，`make site` 在固定的 node 容器中构建文档站点（`site/README.md`）。

## 获取帮助

关于本 crate 如何工作的问题，可以先问 [DeepWiki](https://deepwiki.com/oxidestack/jmustache)：它根据本仓库的代码和文档
作答；若它与文档不一致，以文档为准。它解决不了的问题请发到
[Discussions](https://github.com/oxidestack/jmustache/discussions)，缺陷请提交到
[issues](https://github.com/oxidestack/jmustache/issues)。 <!-- these three links need the public repository; it is not published yet -->

## 参与贡献

参见 [CONTRIBUTING.md](CONTRIBUTING.md)；项目治理见 [GOVERNANCE.md](GOVERNANCE.md)，漏洞报告请按
[SECURITY.md](SECURITY.md) 的说明提交。

## 许可证

代码采用 BSD 3-Clause 许可证（[LICENSE](LICENSE)，`BSD-3-Clause`），与 JMustache 相同。文档（本 README 及 `docs/`
下的指南）采用 [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/)
（`CC-BY-4.0`）许可；其法律文本见 [LICENSE-docs](LICENSE-docs)。
