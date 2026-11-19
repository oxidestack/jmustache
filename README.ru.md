# jmustache

<!-- languages:begin -->
[English](README.md) · **Русский** · [中文](README.zh-CN.md) · [Español](README.es.md)
<!-- languages:end -->
<!-- translation of README.md: 2e3220f3894b -->

Порт [JMustache](https://github.com/samskivert/jmustache) 1.16 (`com.samskivert.mustache`), движка Mustache для
Java, на Rust, верный результату рендеринга: тот же шаблон и те же данные дают те же байты, включая ошибки.

Это неофициальный порт. Он не создан, не одобрен и не поддерживается проектом JMustache или его автором, Michael
Bayne. JMustache распространяется по лицензии BSD-3-Clause; этот крейт — тоже ([LICENSE](LICENSE), [NOTICE](NOTICE)).

Статус: до первого выпуска. Крейт ещё не опубликован на crates.io; пока этого не произошло, его используют из
локальной копии этого репозитория.

## Что это

Весь JMustache: все опции `Compiler` (standards mode, strict sections, значения null и по умолчанию, ложность пустых
строк и нулей, форматтеры, экранировщики, загрузчики шаблонов, коллекторы, разделители), partials с отступами
standalone-строк, наследование шаблонов (`{{< parent}}` / `{{$ block}}`), лямбды с API `Template.Fragment` (execute,
execute с контекстом, execute другого шаблона, поиск в контексте, decompile), инвертируемые лямбды, собственные
контексты и visitors. Объекты Java — это `java_compat::lang::JValue`; объекты, описывающие свой класс
(`JObject::reflection`), разрешаются в точности так, как их разрешает `DefaultCollector`, включая access coercion.

Крейт создан для [mock-server](https://github.com/oxidestack/mock-server) — реализации MockServer на Rust, которой
нужно, чтобы вывод шаблонов ответов Mustache совпадал с выводом MockServer байт в байт. Любой, кому нужно поведение
JMustache (его standards mode, strict sections, лямбды, наследование шаблонов) в Rust, может использовать крейт сам по
себе.

## Установка

Крейт называется `jmustache`. На crates.io он ещё не опубликован; после публикации:

```toml
[dependencies]
jmustache = "0.1"                   # not published yet
java-compat-rs = "0.1"              # the Java values a template sees; not published yet
```

До тех пор зависьте от локальной копии: `jmustache = { path = "../jmustache" }`, а копию
[java-compat-rs](https://github.com/oxidestack/java-compat-rs) положите в `../java-compat-rs`.

## Быстрый старт

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

Методы-построители `Compiler` — те же, что в JMustache (`standards_mode`, `strict_sections`, `default_value`,
`with_loader`, `with_delims`, …). Справочник API появится на docs.rs после публикации крейта; из локальной копии его
собирает `cargo doc --open`.

## Руководства

[Обновление](docs/i18n/ru/guide/upgrading.md): что каждый выпуск требует от кода, использующего крейт. Этот README и
руководства доступны также в виде сайта, `https://oxidestack.github.io/jmustache/`, после того как репозиторий станет
публичным.

## Сборка и тесты

`upstream/` — исходный код JMustache, git-подмодуль, закреплённый на портированном выпуске, только для чтения:

```sh
git clone --recursive https://github.com/oxidestack/jmustache.git
git clone https://github.com/oxidestack/java-compat-rs.git   # the JDK's semantics in Rust, beside it
cd jmustache
cargo test
```

Тесты — собственные тесты JMustache, портированные один к одному: `tests/mustache.rs` (`SharedTests` и
`MustacheTest`), `tests/spec.rs` (`OfficialSpecTest` и `CustomSpecTest`, читаются из YAML-спецификаций в `upstream/`)
и `tests/thread_safety.rs` (`ThreadSafetyTest`). Там, где этот крейт и JMustache расходятся, прав JMustache.

`make lint` запускает clippy, `make doc` собирает документацию API, считая предупреждения rustdoc ошибками, `make deny`
сверяет лицензии со списком разрешённых CNCF, `make public-check` — проверка, которую проходит каждый push, а
`make site` собирает сайт документации (`site/README.md`) в закреплённом контейнере node.

## Помощь

Вопрос о том, как работает крейт, можно сначала задать [DeepWiki](https://deepwiki.com/oxidestack/jmustache): она
отвечает по коду и документации этого репозитория, а там, где она расходится с документацией, права документация.
Вопросы, которые она не решает, — в [Discussions](https://github.com/oxidestack/jmustache/discussions), ошибки — в
[issues](https://github.com/oxidestack/jmustache/issues). <!-- these three links need the public repository; it is not published yet -->

## Участие

См. [CONTRIBUTING.md](CONTRIBUTING.md); управление проектом описано в [GOVERNANCE.md](GOVERNANCE.md), а сообщения
об уязвимостях отправляются так, как сказано в [SECURITY.md](SECURITY.md).

## Лицензия

Код распространяется по лицензии BSD 3-Clause ([LICENSE](LICENSE), `BSD-3-Clause`), как и JMustache. Документация —
этот README и руководства в `docs/` — распространяется по лицензии
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/) (`CC-BY-4.0`); её
юридический текст — в [LICENSE-docs](LICENSE-docs).
