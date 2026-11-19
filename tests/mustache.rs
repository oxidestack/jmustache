//! `SharedTests` and `MustacheTest` (which runs them on the JVM), test for test. Java's anonymous test objects
//! are [`Bean`]s: each declares what reflection sees (methods by return type and visibility, fields, interfaces)
//! and answers invocations like the Java object would.

use std::collections::HashMap;
use std::sync::Arc;

use java_compat::reflect::JType;
use java_compat::lang::{JArray, JError, JList, JMap, JObject, JValue, ListKind, MapKind, Members, Reflection, Returns};
use jmustache::{Compiler, CustomContext, DefaultCollector, Escapers, Formatter, Fragment, Lambda, LambdaFn, MustacheError, StringSegment, Visitor};

type Getter = Arc<dyn Fn() -> Result<Option<JValue>, JError> + Send + Sync>;

/// A Java object built for a test.
#[derive(Clone, Default)]
struct Bean {
    class: String,
    reflection: Reflection,
    methods: HashMap<String, Getter>,
    fields: HashMap<String, JValue>,
    to_string: Option<String>,
}

impl Bean {
    fn new(class: &str) -> Bean {
        Bean { class: class.into(), reflection: Reflection { classes: vec![Members::default()], interfaces: vec![] }, ..Bean::default() }
    }

    fn members(&mut self) -> &mut Members {
        &mut self.reflection.classes[0]
    }

    /// A method declared on the class, returning `v`.
    fn method(mut self, name: &str, returns: Returns, public: bool, v: JValue) -> Bean {
        *self.members() = std::mem::take(self.members()).method(name, returns, public);
        self.methods.insert(name.into(), Arc::new(move || Ok(Some(v.clone()))));
        self
    }

    fn void_method(mut self, name: &str) -> Bean {
        *self.members() = std::mem::take(self.members()).method(name, Returns::Void, false);
        self.methods.insert(name.into(), Arc::new(|| Ok(None)));
        self
    }

    fn field(mut self, name: &str, public: bool, v: JValue) -> Bean {
        *self.members() = std::mem::take(self.members()).field(name, public);
        self.fields.insert(name.into(), v);
        self
    }

    /// An interface the class implements, with its default methods; `impl_` is what invoking the name runs.
    fn interface(mut self, members: Members, impls: &[(&str, JValue)]) -> Bean {
        self.reflection.interfaces.push(members);
        for (name, v) in impls {
            let v = v.clone();
            self.methods.entry(name.to_string()).or_insert_with(|| Arc::new(move || Ok(Some(v.clone()))));
        }
        self
    }

    fn value(self) -> JValue {
        JValue::Object(Arc::new(self))
    }
}

impl JObject for Bean {
    fn class_name(&self) -> String {
        self.class.clone()
    }

    fn invoke(&self, name: &str, args: &[JValue]) -> Option<Result<Option<JValue>, JError>> {
        if !args.is_empty() {
            return None;
        }
        self.methods.get(name).map(|f| f())
    }

    fn field(&self, name: &str) -> Option<JValue> {
        self.fields.get(name).cloned()
    }

    fn to_java_string(&self) -> String {
        self.to_string.clone().unwrap_or_else(|| format!("{}@1b6d3586", self.class))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn reflection(&self) -> Option<Reflection> {
        Some(self.reflection.clone())
    }
}

fn s(v: &str) -> JValue {
    JValue::str(v)
}

/// `SharedTests.context(Object...)`: a `HashMap`.
fn context(entries: &[(&str, JValue)]) -> JValue {
    JValue::Map(JMap::from_entries(MapKind::Hash, entries.iter().map(|(k, v)| (s(k), v.clone()))))
}

fn empty() -> JValue {
    context(&[])
}

fn list(items: Vec<JValue>) -> JValue {
    JValue::List(JList::new(ListKind::Fixed, items))
}

fn array(items: Vec<JValue>) -> JValue {
    JValue::Array(Arc::new(JArray::new(JType::object(), items)))
}

fn int_array(items: &[i32]) -> JValue {
    JValue::Array(Arc::new(JArray::new(JType::Int, items.iter().map(|i| JValue::Int(*i)).collect())))
}

fn iterator(items: Vec<JValue>) -> JValue {
    JValue::List(JList::new(ListKind::Iterator, items))
}

fn test_with(compiler: &Compiler, expected: &str, template: &str, ctx: JValue) {
    let actual = compiler.compile(template).and_then(|t| t.execute(ctx)).unwrap_or_else(|e| panic!("{template:?}: {e}"));
    assert_eq!(actual, expected, "template {template:?}");
}

fn test(expected: &str, template: &str, ctx: JValue) {
    test_with(&Compiler::new(), expected, template, ctx);
}

fn fails(compiler: &Compiler, template: &str, ctx: JValue) -> MustacheError {
    match compiler.compile(template).and_then(|t| t.execute(ctx)) {
        Ok(out) => panic!("{template:?} should fail, rendered {out:?}"),
        Err(e) => e,
    }
}

fn loader(f: impl Fn(&str) -> String + Send + Sync + 'static) -> Arc<dyn jmustache::TemplateLoader> {
    Arc::new(move |n: &str| -> Result<String, JError> { Ok(f(n)) })
}

fn partials(entries: &[(&str, &str)]) -> Arc<dyn jmustache::TemplateLoader> {
    let map: HashMap<String, String> = entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    loader(move |n| map.get(n).cloned().unwrap_or_default())
}

/// A lambda's body: the fragment and where it writes.
type LambdaBody = Box<dyn Fn(&Fragment, &mut String) -> Result<(), MustacheError> + Send + Sync>;

struct Closure(LambdaBody);

impl LambdaFn for Closure {
    fn execute(&self, frag: &Fragment, out: &mut String) -> Result<(), MustacheError> {
        (self.0)(frag, out)
    }
}

fn lambda(f: impl Fn(&Fragment, &mut String) -> Result<(), MustacheError> + Send + Sync + 'static) -> JValue {
    Lambda::of(Closure(Box::new(f)))
}

// ---- SharedTests ---------------------------------------------------------------------------------------------

#[test]
fn simple_variable() {
    test("bar", "{{foo}}", context(&[("foo", s("bar"))]));
}

#[test]
fn primitive_array_variable() {
    test("1234", "{{#foo}}{{this}}{{/foo}}", context(&[("foo", int_array(&[1, 2, 3, 4]))]));
}

#[test]
fn primitive_array_index_variable() {
    test("1", "{{foo.0}}", context(&[("foo", int_array(&[1, 2, 3, 4]))]));
}

#[test]
fn primitive_array_index_out_of_bounds_variable() {
    test_with(&Compiler::new().default_value("?"), "?", "{{foo.4}}", context(&[("foo", int_array(&[1, 2, 3, 4]))]));
}

#[test]
fn one_shot_section() {
    test("baz", "{{#foo}}{{bar}}{{/foo}}", context(&[("foo", context(&[("bar", s("baz"))]))]));
}

fn bar_list() -> Vec<JValue> {
    vec![context(&[("bar", s("baz"))]), context(&[("bar", s("bif"))])]
}

#[test]
fn list_section() {
    test("bazbif", "{{#foo}}{{bar}}{{/foo}}", context(&[("foo", list(bar_list()))]));
}

#[test]
fn list_index_section() {
    test("baz", "{{#foo.0}}{{bar}}{{/foo.0}}", context(&[("foo", list(bar_list()))]));
}

#[test]
fn list_index_out_of_bounds_section() {
    test("", "{{#foo.2}}{{bar}}{{/foo.2}}", context(&[("foo", list(bar_list()))]));
}

#[test]
fn list_item_section() {
    test("baz", "{{foo.0.bar}}", context(&[("foo", list(bar_list()))]));
}

#[test]
fn map_entries_section() {
    let data = JMap::new(MapKind::Hash);
    data.put_str("k1", s("v1"));
    data.put_str("k2", s("v2"));
    test_with(&Compiler::new().escape_html(false), "k1=v1k2=v2", "{{#map.entrySet}}{{.}}{{/map.entrySet}}", context(&[("map", JValue::Map(data))]));
}

#[test]
fn array_section() {
    test("bazbif", "{{#foo}}{{bar}}{{/foo}}", context(&[("foo", array(bar_list()))]));
}

#[test]
fn array_index_section() {
    test("baz", "{{#foo.0}}{{bar}}{{/foo.0}}", context(&[("foo", array(bar_list()))]));
}

#[test]
fn array_index_out_of_bounds_section() {
    test("", "{{#foo.2}}{{bar}}{{/foo.2}}", context(&[("foo", array(bar_list()))]));
}

#[test]
fn array_item_section() {
    test("baz", "{{foo.0.bar}}", context(&[("foo", array(bar_list()))]));
}

#[test]
fn iterator_section() {
    test("bazbif", "{{#foo}}{{bar}}{{/foo}}", context(&[("foo", iterator(bar_list()))]));
}

#[test]
fn iterator_index_section() {
    test("baz", "{{#foo.0}}{{bar}}{{/foo.0}}", context(&[("foo", iterator(bar_list()))]));
}

#[test]
fn iterator_index_out_of_bounds_section() {
    test("", "{{#foo.2}}{{bar}}{{/foo.2}}", context(&[("foo", iterator(bar_list()))]));
}

#[test]
fn iterator_item_section() {
    test("baz", "{{foo.0.bar}}", context(&[("foo", iterator(bar_list()))]));
}

#[test]
fn empty_list_section() {
    test("", "{{#foo}}{{bar}}{{/foo}}", context(&[("foo", list(vec![]))]));
}

#[test]
fn empty_array_section() {
    test("", "{{#foo}}{{bar}}{{/foo}}", context(&[("foo", array(vec![]))]));
}

#[test]
fn empty_iterator_section() {
    test("", "{{#foo}}{{bar}}{{/foo}}", context(&[("foo", iterator(vec![]))]));
}

#[test]
fn false_section() {
    test("", "{{#foo}}{{bar}}{{/foo}}", context(&[("foo", JValue::Bool(false))]));
}

#[test]
fn nested_list_section() {
    test(
        "1234",
        "{{#a}}{{#b}}{{c}}{{/b}}{{#d}}{{e}}{{/d}}{{/a}}",
        context(&[(
            "a",
            context(&[
                ("b", array(vec![context(&[("c", s("1"))]), context(&[("c", s("2"))])])),
                ("d", array(vec![context(&[("e", s("3"))]), context(&[("e", s("4"))])])),
            ]),
        )]),
    );
}

#[test]
fn null_section() {
    let ctx = context(&[("foo", JValue::Null)]);
    test("", "{{#foo}}{{bar}}{{/foo}}", ctx.clone());
    test_with(&Compiler::new().default_value(""), "", "{{#foo}}{{bar}}{{/foo}}", ctx.clone());
    test_with(&Compiler::new().null_value(""), "", "{{#foo}}{{bar}}{{/foo}}", ctx);
}

#[test]
fn missing_non_strict_section() {
    test("", "{{#foo}}{{bar}}{{/foo}}", empty());
    test_with(&Compiler::new().null_value(""), "", "{{#foo}}{{bar}}{{/foo}}", empty());
    test_with(&Compiler::new().default_value(""), "", "{{#foo}}{{bar}}{{/foo}}", empty());
}

#[test]
fn missing_strict_section() {
    assert!(fails(&Compiler::new().strict_sections(true), "{{#foo}}{{bar}}{{/foo}}", empty()).is_mustache_exception());
}

#[test]
fn missing_strict_section_null_value() {
    assert!(fails(&Compiler::new().strict_sections(true).null_value(""), "{{#foo}}{{bar}}{{/foo}}", empty()).is_mustache_exception());
}

#[test]
fn missing_strict_section_default_value() {
    assert!(fails(&Compiler::new().strict_sections(true).default_value(""), "{{#foo}}{{bar}}{{/foo}}", empty()).is_mustache_exception());
}

#[test]
fn section_with_non_falsey_empty_string() {
    test("test", "{{#foo}}test{{/foo}}", context(&[("foo", s(""))]));
}

#[test]
fn section_with_falsey_empty_string() {
    let ctx = context(&[("foo", s("")), ("bar", s("nonempty"))]);
    let compiler = Compiler::new().empty_string_is_false(true);
    test_with(&compiler, "", "{{#foo}}test{{/foo}}", ctx.clone());
    test_with(&compiler, "test", "{{#bar}}test{{/bar}}", ctx.clone());
    test_with(&compiler, "test", "{{^foo}}test{{/foo}}", ctx.clone());
    test_with(&compiler, "", "{{^bar}}test{{/bar}}", ctx);
}

#[test]
fn comment() {
    test("foobar", "foo{{! nothing to see here}}bar", empty());
}

#[test]
fn comment_with_funny_chars() {
    test("foobar", "foo{{! {baz\n }}bar", empty());
}

#[test]
fn partial_use_when_unconfigured() {
    let e = fails(&Compiler::new(), "{{>foo}}", JValue::Null);
    assert_eq!(e.class_name(), "java.lang.UnsupportedOperationException");
}

#[test]
fn partial() {
    test_with(
        &Compiler::new().with_loader(partials(&[("foo", "inside:{{bar}}"), ("baz", "nonfoo")])),
        "foo inside:foo nonfoo foo",
        "{{bar}} {{>foo}} {{>baz}} {{bar}}",
        context(&[("bar", s("foo"))]),
    );
}

#[test]
fn partial_indent() {
    test_with(
        &Compiler::new().with_loader(loader(|_| "|\n{{{content}}}\n|\n".into())),
        "\\\n |\n <\n->\n |\n/\n",
        "\\\n {{>partial}}\n/\n",
        context(&[("content", s("<\n->"))]),
    );
}

#[test]
fn partial_blank_lines() {
    test_with(&Compiler::new().with_loader(loader(|_| "|\na\n\nb\n|\n".into())), "\\\n\t|\n\ta\n\t\n\tb\n\t|\n/\n", "\\\n\t{{>partial}}\n/\n", empty());
}

#[test]
fn nested_partial_blank_lines() {
    let l = loader(|n| if n == "partial" { "1\n\t{{>nest}}\n1\n".into() } else { "2\na\n\nb\n2\n".into() });
    test_with(&Compiler::new().with_loader(l), "\\\n\t1\n\t\t2\n\t\ta\n\t\t\n\t\tb\n\t\t2\n\t1\n/\n", "\\\n\t{{>partial}}\n/\n", empty());
}

#[test]
fn nested_partial_blank_lines_crlf() {
    let l = loader(|n| if n == "partial" { "1\r\n\t{{>nest}}\r\n1\r\n".into() } else { "2\r\na\r\n\r\nb\r\n2\r\n".into() });
    test_with(
        &Compiler::new().with_loader(l),
        "\\\r\n\t1\r\n\t\t2\r\n\t\ta\r\n\t\t\r\n\t\tb\r\n\t\t2\r\n\t1\r\n/\r\n",
        "\\\r\n\t{{>partial}}\r\n/\r\n",
        empty(),
    );
}

#[test]
fn nested_partial_indent() {
    let l = partials(&[("partial", "1\n {{>nest}}\n1\n"), ("nest", "2\n{{{content}}}\n2\n")]);
    test_with(&Compiler::new().with_loader(l), "|\n 1\n  2\n  <\n->\n  2\n 1\n|\n", "|\n {{>partial}}\n|\n", context(&[("content", s("<\n->"))]));
}

#[test]
fn partial_indent_with_variable_at_the_start() {
    test_with(
        &Compiler::new().with_loader(loader(|_| "{{{content}}}\n|\n".into())),
        "\\\n <\n->\n |\n/\n",
        "\\\n {{>partial}}\n/\n",
        context(&[("content", s("<\n->"))]),
    );
}

#[test]
fn partial_indent_with_block() {
    test_with(
        &Compiler::new().with_loader(loader(|_| "|\n{{#show}}\n{{{content}}}{{/show}}\n|\n".into())),
        "\\\n |\n <\n->\n |\n/\n",
        "\\\n {{>partial}}\n/\n",
        context(&[("show", JValue::Bool(true)), ("content", s("<\n->"))]),
    );
}

#[test]
fn partial_indent_in_block() {
    test_with(&Compiler::new().with_loader(loader(|_| "content".into())), " content", "{{#show}}\n {{>partial}}\n{{/show}}\n", context(&[("show", JValue::Bool(true))]));
}

#[test]
fn partial_indent_with_block_at_start() {
    test_with(
        &Compiler::new().with_loader(loader(|_| "{{#show}}\n{{{content}}}{{/show}}\n|\n".into())),
        "\\\n <\n->\n |\n/\n",
        "\\\n {{>partial}}\n/\n",
        context(&[("show", JValue::Bool(true)), ("content", s("<\n->"))]),
    );
}

#[test]
fn partial_indent_with_inline_block() {
    test_with(
        &Compiler::new().with_loader(loader(|_| "line {{#show}}content{{/show}}\n".into())),
        "\\\n line content\n/\n",
        "\\\n {{>partial}}\n/\n",
        context(&[("show", JValue::Bool(true))]),
    );
}

#[test]
fn partial_plus_nested_context() {
    let l = loader(|n| if n == "nested" { "{{name}}{{thing_name}}".into() } else { "nonfoo".into() });
    test_with(
        &Compiler::new().with_loader(l),
        "foo((foobar)(foobaz))",
        "{{name}}({{#things}}({{>nested}}){{/things}})",
        context(&[("name", s("foo")), ("things", list(vec![context(&[("thing_name", s("bar"))]), context(&[("thing_name", s("baz"))])]))]),
    );
}

#[test]
fn recursive_partial() {
    let template = "[{{name}}{{#properties}}, {{> schema.mustache}}{{/properties}}]";
    let t = template.to_string();
    test_with(
        &Compiler::new().with_loader(loader(move |_| t.clone())),
        "[level0, [level1a, [level2a]], [level1b, [level2b]]]",
        template,
        context(&[
            ("name", s("level0")),
            (
                "properties",
                list(vec![
                    context(&[("name", s("level1a")), ("properties", context(&[("name", s("level2a")), ("properties", list(vec![]))]))]),
                    context(&[("name", s("level1b")), ("properties", context(&[("name", s("level2b")), ("properties", JValue::Null)]))]),
                ]),
            ),
        ]),
    );
}

#[test]
fn delimiter_change() {
    let ctx = context(&[("one", s("foo")), ("two", s("bar")), ("three", s("baz"))]);
    test("foo bar baz", "{{one}} {{=<% %>=}}<%two%><%={{ }}=%> {{three}}", ctx.clone());
    test("baz bar foo", "{{three}} {{=% %=}}%two%%={{ }}=% {{one}}", ctx);
}

#[test]
fn unescape_html() {
    let ctx = || context(&[("a", s("<b>"))]);
    for escape in [true, false] {
        assert_eq!(Compiler::new().escape_html(escape).compile("{{&a}}").unwrap().execute(ctx()).unwrap(), "<b>");
        assert_eq!(Compiler::new().escape_html(escape).compile("{{{a}}}").unwrap().execute(ctx()).unwrap(), "<b>");
    }
}

#[test]
fn dangling_tag() {
    let ctx = || context(&[("a", s("<b>"))]);
    test("foo{", "foo{", ctx());
    test("foo{{", "foo{{", ctx());
    test("foo{{a", "foo{{a", ctx());
    test("foo{{a}", "foo{{a}", ctx());
}

#[test]
fn stray_tag_characters() {
    let b = || context(&[("a", s("[b]"))]);
    let h = || context(&[("a", s("<b>"))]);
    test("funny [b] business {{", "funny {{a}} business {{", b());
    test("funny <b> business {{", "funny {{{a}}} business {{", h());
    test("{{ funny [b] business", "{{ funny {{a}} business", b());
    test("{{ funny <b> business", "{{ funny {{{a}}} business", h());
    test("funny [b] business }}", "funny {{a}} business }}", b());
    test("funny <b> business }}", "funny {{{a}}} business }}", h());
    test("}} funny [b] business", "}} funny {{a}} business", b());
    test("}} funny <b> business", "}} funny {{{a}}} business", h());
}

#[test]
fn invalid_unescape_html() {
    let e = fails(&Compiler::new().escape_html(true), "{{{a}}", context(&[("a", s("<b>"))]));
    assert!(e.is_parse_exception());
}

#[test]
fn escape_html() {
    assert_eq!(Compiler::new().compile("{{a}}").unwrap().execute(context(&[("a", s("<b>"))])).unwrap(), "&lt;b&gt;");
    assert_eq!(Compiler::new().escape_html(false).compile("{{a}}").unwrap().execute(context(&[("a", s("<b>"))])).unwrap(), "<b>");
    assert_eq!(Compiler::new().compile("{{a}}").unwrap().execute(context(&[("a", s("`="))])).unwrap(), "&#x60;&#x3D;");
}

#[test]
fn user_defined_escaping() {
    let escaper = Escapers::simple(&[("[", ":BEGIN:"), ("]", ":END:")]);
    assert_eq!(Compiler::new().with_escaper(escaper).compile("{{a}}").unwrap().execute(context(&[("a", s("[b]"))])).unwrap(), ":BEGIN:b:END:");
}

#[test]
fn partial_delimiter_match() {
    assert_eq!(Compiler::new().compile("{bob}").unwrap().execute(empty()).unwrap(), "{bob}");
    assert_eq!(Compiler::new().compile("{{bob}bob}}").unwrap().execute(context(&[("bob}bob", s("bar"))])).unwrap(), "bar");
}

#[test]
fn top_level_this() {
    assert_eq!(Compiler::new().compile("{{this}}").unwrap().execute(s("bar")).unwrap(), "bar");
    assert_eq!(Compiler::new().compile("{{.}}").unwrap().execute(s("bar")).unwrap(), "bar");
}

#[test]
fn nested_this() {
    let ctx = || context(&[("things", list(vec![s("bar"), s("baz"), s("bif")]))]);
    assert_eq!(Compiler::new().compile("{{#things}}{{this}}{{/things}}").unwrap().execute(ctx()).unwrap(), "barbazbif");
    assert_eq!(Compiler::new().compile("{{#things}}{{.}}{{/things}}").unwrap().execute(ctx()).unwrap(), "barbazbif");
}

#[test]
fn nested_null_this() {
    let out = Compiler::new().default_value("!").compile("{{#things}}{{.}}{{/things}}").unwrap().execute(context(&[("things", list(vec![s("bar"), JValue::Null, s("bif")]))])).unwrap();
    assert_eq!(out, "bar!bif");
}

fn newline_skipping(sep: &str) {
    let tmpl = format!("list:{sep}{{{{#items}}}}{sep}{{{{this}}}}{sep}{{{{/items}}}}{sep}{{{{^items}}}}{sep}no items{sep}{{{{/items}}}}{sep}endlist");
    test(&format!("list:{sep}one{sep}two{sep}three{sep}endlist"), &tmpl, context(&[("items", list(vec![s("one"), s("two"), s("three")]))]));
    test(&format!("list:{sep}no items{sep}endlist"), &tmpl, context(&[("items", list(vec![]))]));
    let html = format!("<ul>{sep}  {{{{#items}}}}  {sep}  <li>{{{{this}}}}</li>{sep}  {{{{/items}}}}  {sep}  {{{{^items}}}}{sep}  <li>no items</li>{sep}  {{{{/items}}}}{sep}</ul>");
    test(&format!("<ul>{sep}  <li>one</li>{sep}  <li>two</li>{sep}  <li>three</li>{sep}</ul>"), &html, context(&[("items", list(vec![s("one"), s("two"), s("three")]))]));
    test(&format!("<ul>{sep}  <li>no items</li>{sep}</ul>"), &html, context(&[("items", list(vec![]))]));
}

#[test]
fn newline_skipping_lf() {
    newline_skipping("\n");
}

#[test]
fn newline_skipping_crlf() {
    newline_skipping("\r\n");
}

#[test]
fn newline_skipping_delims_tag() {
    test("Begin.\nEnd.\n", "Begin.\n{{=@ @=}}\nEnd.\n", empty());
}

#[test]
fn no_trim_newline_from_nested_tag_at_0() {
    test(" | \n  | \n", " | {{^boolean}}{{! comment }}\n {{/boolean}} | \n", context(&[("boolean", JValue::Bool(false))]));
}

#[test]
fn trim_blank() {
    let str = StringSegment::new("  \r\n  ", false);
    assert_eq!(str.trim_lead_blank().to_string(), "Text(  )-1/0");
    assert_eq!(str.trim_trail_blank().to_string(), "Text(  \\r\\n)3/-1");
}

#[derive(Default)]
struct GetKeysVisitor {
    keys: std::collections::HashSet<String>,
}

impl Visitor for GetKeysVisitor {
    fn visit_text(&mut self, _text: &str) {}
    fn visit_variable(&mut self, name: &str) {
        self.keys.insert(name.into());
    }
    fn visit_include(&mut self, name: &str) -> bool {
        self.keys.insert(name.into());
        true
    }
    fn visit_section(&mut self, name: &str) -> bool {
        self.keys.insert(name.into());
        true
    }
    fn visit_inverted_section(&mut self, name: &str) -> bool {
        self.keys.insert(name.into());
        true
    }
}

fn keys(names: &[&str]) -> std::collections::HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn visit() {
    let mut viz = GetKeysVisitor::default();
    Compiler::new().compile("{{#one}}1{{/one}} {{^two}}2{{three}}{{/two}}{{four}}").unwrap().visit(&mut viz).unwrap();
    assert_eq!(viz.keys, keys(&["one", "two", "three", "four"]));
}

#[test]
fn visit_nested() {
    let template = "{{#one}}1{{/one}} {{^two}}2{{two_and_half}}{{#five}}{{three}}{{/five}}{{/two}} {{#one}}{{three}}{{/one}} {{four}}{{! ignore me }}";
    let mut viz = GetKeysVisitor::default();
    Compiler::new().compile(template).unwrap().visit(&mut viz).unwrap();
    assert_eq!(viz.keys, keys(&["one", "two", "three", "four", "two_and_half", "five"]));
}

#[test]
fn newline_non_skipping() {
    let tmpl = "thing?: {{#thing}}yes{{/thing}}{{^thing}}no{{/thing}}\nthat's nice";
    test("thing?: yes\nthat's nice", tmpl, context(&[("thing", JValue::Bool(true))]));
    test("thing?: no\nthat's nice", tmpl, context(&[("thing", JValue::Bool(false))]));
}

#[test]
fn nested_contexts() {
    test(
        "foo((foobar)(foobaz))",
        "{{name}}({{#things}}({{name}}{{thing_name}}){{/things}})",
        context(&[("name", s("foo")), ("things", list(vec![context(&[("thing_name", s("bar"))]), context(&[("thing_name", s("baz"))])]))]),
    );
}

#[test]
fn shadowed_context() {
    test(
        "foo((bar)(baz))",
        "{{name}}({{#things}}({{name}}){{/things}})",
        context(&[("name", s("foo")), ("things", list(vec![context(&[("name", s("bar"))]), context(&[("name", s("baz"))])]))]),
    );
}

fn things(items: &[&str]) -> JValue {
    context(&[("things", list(items.iter().map(|i| s(i)).collect()))])
}

#[test]
fn first() {
    test("foo|bar|baz", "{{#things}}{{^-first}}|{{/-first}}{{this}}{{/things}}", things(&["foo", "bar", "baz"]));
}

#[test]
fn last() {
    test("foo|bar|baz", "{{#things}}{{this}}{{^-last}}|{{/-last}}{{/things}}", things(&["foo", "bar", "baz"]));
}

#[test]
fn first_last() {
    test("[foo]", "{{#things}}{{#-first}}[{{/-first}}{{this}}{{#-last}}]{{/-last}}{{/things}}", things(&["foo"]));
    test("foo", "{{#things}}{{this}}{{^-last}}|{{/-last}}{{/things}}", things(&["foo"]));
}

#[test]
fn first_last_combo() {
    test("FIRST_and_LAST", "{{#things}}{{#-first}}FIRST{{/-first}}{{this}}{{#-last}}LAST{{/-last}}{{/things}}", things(&["_and_"]));
}

#[test]
fn inverse_first_last_combo() {
    test("_and_", "{{#things}}{{^-first}}NOT-FIRST{{/-first}}{{this}}{{^-last}}NOT-LAST{{/-last}}{{/things}}", things(&["_and_"]));
}

#[test]
fn not_first() {
    test("1,2,3", "{{#things}}{{^-first}},{{/-first}}{{this}}{{/things}}", things(&["1", "2", "3"]));
}

#[test]
fn not_last() {
    test("1,2,3", "{{#things}}{{this}}{{^-last}},{{/-last}}{{/things}}", things(&["1", "2", "3"]));
}

#[test]
fn index() {
    test("123", "{{#things}}{{-index}}{{/things}}", things(&["foo", "bar", "baz"]));
}

#[test]
fn nested_index() {
    let tmpl = "{{#fooList}}\n{{#quantity}}|q{{-index}}={{quantity}}{{/quantity}}|{{name}}\n{{/fooList}}";
    test("|q1=1|a\n|b\n", tmpl, context(&[("fooList", list(vec![context(&[("name", s("a")), ("quantity", JValue::Int(1))]), context(&[("name", s("b"))])]))]));
}

#[test]
fn line_reporting() {
    let e = fails(&Compiler::new(), "first line\n{{nonexistent}}\nsecond line", empty());
    assert!(e.is_mustache_exception() && e.message().unwrap().contains("line 2"), "{e}");
}

#[test]
fn standards_mode_with_null_values_in_loop() {
    test("first line\nsecond line", "first line\n{{#nullvalue}}foo\n{{/nullvalue}}\nsecond line", context(&[("nullvalue", JValue::Null)]));
}

#[test]
fn standards_mode_with_null_values_in_inverse_loop() {
    test("first line\nfoo \nsecond line", "first line\n{{^nullvalue}}foo{{/nullvalue}} \nsecond line", context(&[("nullvalue", JValue::Null)]));
}

#[test]
fn standards_mode_with_dot_value() {
    let singleton = JValue::Map(JMap::from_entries(MapKind::Fixed, [(s("foo"), s("bar"))]));
    assert_eq!(Compiler::new().standards_mode(true).compile("{{#foo}}:{{.}}:{{/foo}}").unwrap().execute(singleton).unwrap(), ":bar:");
}

#[test]
fn standards_mode_with_no_parent_context_searching() {
    let e = fails(&Compiler::new().standards_mode(true), "{{#parent}}foo{{parentProperty}}bar{{/parent}}", context(&[("parent", empty()), ("parentProperty", s("bar"))]));
    assert!(e.is_mustache_exception());
}

#[test]
fn missing_value() {
    assert!(fails(&Compiler::new(), "{{missing}} {{notmissing}}", context(&[("notmissing", s("bar"))])).is_mustache_exception());
}

#[test]
fn missing_value_with_default() {
    test_with(&Compiler::new().default_value(""), "bar", "{{missing}}{{notmissing}}", context(&[("notmissing", s("bar"))]));
}

#[test]
fn missing_value_with_default_non_empty_string() {
    test_with(&Compiler::new().default_value("foo"), "foobar", "{{missing}}{{notmissing}}", context(&[("notmissing", s("bar"))]));
}

#[test]
fn missing_value_with_default_substitution() {
    test_with(&Compiler::new().default_value("?{{name}}?"), "?missing?bar", "{{missing}}{{notmissing}}", context(&[("notmissing", s("bar"))]));
}

#[test]
fn missing_value_with_default_substitution2() {
    test_with(&Compiler::new().default_value("{{{{name}}}}"), "{{missing}}bar", "{{missing}}{{notmissing}}", context(&[("notmissing", s("bar"))]));
}

#[test]
fn missing_value_with_default_substitution3() {
    test_with(&Compiler::new().default_value("{{?{{name}}?}}"), "{{?missing?}}bar", "{{missing}}{{notmissing}}", context(&[("notmissing", s("bar"))]));
}

#[test]
fn null_value_gets_default() {
    test_with(&Compiler::new().default_value("foo"), "foobar", "{{nullvar}}{{nonnullvar}}", context(&[("nonnullvar", s("bar")), ("nullvar", JValue::Null)]));
}

#[test]
fn missing_value_with_null_default() {
    assert!(fails(&Compiler::new().null_value(""), "{{missing}}{{notmissing}}", context(&[("notmissing", s("bar"))])).is_mustache_exception());
}

#[test]
fn invalid_triple_mustache() {
    let e = Compiler::new().compile("{{{foo}}").err().expect("parse error");
    assert!(e.is_parse_exception());
    assert_eq!(e.message().unwrap(), "Invalid triple-mustache tag: {{{foo}} @ line 1");
    let e = Compiler::new().compile("{{{foo}}]").err().expect("parse error");
    assert_eq!(e.message().unwrap(), "Invalid triple-mustache tag: {{{foo}}] @ line 1");
}

#[test]
fn null_value_gets_null_default() {
    test_with(&Compiler::new().null_value("foo"), "foobar", "{{nullvar}}{{nonnullvar}}", context(&[("nonnullvar", s("bar")), ("nullvar", JValue::Null)]));
}

#[test]
fn compiling_doesnt_change_compilers_delimiters() {
    let compiler = Compiler::new();
    let ctx = || context(&[("variable", s("value"))]);
    test_with(&compiler, "value", "{{=<% %>=}}<% variable %>", ctx());
    test_with(&compiler, "value", "{{=<% %>=}}<% variable %>", ctx());
}

#[test]
fn lambda1() {
    let bold = lambda(|frag, out| {
        out.push_str("<b>");
        frag.execute_into(out)?;
        out.push_str("</b>");
        Ok(())
    });
    test("<b>Willy is awesome.</b>", "{{#bold}}{{name}} is awesome.{{/bold}}", context(&[("name", s("Willy")), ("bold", bold)]));
}

#[test]
fn lambda2() {
    let l = lambda(|frag, out| {
        out.push_str(match frag.execute()?.as_str() {
            "1" => "Slug",
            "2" => "bug",
            "3" => "potato!",
            _ => "Who was that man?",
        });
        Ok(())
    });
    test("Slug bug potato!", "{{#l}}1{{/l}} {{#l}}2{{/l}} {{#l}}{{three}}{{/l}}", context(&[("three", s("3")), ("l", l)]));
}

struct Invertible;

impl LambdaFn for Invertible {
    fn execute(&self, frag: &Fragment, out: &mut String) -> Result<(), MustacheError> {
        out.push_str("positive = ");
        frag.execute_into(out)
    }

    fn execute_inverse(&self, frag: &Fragment, out: &mut String) -> Option<Result<(), MustacheError>> {
        out.push_str("negative = ");
        Some(frag.execute_into(out))
    }
}

#[test]
fn invertible_lambda() {
    test(
        "positive = positive, negative = negative, simple lambdas do still work",
        "{{#invertible}}positive{{/invertible}}, {{^invertible}}negative{{/invertible}}, simple lambdas do {{^simple}}NOT {{/simple}}still work",
        context(&[("invertible", Lambda::of(Invertible)), ("simple", lambda(|frag, out| frag.execute_into(out)))]),
    );
}

#[test]
fn lambda_with_context() {
    test(
        "a in l1, a in l2",
        "{{#l1}}{{a}}{{/l1}}, {{#l2}}{{a}}{{/l2}}",
        context(&[
            ("l1", lambda(|frag, out| frag.execute_with_into(context(&[("a", s("a in l1"))]), out))),
            ("l2", lambda(|frag, out| frag.execute_with_into(context(&[("a", s("a in l2"))]), out))),
        ]),
    );
}

#[test]
fn lambda_decompile() {
    let lam = || {
        lambda(|frag, out| {
            out.push_str(&frag.decompile());
            Ok(())
        })
    };
    test("Foo {{a}}, Bar {{a}}", "{{#lam}}Foo {{a}}{{/lam}}, {{#lam}}Bar {{a}}{{/lam}}", context(&[("lam", lam())]));
    test("Foo {{a}}, Bar {{a}}", "{{#lam}}Foo {{a}}{{/lam}}, {{#lam}}Bar {{ a }}{{/lam}}", context(&[("lam", lam())]));
    test("Foo {{a}}, Bar {{a}}", "{{#lam}}Foo {{a}}{{/lam}}, {{=(( ))=}}((#lam))Bar ((a))((/lam))", context(&[("lam", lam())]));
    test("{{#section}}\n{{a}}\n{{/section}}", "{{#lam}}{{#section}}\n{{a}}\n{{/section}}{{/lam}}", context(&[("lam", lam())]));
}

#[test]
fn lambda_frag_exec_template() {
    let a = Arc::new(Compiler::new().compile("{{value}} should be A").unwrap());
    let b = Arc::new(Compiler::new().compile("{{value}} should be B").unwrap());
    let lam = lambda(move |frag, out| match frag.execute()?.as_str() {
        "A" => frag.execute_template(&a, out),
        "B" => frag.execute_template(&b, out),
        which => panic!("Invalid fragment content: {which}"),
    });
    let tmpl = "[{{#lam}}{{value}}{{/lam}}]";
    test("[A should be A]", tmpl, context(&[("lam", lam.clone()), ("value", s("A"))]));
    test("[B should be B]", tmpl, context(&[("lam", lam), ("value", s("B"))]));
}

#[test]
fn non_standard_default_delims() {
    test_with(&Compiler::new().with_delims("<% %>").unwrap(), "bar", "<%foo%>", context(&[("foo", s("bar"))]));
}

// ---- MustacheTest --------------------------------------------------------------------------------------------

#[test]
fn field_variable() {
    test("bar", "{{foo}}", Bean::new("Anon$1").field("foo", false, s("bar")).value());
}

#[test]
fn method_variable() {
    test("bar", "{{foo}}", Bean::new("Anon$2").method("foo", Returns::Other, false, s("bar")).value());
}

#[test]
fn property_variable() {
    test("bar", "{{foo}}", Bean::new("Anon$3").method("getFoo", Returns::Other, false, s("bar")).value());
}

#[test]
fn custom_context() {
    test("bar", "{{foo}}", CustomContext::value(|name| Ok(if name == "foo" { s("bar") } else { JValue::Null })));
}

#[test]
fn char_sequence_variable() {
    let mut buffer = Bean::new("java.lang.StringBuffer");
    buffer.to_string = Some("bar".into());
    test("bar", "{{foo}}", context(&[("foo", buffer.value())]));
}

#[test]
fn default_method_variable() {
    let has_default = Members::default().method("getFoo", Returns::Other, true);
    test("bar", "{{foo}}", Bean::new("Anon$4").interface(has_default.clone(), &[("getFoo", s("bar"))]).value());
    // `Interloper extends HasDefault` overrides the default method.
    let interloper = Members::default().method("getFoo", Returns::Other, true);
    test("bang", "{{foo}}", Bean::new("Anon$5").interface(interloper.clone(), &[("getFoo", s("bang"))]).interface(has_default.clone(), &[]).value());
    test(
        "bong",
        "{{foo}}",
        Bean::new("Anon$6").method("getFoo", Returns::Other, true, s("bong")).interface(interloper, &[]).interface(has_default, &[]).value(),
    );
}

#[test]
fn boolean_property_variable() {
    test("true", "{{foo}}", Bean::new("Anon$7").method("isFoo", Returns::Boolean, false, JValue::Bool(true)).value());
}

#[test]
fn primitive_boolean_property_variable() {
    test("false", "{{foo}}", Bean::new("Anon$8").method("isFoo", Returns::Boolean, false, JValue::Bool(false)).value());
}

#[test]
fn skip_void_return() {
    test("bar", "{{foo}}", Bean::new("Anon$9").void_method("foo").method("getFoo", Returns::Other, false, s("bar")).value());
}

#[test]
fn call_site_reuse() {
    let tmpl = Compiler::new().compile("{{foo}}").unwrap();
    let ctx = Bean::new("Anon$10").method("getFoo", Returns::Other, false, s("bar")).value();
    for _ in 0..50 {
        assert_eq!(tmpl.execute(ctx.clone()).unwrap(), "bar");
    }
}

#[test]
fn call_site_change() {
    let tmpl = Compiler::new().compile("{{foo}}").unwrap();
    assert_eq!(tmpl.execute(Bean::new("Anon$11").method("getFoo", Returns::Other, false, s("bar")).value()).unwrap(), "bar");
    assert_eq!(tmpl.execute(Bean::new("Anon$12").field("foo", false, s("bar")).value()).unwrap(), "bar");
}

#[test]
fn section_with_non_falsey_zero() {
    test("test", "{{#foo}}test{{/foo}}", Bean::new("Anon$13").field("foo", false, JValue::Long(0)).value());
}

#[test]
fn section_with_falsey_zero() {
    let ctx = Bean::new("Anon$14")
        .field("intv", false, JValue::Int(0))
        .field("longv", false, JValue::Long(0))
        .field("floatv", false, JValue::Float(0.0))
        .field("doublev", false, JValue::Double(0.0))
        .method("intm", Returns::Other, false, JValue::Int(0))
        .method("longm", Returns::Other, false, JValue::Long(0))
        .method("floatm", Returns::Other, false, JValue::Float(0.0))
        .method("doublem", Returns::Other, false, JValue::Double(0.0))
        .value();
    test_with(
        &Compiler::new().zero_is_false(true),
        "",
        "{{#intv}}intv{{/intv}}{{#longv}}longv{{/longv}}{{#floatv}}floatv{{/floatv}}{{#doublev}}doublev{{/doublev}}{{#longm}}longm{{/longm}}{{#intm}}intm{{/intm}}{{#floatm}}floatm{{/floatm}}{{#doublem}}doublem{{/doublem}}",
        ctx,
    );
}

/// `java.util.Optional`.
fn optional(v: Option<JValue>) -> JValue {
    let mut b = Bean::new("java.util.Optional");
    b.methods.insert("isPresent".into(), Arc::new({
        let p = v.is_some();
        move || Ok(Some(JValue::Bool(p)))
    }));
    if let Some(v) = v {
        b.methods.insert("get".into(), Arc::new(move || Ok(Some(v.clone()))));
    }
    b.value()
}

struct OptionalCollector(DefaultCollector);

impl jmustache::Collector for OptionalCollector {
    fn to_iterator(&self, value: &JValue) -> Option<Vec<JValue>> {
        if let JValue::Object(o) = value {
            if o.class_name() == "java.util.Optional" {
                return Some(match o.invoke("get", &[]) {
                    Some(Ok(Some(v))) => vec![v],
                    _ => vec![],
                });
            }
        }
        self.0.to_iterator(value)
    }

    fn fetch(&self, ctx: &JValue, name: &str) -> Result<jmustache::Fetched, JError> {
        self.0.fetch(ctx, name)
    }
}

#[test]
fn optional_supporting_collector() {
    let comp = Compiler::new().with_collector(Arc::new(OptionalCollector(DefaultCollector::default())));
    test_with(&comp, "test", "{{#foo}}{{.}}{{/foo}}", context(&[("foo", optional(Some(s("test"))))]));
    test_with(&comp, "", "{{#foo}}{{.}}{{/foo}}", context(&[("foo", optional(None))]));
    test_with(&comp, "", "{{^foo}}{{.}}{{/foo}}", context(&[("foo", optional(Some(s("test"))))]));
    test_with(&comp, "test", "{{^foo}}test{{/foo}}", context(&[("foo", optional(None))]));
}

#[test]
fn compound_variable() {
    let baz = Bean::new("Anon$16").field("baz", false, s("hello")).value();
    let bar = Bean::new("Anon$15").field("bar", false, baz).value();
    test("hello", "{{foo.bar.baz}}", Bean::new("Anon$17").method("foo", Returns::Other, false, bar).value());
}

#[test]
fn null_component_in_compound_variable() {
    let foo = Bean::new("Anon$18").field("bar", false, JValue::Null).value();
    assert!(fails(&Compiler::new(), "{{foo.bar.baz}}", Bean::new("Anon$19").field("foo", false, foo).value()).is_mustache_exception());
}

#[test]
fn missing_component_in_compound_variable() {
    let foo = Bean::new("java.lang.Object").value();
    assert!(fails(&Compiler::new(), "{{foo.bar.baz}}", Bean::new("Anon$20").field("foo", false, foo).value()).is_mustache_exception());
}

#[test]
fn null_component_in_compound_variable_with_default() {
    test_with(&Compiler::new().null_value("null"), "null", "{{foo.bar.baz}}", Bean::new("Anon$21").field("foo", false, JValue::Null).value());
    let foo = Bean::new("Anon$22").field("bar", false, JValue::Null).value();
    test_with(&Compiler::new().null_value("null"), "null", "{{foo.bar.baz}}", Bean::new("Anon$23").field("foo", false, foo).value());
}

#[test]
fn missing_component_in_compound_variable_with_default() {
    test_with(&Compiler::new().default_value("?"), "?", "{{foo.bar.baz}}", Bean::new("Anon$24").value());
    let foo = Bean::new("java.lang.Object").value();
    test_with(&Compiler::new().default_value("?"), "?", "{{foo.bar.baz}}", Bean::new("Anon$25").field("foo", false, foo).value());
}

#[test]
fn compound_variable_as_plain() {
    let foo = Bean::new("Anon$26").field("bar", false, s("hello")).value();
    test("wholekey", "{{foo.bar}}", context(&[("foo.bar", s("wholekey")), ("foo", foo)]));
}

#[test]
fn shadowed_context_with_null() {
    let comp = Compiler::new().null_value("(null)");
    let (tmpl, expect) = ("{{foo}}{{#inner}}{{foo}}{{/inner}}", "outer(null)");
    let inner = Bean::new("Anon$27").field("foo", true, JValue::Null).value();
    test_with(&comp, expect, tmpl, Bean::new("Anon$28").field("foo", true, s("outer")).field("inner", true, inner).value());
    test_with(&comp, expect, tmpl, context(&[("foo", s("outer")), ("inner", context(&[("foo", JValue::Null)]))]));
}

#[test]
fn context_poking_lambda() {
    let c = Compiler::new();
    let lfoo = Arc::new(c.compile("{{foo}}").unwrap());
    let foo = || Bean::new("MustacheTest$1Foo").field("foo", true, JValue::Int(1)).value();
    assert_eq!(lfoo.execute(foo()).unwrap(), "1");
    let lbar = Arc::new(c.compile("{{bar}}").unwrap());
    let bar = || Bean::new("MustacheTest$1Bar").field("bar", true, s("one")).value();
    assert_eq!(lbar.execute(bar()).unwrap(), "one");
    let render = lambda(move |frag, out| {
        let ctx = frag.context();
        match ctx.class_name().as_str() {
            "MustacheTest$1Foo" => lfoo.execute_into(ctx, out),
            "MustacheTest$1Bar" => lbar.execute_into(ctx, out),
            _ => {
                out.push('!');
                Ok(())
            }
        }
    });
    test_with(
        &c,
        "1oneone1one!",
        "{{#events}}{{#renderEvent}}{{/renderEvent}}{{/events}}",
        context(&[("renderEvent", render), ("events", list(vec![foo(), bar(), bar(), foo(), bar(), s("wtf?")]))]),
    );
}

/// A `java.util.Date`.
fn date(millis: i64) -> JValue {
    let mut b = Bean::new("java.util.Date");
    b.methods.insert("getTime".into(), Arc::new(move || Ok(Some(JValue::Long(millis)))));
    b.value()
}

struct DateFormatter;

impl Formatter for DateFormatter {
    fn format(&self, value: &JValue) -> String {
        if value.class_name() == "java.util.Date" {
            let millis = match java_compat::methods::invoke(value, "getTime", &[]) {
                Some(Ok(Some(JValue::Long(m)))) => m,
                _ => 0,
            };
            // `SimpleDateFormat("yyyy/MM/dd")` at UTC.
            let days = millis.div_euclid(86_400_000);
            let (y, m, d) = civil_from_days(days);
            return format!("{y:04}/{m:02}/{d:02}");
        }
        value.to_java_string()
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian date.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[test]
fn custom_formatter() {
    let ctx = Bean::new("Anon$29").field("msg", false, s("Date")).field("today", false, date(1389208567874)).value();
    assert_eq!(Compiler::new().with_formatter(Arc::new(DateFormatter)).compile("{{msg}}: {{today}}").unwrap().execute(ctx).unwrap(), "Date: 2014/01/08");
}

#[test]
fn map_entries_plus_reflect_section() {
    let data = JMap::new(MapKind::Hash);
    data.put_str("k1", s("v1"));
    data.put_str("k2", s("v2"));
    test("k1v1k2v2", "{{#map.entrySet}}{{key}}{{value}}{{/map.entrySet}}", context(&[("map", JValue::Map(data))]));
}

#[test]
fn no_access_coercion() {
    let baz = Bean::new("Anon$30").field("baz", true, s("hello")).field("quux", false, s("hello")).value();
    let bar = Bean::new("Anon$31").field("bar", true, baz).value();
    let ctx = Bean::new("Anon$32").method("foo", Returns::Other, true, bar).value();
    test("hello:hello", "{{foo.bar.baz}}:{{foo.bar.quux}}", ctx.clone());
    let comp = Compiler::new().with_collector(Arc::new(DefaultCollector::new(false))).default_value("missing");
    test_with(&comp, "hello:missing", "{{foo.bar.baz}}:{{foo.bar.quux}}", ctx);
}

