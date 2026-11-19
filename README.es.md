# jmustache

<!-- languages:begin -->
[English](README.md) · [Русский](README.ru.md) · [中文](README.zh-CN.md) · **Español**
<!-- languages:end -->
<!-- translation of README.md: 2e3220f3894b -->

Un port a Rust de [JMustache](https://github.com/samskivert/jmustache) 1.16 (`com.samskivert.mustache`), el motor
Mustache para Java, fiel a la salida del renderizado: la misma plantilla y los mismos datos producen los mismos bytes,
errores incluidos.

Es un port no oficial. No lo ha hecho, avalado ni soporta el proyecto JMustache ni su autor, Michael Bayne. JMustache
es BSD-3-Clause, y este crate también ([LICENSE](LICENSE), [NOTICE](NOTICE)).

Estado: previo a la primera versión. El crate aún no está publicado en crates.io; hasta entonces se usa desde una copia
local de este repositorio.

## Qué es

Todo JMustache: cada opción de `Compiler` (standards mode, strict sections, valores nulos y por defecto, cadenas vacías
y ceros como falsos, formateadores, escapadores, cargadores de plantillas, collectors, delimitadores), partials con la
sangría de las líneas standalone, herencia de plantillas (`{{< parent}}` / `{{$ block}}`), lambdas con la API
`Template.Fragment` (execute, execute con un contexto, execute de otra plantilla, búsqueda en el contexto, decompile),
lambdas invertibles, contextos propios y visitors. Los objetos de Java son `java_compat::lang::JValue`; los objetos que
describen su clase (`JObject::reflection`) se resuelven exactamente como los resuelve `DefaultCollector`, access
coercion incluida.

Existe para [mock-server](https://github.com/oxidestack/mock-server), una reimplementación de MockServer en Rust, que
necesita que la salida de las plantillas de respuesta Mustache de MockServer sea idéntica byte a byte. Cualquiera que
necesite el comportamiento de JMustache (su standards mode, strict sections, lambdas, herencia de plantillas) en Rust
puede usarlo por sí solo.

## Instalación

El crate se llama `jmustache`. Aún no está publicado en crates.io; cuando lo esté:

```toml
[dependencies]
jmustache = "0.1"                   # not published yet
java-compat-rs = "0.1"              # the Java values a template sees; not published yet
```

Hasta entonces, depende de una copia local: `jmustache = { path = "../jmustache" }`, con
[java-compat-rs](https://github.com/oxidestack/java-compat-rs) clonado en `../java-compat-rs`.

## Inicio rápido

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

Los métodos constructores de `Compiler` son los de JMustache (`standards_mode`, `strict_sections`, `default_value`,
`with_loader`, `with_delims`, …). La referencia de la API estará en docs.rs cuando el crate se publique; desde una
copia local la genera `cargo doc --open`.

## Guías

[Actualización](docs/i18n/es/guide/upgrading.md): lo que cada versión pide al código que usa el crate. Este README y
las guías son también un sitio, `https://oxidestack.github.io/jmustache/`, cuando el repositorio sea público.

## Compilar y probar

`upstream/` es el código fuente original de JMustache, un submódulo de git fijado en la versión portada y de solo
lectura:

```sh
git clone --recursive https://github.com/oxidestack/jmustache.git
git clone https://github.com/oxidestack/java-compat-rs.git   # the JDK's semantics in Rust, beside it
cd jmustache
cargo test
```

Las pruebas son las propias de JMustache, portadas una a una: `tests/mustache.rs` (`SharedTests` y `MustacheTest`),
`tests/spec.rs` (`OfficialSpecTest` y `CustomSpecTest`, leídas de las especificaciones YAML de `upstream/`) y
`tests/thread_safety.rs` (`ThreadSafetyTest`). Donde este crate y JMustache no coinciden, JMustache tiene razón.

`make lint` ejecuta clippy, `make doc` genera la documentación de la API con los avisos de rustdoc como errores,
`make deny` comprueba las licencias contra la lista permitida de la CNCF, `make public-check` es la comprobación que
pasa cada push, y `make site` genera el sitio de documentación (`site/README.md`) en un contenedor de node fijado.

## Obtener ayuda

Una pregunta sobre cómo funciona el crate puede ir primero a [DeepWiki](https://deepwiki.com/oxidestack/jmustache):
responde a partir del código y la documentación de este repositorio, y donde no coincide con la documentación, la
documentación tiene razón. Las preguntas que no resuelva van a
[Discussions](https://github.com/oxidestack/jmustache/discussions), y los errores a
[issues](https://github.com/oxidestack/jmustache/issues). <!-- these three links need the public repository; it is not published yet -->

## Contribuir

Consulta [CONTRIBUTING.md](CONTRIBUTING.md); el gobierno del proyecto está en [GOVERNANCE.md](GOVERNANCE.md), y las
vulnerabilidades se notifican como indica [SECURITY.md](SECURITY.md).

## Licencia

El código se distribuye bajo la licencia BSD 3-Clause ([LICENSE](LICENSE), `BSD-3-Clause`), como JMustache. La
documentación —este README y las guías de `docs/`— se distribuye bajo la licencia
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/) (`CC-BY-4.0`); su texto
legal está en [LICENSE-docs](LICENSE-docs).
