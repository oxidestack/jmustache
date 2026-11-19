//! A port of JMustache 1.16 (`com.samskivert.mustache`), faithful to its rendering output: the same parser,
//! standalone-line trimming, partials with indentation, template inheritance (parents and blocks), variable
//! resolution (parent contexts, compound keys, `this`, `-first` / `-last` / `-index`), lambdas with the
//! `Template.Fragment` API, visitors, falsiness rules and error messages. Values are Java values from
//! [`java_compat::lang`], resolved the way `DefaultCollector` resolves them by reflection.
//!
//! A [`Compiler`] (`Mustache.compiler()`) compiles a [`Template`], which renders against a context value:
//!
//! ```
//! use java_compat::lang::{JList, JMap, JValue, ListKind, MapKind};
//! use jmustache::Compiler;
//!
//! # fn main() -> Result<(), jmustache::MustacheError> {
//! let template = Compiler::new().compile("Hello {{name}}!{{#items}} {{.}}{{/items}}")?;
//! let data = JValue::Map(JMap::from_entries(MapKind::Hash, [
//!     (JValue::str("name"), JValue::str("world")),
//!     (JValue::str("items"), JValue::List(JList::new(ListKind::ArrayList, vec![JValue::Int(1), JValue::Int(2)]))),
//! ]));
//! assert_eq!(template.execute(data)?, "Hello world! 1 2");
//! # Ok(())
//! # }
//! ```
//!
//! The compiler's options are JMustache's, each `with…` or option method returning a new compiler:
//!
//! ```
//! use java_compat::lang::{JMap, JValue, MapKind};
//! use jmustache::Compiler;
//!
//! # fn main() -> Result<(), jmustache::MustacheError> {
//! let compiler = Compiler::new().default_value("?").escape_html(false);
//! let empty = JValue::Map(JMap::new(MapKind::Hash));
//! assert_eq!(compiler.compile("<{{missing}}>")?.execute(empty)?, "<?>");
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod collector;
mod parse;
mod template;

use std::sync::Arc;

pub use collector::{Collector, DefaultCollector, Fetched};
pub use parse::StringSegment;
pub use template::{Fragment, Template};

use java_compat::lang::{JError, JObject, JValue};

/// `MustacheException` (and `MustacheParseException`, `MustacheException.Context`) or a Java exception thrown
/// while resolving a value.
#[derive(Debug, Clone, PartialEq)]
pub enum MustacheError {
    /// A `MustacheException`; `message` is `getMessage()`.
    Mustache {
        /// The exception's class: `com.samskivert.mustache.MustacheException` or one of its subclasses.
        class: &'static str,
        /// `getMessage()`.
        message: String,
    },
    /// Any other Java exception.
    Java(JError),
}

impl MustacheError {
    pub(crate) fn parse(message: impl Into<String>, line: usize) -> MustacheError {
        MustacheError::Mustache { class: "com.samskivert.mustache.MustacheParseException", message: format!("{} @ line {line}", message.into()) }
    }

    /// A `MustacheException` with this message.
    pub fn mustache(message: impl Into<String>) -> MustacheError {
        MustacheError::Mustache { class: "com.samskivert.mustache.MustacheException", message: message.into() }
    }

    pub(crate) fn context(message: impl Into<String>) -> MustacheError {
        MustacheError::Mustache { class: "com.samskivert.mustache.MustacheException$Context", message: message.into() }
    }

    /// `getMessage()`, `None` when the exception has none.
    pub fn message(&self) -> Option<String> {
        match self {
            MustacheError::Mustache { message, .. } => Some(message.clone()),
            MustacheError::Java(e) => e.message.clone(),
        }
    }

    /// `getClass().getName()`.
    pub fn class_name(&self) -> String {
        match self {
            MustacheError::Mustache { class, .. } => class.to_string(),
            MustacheError::Java(e) => e.class.clone(),
        }
    }

    /// `getClass().getSimpleName()`.
    pub fn simple_class_name(&self) -> String {
        self.class_name().rsplit(['.', '$']).next().unwrap_or_default().to_string()
    }

    /// `instanceof MustacheException` (a parse or context exception included).
    pub fn is_mustache_exception(&self) -> bool {
        matches!(self, MustacheError::Mustache { .. })
    }

    /// `instanceof MustacheParseException`.
    pub fn is_parse_exception(&self) -> bool {
        matches!(self, MustacheError::Mustache { class, .. } if class.ends_with("MustacheParseException"))
    }
}

impl std::fmt::Display for MustacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MustacheError::Mustache { class, message } => write!(f, "{class}: {message}"),
            MustacheError::Java(e) => write!(f, "{e}"),
        }
    }
}

/// `Mustache.Formatter`: turns values into text.
pub trait Formatter: Send + Sync {
    /// `format(value)`: the text a variable tag writes for `value`, before escaping.
    fn format(&self, value: &JValue) -> String;
}

/// `DEFAULT_FORMATTER`: `String.valueOf`.
pub struct DefaultFormatter;

impl Formatter for DefaultFormatter {
    fn format(&self, value: &JValue) -> String {
        value.to_java_string()
    }
}

/// `Mustache.Escaper`.
pub trait Escaper: Send + Sync {
    /// `escape(raw)`: the text as an escaped variable tag writes it.
    fn escape(&self, raw: &str) -> String;
}

/// `Escapers`: `HTML`, `NONE` and `simple(repls)`.
pub struct Escapers;

struct Simple(Vec<(String, String)>);

impl Escaper for Simple {
    fn escape(&self, raw: &str) -> String {
        let mut text = raw.to_string();
        for (from, to) in &self.0 {
            text = text.replace(from.as_str(), to);
        }
        text
    }
}

struct NoEscape;

impl Escaper for NoEscape {
    fn escape(&self, raw: &str) -> String {
        raw.to_string()
    }
}

impl Escapers {
    /// `Escapers.HTML`.
    pub fn html() -> Arc<dyn Escaper> {
        Escapers::simple(&[("&", "&amp;"), ("'", "&#39;"), ("\"", "&quot;"), ("<", "&lt;"), (">", "&gt;"), ("`", "&#x60;"), ("=", "&#x3D;")])
    }

    /// `Escapers.NONE`.
    pub fn none() -> Arc<dyn Escaper> {
        Arc::new(NoEscape)
    }

    /// `Escapers.simple(repls)`: each replacement applied in turn to the whole text.
    pub fn simple(repls: &[(&str, &str)]) -> Arc<dyn Escaper> {
        Arc::new(Simple(repls.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()))
    }
}

/// `Escapers.HTML.escape(raw)`.
pub fn escape_html(raw: &str) -> String {
    Escapers::html().escape(raw)
}

/// `Mustache.TemplateLoader`: the source of a partial.
pub trait TemplateLoader: Send + Sync {
    /// `getTemplate(name)`: the source of the partial `{{> name}}`, or the Java exception loading it threw.
    fn get_template(&self, name: &str) -> Result<String, JError>;
}

impl<F: Fn(&str) -> Result<String, JError> + Send + Sync> TemplateLoader for F {
    fn get_template(&self, name: &str) -> Result<String, JError> {
        self(name)
    }
}

/// `FAILING_LOADER`.
struct FailingLoader;

impl TemplateLoader for FailingLoader {
    fn get_template(&self, _name: &str) -> Result<String, JError> {
        Err(JError::new("java.lang.UnsupportedOperationException", "Template loading not configured"))
    }
}

/// `Mustache.Visitor`: walks a compiled template's tags in order (`Template::visit`).
pub trait Visitor {
    /// `visitText`: a run of plain text.
    fn visit_text(&mut self, text: &str);
    /// `visitVariable`: a variable tag, `{{name}}` or `{{{name}}}`.
    fn visit_variable(&mut self, name: &str);
    /// `visitInclude`: a partial, `{{> name}}`; `true` visits the partial's tags too.
    fn visit_include(&mut self, name: &str) -> bool;
    /// `visitParent`: a parent template, `{{< name}}`; `true` visits its tags too. Default `false`.
    fn visit_parent(&mut self, _name: &str) -> bool {
        false
    }
    /// `visitBlock`: a block, `{{$ name}}`; `true` visits its tags too. Default `false`.
    fn visit_block(&mut self, _name: &str) -> bool {
        false
    }
    /// `visitSection`: a section, `{{# name}}`; `true` visits its tags too.
    fn visit_section(&mut self, name: &str) -> bool;
    /// `visitInvertedSection`: an inverted section, `{{^ name}}`; `true` visits its tags too.
    fn visit_inverted_section(&mut self, name: &str) -> bool;
}

/// `Mustache.Lambda` (and `InvertibleLambda`).
pub trait LambdaFn: Send + Sync {
    /// `execute(frag, out)`: what the section `{{#lambda}}…{{/lambda}}` writes, appended to `out`.
    fn execute(&self, frag: &Fragment, out: &mut String) -> Result<(), MustacheError>;
    /// `InvertibleLambda.executeInverse`; `None` for a plain lambda.
    fn execute_inverse(&self, _frag: &Fragment, _out: &mut String) -> Option<Result<(), MustacheError>> {
        None
    }
}

/// A lambda as a context value.
pub struct Lambda {
    /// The lambda's implementation.
    pub f: Arc<dyn LambdaFn>,
    /// The implementing class's name (a Java lambda's is synthetic, `Outer$$Lambda/0x…`), which `toString()` shows.
    pub class: String,
}

struct Rendering(Arc<dyn Fn(String) -> Result<String, MustacheError> + Send + Sync>);

impl LambdaFn for Rendering {
    fn execute(&self, frag: &Fragment, out: &mut String) -> Result<(), MustacheError> {
        let s = frag.execute()?;
        out.push_str(&(self.0)(s)?);
        Ok(())
    }
}

impl Lambda {
    /// A `Mustache.Lambda` implemented by `f`, as a context value.
    pub fn of(f: impl LambdaFn + 'static) -> JValue {
        Lambda::object("com.samskivert.mustache.Mustache$Lambda", Arc::new(f))
    }

    /// The lambda `f` as a context value whose class is `class`.
    pub fn object(class: &str, f: Arc<dyn LambdaFn>) -> JValue {
        JValue::Object(Arc::new(Lambda { f, class: class.to_string() }))
    }

    /// A lambda that only executes its fragment and writes what `f` makes of the result.
    pub fn value(f: impl Fn(String) -> Result<String, MustacheError> + Send + Sync + 'static) -> JValue {
        Lambda::with_class("com.samskivert.mustache.Mustache$Lambda", f)
    }

    /// [`Lambda::value`] whose class is `class`.
    pub fn with_class(class: &str, f: impl Fn(String) -> Result<String, MustacheError> + Send + Sync + 'static) -> JValue {
        Lambda::object(class, Arc::new(Rendering(Arc::new(f))))
    }
}

impl JObject for Lambda {
    fn class_name(&self) -> String {
        self.class.clone()
    }

    fn invoke(&self, _name: &str, _args: &[JValue]) -> Option<Result<Option<JValue>, JError>> {
        None
    }

    fn to_java_string(&self) -> String {
        format!("{}@{:x}", self.class_name(), self as *const Lambda as usize as u32)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A [`CustomContext`]'s `get`: the variable's value by name.
pub type CustomGet = Arc<dyn Fn(&str) -> Result<JValue, JError> + Send + Sync>;

/// `Mustache.CustomContext`: variables looked up through `get`.
pub struct CustomContext(pub CustomGet);

impl CustomContext {
    /// A custom context looking variables up through `f`, as a context value.
    pub fn value(f: impl Fn(&str) -> Result<JValue, JError> + Send + Sync + 'static) -> JValue {
        JValue::Object(Arc::new(CustomContext(Arc::new(f))))
    }
}

impl JObject for CustomContext {
    fn class_name(&self) -> String {
        "com.samskivert.mustache.Mustache$CustomContext".into()
    }

    fn invoke(&self, _name: &str, _args: &[JValue]) -> Option<Result<Option<JValue>, JError>> {
        None
    }

    fn to_java_string(&self) -> String {
        format!("{}@{:x}", self.class_name(), self as *const CustomContext as usize as u32)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `Mustache.Compiler`: immutable; each `with…` returns a new compiler.
#[derive(Clone)]
pub struct Compiler {
    /// `standardsMode`: the Mustache specification's behaviour alone, without JMustache's extensions.
    pub standards_mode: bool,
    /// `strictSections`: a section whose variable is missing fails instead of rendering as empty.
    pub strict_sections: bool,
    /// `nullValue`; with `missing_is_null` this is `defaultValue`.
    pub null_value: Option<String>,
    /// `missingIsNull`: a missing variable renders as `null_value` instead of failing.
    pub missing_is_null: bool,
    /// `emptyStringIsFalse`: an empty string is falsey in a section.
    pub empty_string_is_false: bool,
    /// `zeroIsFalse`: a zero number is falsey in a section.
    pub zero_is_false: bool,
    /// `formatter`: turns values into text.
    pub formatter: Arc<dyn Formatter>,
    /// `escaper`: escapes what `{{name}}` writes; HTML by default.
    pub escaper: Arc<dyn Escaper>,
    /// `loader`: the source of partials; by default loading fails.
    pub loader: Arc<dyn TemplateLoader>,
    /// `collector`: how variables are resolved on context values.
    pub collector: Arc<dyn Collector>,
    pub(crate) delims: parse::Delims,
}

impl Default for Compiler {
    /// `Mustache.compiler()`.
    fn default() -> Compiler {
        Compiler {
            standards_mode: false,
            strict_sections: false,
            null_value: None,
            missing_is_null: false,
            empty_string_is_false: false,
            zero_is_false: false,
            formatter: Arc::new(DefaultFormatter),
            escaper: Escapers::html(),
            loader: Arc::new(FailingLoader),
            collector: Arc::new(DefaultCollector::default()),
            delims: parse::Delims::default(),
        }
    }
}

impl Compiler {
    /// `Mustache.compiler()`.
    pub fn new() -> Compiler {
        Compiler::default()
    }

    /// `escapeHTML(escape)`: HTML escaping on, or no escaping.
    pub fn escape_html(self, escape: bool) -> Compiler {
        self.with_escaper(if escape { Escapers::html() } else { Escapers::none() })
    }

    /// `standardsMode(on)`.
    pub fn standards_mode(mut self, on: bool) -> Compiler {
        self.standards_mode = on;
        self
    }

    /// `strictSections(on)`.
    pub fn strict_sections(mut self, on: bool) -> Compiler {
        self.strict_sections = on;
        self
    }

    /// `defaultValue(value)`: the value for missing variables and variables that resolve to null.
    pub fn default_value(mut self, value: &str) -> Compiler {
        self.null_value = Some(value.to_string());
        self.missing_is_null = true;
        self
    }

    /// `nullValue(value)`: the value for variables that resolve to null; missing ones still fail.
    pub fn null_value(mut self, value: &str) -> Compiler {
        self.null_value = Some(value.to_string());
        self.missing_is_null = false;
        self
    }

    /// `emptyStringIsFalse(on)`.
    pub fn empty_string_is_false(mut self, on: bool) -> Compiler {
        self.empty_string_is_false = on;
        self
    }

    /// `zeroIsFalse(on)`.
    pub fn zero_is_false(mut self, on: bool) -> Compiler {
        self.zero_is_false = on;
        self
    }

    /// `withFormatter(formatter)`.
    pub fn with_formatter(mut self, formatter: Arc<dyn Formatter>) -> Compiler {
        self.formatter = formatter;
        self
    }

    /// `withEscaper(escaper)`.
    pub fn with_escaper(mut self, escaper: Arc<dyn Escaper>) -> Compiler {
        self.escaper = escaper;
        self
    }

    /// `withLoader(loader)`.
    pub fn with_loader(mut self, loader: Arc<dyn TemplateLoader>) -> Compiler {
        self.loader = loader;
        self
    }

    /// `withCollector(collector)`.
    pub fn with_collector(mut self, collector: Arc<dyn Collector>) -> Compiler {
        self.collector = collector;
        self
    }

    /// `withDelims("AB CD")`.
    pub fn with_delims(mut self, delims: &str) -> Result<Compiler, MustacheError> {
        let mut d = parse::Delims::default();
        d.update(delims)?;
        self.delims = d;
        Ok(self)
    }

    /// `compile(template)`.
    pub fn compile(&self, source: &str) -> Result<Template, MustacheError> {
        let segs = parse::Parser::new(self).parse(source)?;
        Ok(Template::new(parse::trim(segs, true), self.clone()))
    }

    /// `loadTemplate(name)`: the loader's source, compiled; a Java exception from the loader passes through.
    pub fn load_template(&self, name: &str) -> Result<Template, MustacheError> {
        let source = self.loader.get_template(name).map_err(MustacheError::Java)?;
        self.compile(&source)
    }

    /// `computeNullValue(name)`.
    pub fn compute_null_value(&self, name: &str) -> Option<String> {
        self.null_value.as_ref().map(|v| v.replace("{{name}}", name))
    }

    /// `isFalsey(value)`: an empty formatted string (with `emptyStringIsFalse`), a zero `longValue()` (with
    /// `zeroIsFalse`).
    pub fn is_falsey(&self, value: &JValue) -> bool {
        (self.empty_string_is_false && !value.is_null() && self.formatter.format(value).is_empty())
            || (self.zero_is_false && is_number(value) && long_value(value) == 0)
    }
}

fn is_number(v: &JValue) -> bool {
    matches!(v, JValue::Int(_) | JValue::Long(_) | JValue::Float(_) | JValue::Double(_) | JValue::BigInteger(_) | JValue::BigDecimal(_))
}

/// `Number.longValue()`.
fn long_value(v: &JValue) -> i64 {
    match v {
        JValue::Int(i) => *i as i64,
        JValue::Long(l) => *l,
        other => java_compat::methods::as_f64(other).map(|d| d as i64).unwrap_or(1),
    }
}
