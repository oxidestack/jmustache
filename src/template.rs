//! `Template`: executing segments against a context chain, `Template.Fragment` for lambdas, visiting.

use java_compat::lang::{JError, JList, JValue};

use crate::collector::Fetched;
use crate::parse::{Section, SectionKind, Segment};
use crate::{Compiler, Lambda, MustacheError, Visitor};

/// A compiled template; immutable, so it may be executed from several threads at once.
pub struct Template {
    segs: Vec<Segment>,
    compiler: Compiler,
}

/// `Template.Context`: the data at one nesting level and the loop state of the section that created it.
pub struct Context<'a> {
    pub data: JValue,
    pub parent: Option<&'a Context<'a>>,
    pub index: i32,
    pub on_first: bool,
    pub on_last: bool,
}

impl<'a> Context<'a> {
    fn nest(&'a self, data: JValue) -> Context<'a> {
        Context { data, parent: Some(self), index: self.index, on_first: self.on_first, on_last: self.on_last }
    }
}

/// `Template.Fragment`: a lambda's section, bound to the context the lambda was called in.
pub struct Fragment<'a> {
    template: &'a Template,
    segs: &'a [Segment],
    ctx: &'a Context<'a>,
}

impl<'a> Fragment<'a> {
    /// `execute(out)`.
    pub fn execute_into(&self, out: &mut String) -> Result<(), MustacheError> {
        self.template.run(self.segs, self.ctx, out)
    }

    /// `execute()`: the fragment's output.
    pub fn execute(&self) -> Result<String, MustacheError> {
        let mut out = String::new();
        self.execute_into(&mut out)?;
        Ok(out)
    }

    /// `execute(context, out)`: with `context` nested in the fragment's context.
    pub fn execute_with_into(&self, context: JValue, out: &mut String) -> Result<(), MustacheError> {
        let nested = self.ctx.nest(context);
        self.template.run(self.segs, &nested, out)
    }

    /// `execute(context)`.
    pub fn execute_with(&self, context: JValue) -> Result<String, MustacheError> {
        let mut out = String::new();
        self.execute_with_into(context, &mut out)?;
        Ok(out)
    }

    /// `executeTemplate(tmpl, out)`: another template in this fragment's context.
    pub fn execute_template(&self, tmpl: &Template, out: &mut String) -> Result<(), MustacheError> {
        tmpl.execute_segs(self.ctx, out)
    }

    /// `context()`.
    pub fn context(&self) -> JValue {
        self.ctx.data.clone()
    }

    /// `context(n)`: the `n`th parent context; a missing parent throws a `NullPointerException`.
    pub fn context_n(&self, n: usize) -> Result<JValue, MustacheError> {
        let mut c = Some(self.ctx);
        for _ in 0..n {
            c = c.and_then(|c| c.parent);
        }
        c.map(|c| c.data.clone()).ok_or_else(|| MustacheError::Java(JError::without_message("java.lang.NullPointerException")))
    }

    /// `decompile()`: an approximation of the fragment's source, with the compiler's delimiters.
    pub fn decompile(&self) -> String {
        let mut into = String::new();
        for seg in self.segs {
            seg.decompile(&self.template.compiler.delims, &mut into);
        }
        into
    }
}

enum Lookup {
    Value(JValue),
    Missing,
}

impl Template {
    pub(crate) fn new(segs: Vec<Segment>, compiler: Compiler) -> Template {
        Template { segs, compiler }
    }

    /// `execute(context)`.
    pub fn execute(&self, context: JValue) -> Result<String, MustacheError> {
        let mut out = String::new();
        self.execute_into(context, &mut out)?;
        Ok(out)
    }

    /// `execute(context, out)`.
    pub fn execute_into(&self, context: JValue, out: &mut String) -> Result<(), MustacheError> {
        let ctx = Context { data: context, parent: None, index: 0, on_first: false, on_last: false };
        self.execute_segs(&ctx, out)
    }

    /// `execute(context, parentContext, out)`.
    pub fn execute_with_parent(&self, context: JValue, parent: JValue, out: &mut String) -> Result<(), MustacheError> {
        let pctx = Context { data: parent, parent: None, index: 0, on_first: false, on_last: false };
        let ctx = Context { data: context, parent: Some(&pctx), index: 0, on_first: false, on_last: false };
        self.execute_segs(&ctx, out)
    }

    /// `visit(visitor)`.
    pub fn visit(&self, visitor: &mut dyn Visitor) -> Result<(), MustacheError> {
        for seg in &self.segs {
            seg.visit(visitor)?;
        }
        Ok(())
    }

    /// `indent(indent)`: this template re-indented for a standalone partial.
    pub(crate) fn indent(self, indent: &str) -> Template {
        match crate::parse::indent_segs(&self.segs, indent, false, false) {
            Some(segs) => Template::new(segs, self.compiler),
            None => self,
        }
    }

    /// `replaceBlocks(blocks)`.
    pub(crate) fn replace_blocks(self, blocks: &[(String, Section)]) -> Template {
        match crate::parse::replace_block_segs(&self.segs, blocks) {
            Some(segs) => Template::new(segs, self.compiler),
            None => self,
        }
    }

    pub(crate) fn execute_segs(&self, ctx: &Context, out: &mut String) -> Result<(), MustacheError> {
        self.run(&self.segs, ctx, out)
    }

    fn run(&self, segs: &[Segment], ctx: &Context, out: &mut String) -> Result<(), MustacheError> {
        for seg in segs {
            seg.execute(self, ctx, out)?;
        }
        Ok(())
    }

    pub(crate) fn execute_variable(&self, ctx: &Context, name: &str, line: usize, escape: bool, out: &mut String) -> Result<(), MustacheError> {
        let value = self.value_or_default(ctx, name, line)?;
        let Some(value) = value else {
            let message = if is_this_name(name) {
                format!("Resolved '.' to null (which is disallowed), on line {line}")
            } else {
                format!("No key, method or field with name '{name}' on line {line}")
            };
            return Err(MustacheError::context(message));
        };
        let text = self.compiler.formatter.format(&value);
        out.push_str(&if escape { self.compiler.escaper.escape(&text) } else { text });
        Ok(())
    }

    pub(crate) fn execute_section(&self, s: &Section, ctx: &Context, out: &mut String) -> Result<(), MustacheError> {
        match s.kind {
            SectionKind::Block => self.run(&s.segs, ctx, out),
            SectionKind::Section => {
                let value = self.section_value(ctx, &s.name, s.line)?;
                if let Some(items) = self.compiler.collector.to_iterator(&value) {
                    let count = items.len();
                    for (i, elem) in items.into_iter().enumerate() {
                        let nested = Context { data: elem, parent: Some(ctx), index: i as i32 + 1, on_first: i == 0, on_last: i + 1 == count };
                        self.run(&s.segs, &nested, out)?;
                    }
                } else if let JValue::Bool(b) = value {
                    if b {
                        self.run(&s.segs, ctx, out)?;
                    }
                } else if let Some(lambda) = as_lambda(&value) {
                    lambda.f.execute(&Fragment { template: self, segs: &s.segs, ctx }, out)?;
                } else if self.compiler.is_falsey(&value) {
                } else {
                    let nested = ctx.nest(value);
                    self.run(&s.segs, &nested, out)?;
                }
                Ok(())
            }
            SectionKind::Inverted => {
                let value = self.section_value(ctx, &s.name, s.line)?;
                if let Some(items) = self.compiler.collector.to_iterator(&value) {
                    if items.is_empty() {
                        self.run(&s.segs, ctx, out)?;
                    }
                } else if let JValue::Bool(b) = value {
                    if !b {
                        self.run(&s.segs, ctx, out)?;
                    }
                } else if let Some(r) = as_lambda(&value).and_then(|l| l.f.execute_inverse(&Fragment { template: self, segs: &s.segs, ctx }, out)) {
                    r?;
                } else if self.compiler.is_falsey(&value) {
                    self.run(&s.segs, ctx, out)?;
                }
                Ok(())
            }
        }
    }

    /// `getSectionValue`: a null value is an empty list.
    fn section_value(&self, ctx: &Context, name: &str, line: usize) -> Result<JValue, MustacheError> {
        match self.value(ctx, name, line, !self.compiler.strict_sections)? {
            Lookup::Value(JValue::Null) | Lookup::Missing => Ok(JValue::List(JList::array_list(Vec::new()))),
            Lookup::Value(v) => Ok(v),
        }
    }

    /// `getValueOrDefault`: `None` when there is neither a value nor a default.
    fn value_or_default(&self, ctx: &Context, name: &str, line: usize) -> Result<Option<JValue>, MustacheError> {
        match self.value(ctx, name, line, self.compiler.missing_is_null)? {
            Lookup::Value(JValue::Null) | Lookup::Missing => Ok(self.compiler.compute_null_value(name).map(JValue::str)),
            Lookup::Value(v) => Ok(Some(v)),
        }
    }

    /// `getValue`: `Missing` stands for a null result after `missingIsNull`.
    fn value(&self, ctx: &Context, name: &str, line: usize, missing_is_null: bool) -> Result<Lookup, MustacheError> {
        match name {
            "-first" => return Ok(Lookup::Value(JValue::Bool(ctx.on_first))),
            "-last" => return Ok(Lookup::Value(JValue::Bool(ctx.on_last))),
            "-index" => return Ok(Lookup::Value(JValue::Int(ctx.index))),
            _ => {}
        }
        if self.compiler.standards_mode {
            let v = self.value_in(&ctx.data, name, line)?;
            return check_for_missing(name, line, missing_is_null, v);
        }
        let mut pctx = Some(ctx);
        while let Some(c) = pctx {
            if let Fetched::Value(v) = self.value_in(&c.data, name, line)? {
                return Ok(Lookup::Value(v));
            }
            pctx = c.parent;
        }
        if name != "." && name.contains('.') {
            self.compound_value(ctx, name, line, missing_is_null)
        } else {
            check_for_missing(name, line, missing_is_null, Fetched::NotFound)
        }
    }

    fn compound_value(&self, ctx: &Context, name: &str, line: usize, missing_is_null: bool) -> Result<Lookup, MustacheError> {
        let comps = java_split_dot(name);
        let mut data = match self.value(ctx, comps[0], line, missing_is_null)? {
            Lookup::Value(v) => Fetched::Value(v),
            Lookup::Missing => Fetched::Value(JValue::Null),
        };
        for ii in 1..comps.len() {
            match data {
                Fetched::NotFound => {
                    if !missing_is_null {
                        return Err(MustacheError::context(format!(
                            "Missing context for compound variable '{name}' on line {line}. '{}' was not found.",
                            comps[ii - 1]
                        )));
                    }
                    return Ok(Lookup::Missing);
                }
                Fetched::Value(JValue::Null) => return Ok(Lookup::Missing),
                Fetched::Value(v) => data = self.value_in(&v, comps[ii], line)?,
            }
        }
        check_for_missing(name, line, missing_is_null, data)
    }

    /// `getValueIn`.
    fn value_in(&self, data: &JValue, name: &str, line: usize) -> Result<Fetched, MustacheError> {
        if is_this_name(name) {
            return Ok(Fetched::Value(data.clone()));
        }
        if data.is_null() {
            return Err(MustacheError::Java(JError::new("java.lang.NullPointerException", format!("Null context for variable '{name}' on line {line}"))));
        }
        self.compiler.collector.fetch(data, name).map_err(|_| MustacheError::context(format!("Failure fetching variable '{name}' on line {line}")))
    }
}

fn check_for_missing(name: &str, line: usize, missing_is_null: bool, value: Fetched) -> Result<Lookup, MustacheError> {
    match value {
        Fetched::NotFound if missing_is_null => Ok(Lookup::Missing),
        Fetched::NotFound => Err(MustacheError::context(format!("No method or field with name '{name}' on line {line}"))),
        Fetched::Value(v) => Ok(Lookup::Value(v)),
    }
}

fn is_this_name(name: &str) -> bool {
    name == "." || name == "this"
}

/// `name.split("\\.")`: trailing empty components dropped.
fn java_split_dot(name: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = name.split('.').collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

fn as_lambda(v: &JValue) -> Option<&Lambda> {
    match v {
        JValue::Object(o) => o.as_any().downcast_ref::<Lambda>(),
        _ => None,
    }
}
