//! `Mustache.Parser`, `Mustache.Accumulator`, the segment classes and `Mustache.trim` / `indentSegs` /
//! `replaceBlockSegs`: text into segments, standalone tags trimmed, partials re-indented, parent blocks replaced.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::template::Template;
use crate::{Compiler, MustacheError, Visitor};

/// No second delimiter character (`Mustache.NO_CHAR`).
pub(crate) const NO_CHAR: char = '\0';

#[derive(Debug, Clone, Copy)]
pub(crate) struct Delims {
    start1: char,
    start2: char,
    end1: char,
    end2: char,
}

impl Default for Delims {
    fn default() -> Delims {
        Delims { start1: '{', start2: '{', end1: '}', end2: '}' }
    }
}

impl Delims {
    fn is_staches(&self) -> bool {
        self.start1 == '{' && self.start2 == '{' && self.end1 == '}' && self.end2 == '}'
    }

    /// `updateDelims(dtext)`.
    pub(crate) fn update(&mut self, dtext: &str) -> Result<(), MustacheError> {
        let errmsg = || {
            MustacheError::mustache(format!(
                "Invalid delimiter configuration '{dtext}'. Must be of the form {{{{=1 2=}}}} or {{{{=12 34=}}}} where 1, 2, 3 and 4 are delimiter chars."
            ))
        };
        let parts: Vec<&str> = java_split_space(dtext);
        if parts.len() != 2 {
            return Err(errmsg());
        }
        let pick = |s: &str| -> Result<(char, char), MustacheError> {
            let cs: Vec<char> = s.chars().collect();
            match cs.len() {
                1 => Ok((cs[0], NO_CHAR)),
                2 => Ok((cs[0], cs[1])),
                _ => Err(errmsg()),
            }
        };
        let (s1, s2) = pick(parts[0])?;
        let (e1, e2) = pick(parts[1])?;
        *self = Delims { start1: s1, start2: s2, end1: e1, end2: e2 };
        Ok(())
    }

    /// Re-inserts the consumed start delimiter in front of `text` (`restoreStartTag`).
    fn restore_start(&self, text: &mut String) {
        let mut prefix = String::new();
        prefix.push(self.start1);
        if self.start2 != NO_CHAR {
            prefix.push(self.start2);
        }
        text.insert_str(0, &prefix);
    }

    /// `addTag(prefix, name, into)`.
    pub(crate) fn add_tag(&self, prefix: char, name: &str, into: &mut String) {
        into.push(self.start1);
        into.push(self.start2);
        if prefix != ' ' {
            into.push(prefix);
        }
        into.push_str(name);
        into.push(self.end1);
        into.push(self.end2);
    }
}

/// `String.split(" ")`: trailing empty strings removed, leading ones kept.
fn java_split_space(s: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(' ').collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts == [""] && !s.is_empty() {
        parts.clear();
    }
    parts
}

/// `Character.isWhitespace`.
pub(crate) fn java_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{1c}'..='\u{1f}')
        || (c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}' | '\u{85}'))
}

/// `String.trim()`: every char up to U+0020 at either end.
pub(crate) fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// `Mustache.StringSegment`: text with the positions of its leading and trailing blank line parts.
#[derive(Debug, Clone)]
pub struct StringSegment {
    pub(crate) text: String,
    lead: isize,
    trail: isize,
    first: bool,
}

impl StringSegment {
    /// The segment of `text`; `first` when it is the template's first segment, which a blank line part may start.
    pub fn new(text: &str, first: bool) -> StringSegment {
        let lead = blank_pos(text, true, first);
        let trail = blank_pos(text, false, first);
        StringSegment { text: text.to_string(), lead, trail, first }
    }

    fn leads_blank(&self) -> bool {
        self.lead != -1
    }

    fn trails_blank(&self) -> bool {
        self.trail != -1
    }

    /// Byte offset of a char index.
    fn byte(&self, char_index: isize) -> usize {
        self.text.char_indices().nth(char_index as usize).map(|(b, _)| b).unwrap_or(self.text.len())
    }

    /// `trimLeadBlank()`: the segment without its leading blank line part.
    pub fn trim_lead_blank(&self) -> StringSegment {
        if self.lead == -1 {
            return self.clone();
        }
        let lpos = self.lead + 1;
        let new_trail = if self.trail == -1 { -1 } else { self.trail - lpos };
        StringSegment { text: self.text[self.byte(lpos)..].to_string(), lead: -1, trail: new_trail, first: self.first }
    }

    /// `trimTrailBlank()`: the segment without its trailing blank line part.
    pub fn trim_trail_blank(&self) -> StringSegment {
        if self.trail == -1 {
            return self.clone();
        }
        StringSegment { text: self.text[..self.byte(self.trail)].to_string(), lead: self.lead, trail: -1, first: self.first }
    }

    /// `indent()`: the blank text after the last newline, the indentation of a standalone partial.
    fn indent(&self) -> String {
        if self.trail == -1 || self.trail as usize >= self.text.chars().count() {
            return String::new();
        }
        self.text[self.byte(self.trail)..].to_string()
    }

    fn reindented(&self, indent: &str, first: bool, last: bool) -> StringSegment {
        StringSegment::new(&reindent(&self.text, indent, first, last), self.first)
    }
}

impl std::fmt::Display for StringSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Text({}){}/{}", self.text.replace('\r', "\\r").replace('\n', "\\n"), self.lead, self.trail)
    }
}

/// `StringSegment.reindent`: the indent after every newline (and first, and after a final newline if `last`).
fn reindent(input: &str, indent: &str, first: bool, last: bool) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut sb = String::with_capacity(indent.len() + input.len());
    if first {
        sb.push_str(indent);
    }
    for (ii, c) in chars.iter().enumerate() {
        sb.push(*c);
        if *c == '\n' && (last || ii != chars.len() - 1) {
            sb.push_str(indent);
        }
    }
    sb
}

/// `StringSegment.blankPos`, in chars.
fn blank_pos(text: &str, leading: bool, first: bool) -> isize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len() as isize;
    let range: Box<dyn Iterator<Item = isize>> = if leading { Box::new(0..len) } else { Box::new((0..len).rev()) };
    for ii in range {
        let c = chars[ii as usize];
        if c == '\n' {
            return if leading { ii } else { ii + 1 };
        }
        if !java_whitespace(c) {
            return -1;
        }
    }
    if leading || !first { -1 } else { 0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SectionKind {
    Section,
    Inverted,
    Block,
}

/// `SectionSegment`, `InvertedSegment` and `BlockSegment`.
#[derive(Clone)]
pub(crate) struct Section {
    pub kind: SectionKind,
    pub name: String,
    pub line: usize,
    pub segs: Vec<Segment>,
    standalone_start: bool,
    standalone_end: bool,
}

impl Section {
    fn new(kind: SectionKind, name: String, segs: Vec<Segment>, line: usize) -> Section {
        Section { kind, name, line, segs: trim(segs, false), standalone_start: false, standalone_end: false }
    }

    /// The copy constructor (`AbstractSectionSegment(original, segs)`): the standalone flags are not copied.
    fn copy(&self, segs: Vec<Segment>) -> Section {
        Section { kind: self.kind, name: self.name.clone(), line: self.line, segs, standalone_start: false, standalone_end: false }
    }
}

/// A partial's lazily loaded template (`AbstractPartialSegment._template`), shared by the copies that are the
/// same Java object.
type Loaded = Arc<Mutex<Option<Arc<Template>>>>;

/// `IncludedTemplateSegment`.
#[derive(Clone)]
pub(crate) struct Include {
    pub name: String,
    pub line: usize,
    indent: String,
    standalone: bool,
    compiler: Compiler,
    loaded: Loaded,
}

/// `ParentTemplateSegment`.
#[derive(Clone)]
pub(crate) struct Parent {
    pub name: String,
    pub line: usize,
    indent: String,
    segs: Vec<Segment>,
    standalone_start: bool,
    standalone_end: bool,
    /// Blocks passed down by an enclosing parent (`_blocks`, a `LinkedHashMap`).
    blocks: Vec<(String, Section)>,
    compiler: Compiler,
    loaded: Loaded,
}

#[derive(Clone)]
pub(crate) enum Segment {
    Text(StringSegment),
    Variable { name: String, line: usize, escape: bool },
    Section(Section),
    Include(Include),
    Parent(Parent),
    /// Comments and delimiter changes: print nothing, but take part in trimming.
    Faux,
}

fn put_block(map: &mut Vec<(String, Section)>, name: &str, block: Section) {
    match map.iter_mut().find(|(k, _)| k == name) {
        Some(e) => e.1 = block,
        None => map.push((name.to_string(), block)),
    }
}

impl Include {
    fn template(&self) -> Result<Arc<Template>, MustacheError> {
        let mut loaded = self.loaded.lock();
        if let Some(t) = &*loaded {
            return Ok(t.clone());
        }
        let t = Arc::new(self.compiler.load_template(&self.name)?.indent(&self.indent));
        *loaded = Some(t.clone());
        Ok(t)
    }
}

impl Parent {
    fn template(&self) -> Result<Arc<Template>, MustacheError> {
        let mut loaded = self.loaded.lock();
        if let Some(t) = &*loaded {
            return Ok(t.clone());
        }
        let mut blocks: Vec<(String, Section)> = Vec::new();
        for seg in &self.segs {
            if let Segment::Section(s) = seg {
                if s.kind == SectionKind::Block {
                    put_block(&mut blocks, &s.name, s.clone());
                }
            }
        }
        for (k, v) in &self.blocks {
            put_block(&mut blocks, k, v.clone());
        }
        let t = Arc::new(self.compiler.load_template(&self.name)?.indent(&self.indent).replace_blocks(&blocks));
        *loaded = Some(t.clone());
        Ok(t)
    }

    /// `ParentTemplateSegment(original, segs, indent, blocks)`.
    fn derive(&self, segs: Vec<Segment>, indent: &str, blocks: &[(String, Section)]) -> Parent {
        let mut merged = self.blocks.clone();
        for (k, v) in blocks {
            put_block(&mut merged, k, v.clone());
        }
        Parent {
            name: self.name.clone(),
            line: self.line,
            indent: format!("{indent}{}", self.indent),
            segs,
            standalone_start: self.standalone_start,
            standalone_end: self.standalone_end,
            blocks: merged,
            compiler: self.compiler.clone(),
            loaded: Arc::new(Mutex::new(None)),
        }
    }
}

impl Segment {
    fn as_text(&self) -> Option<&StringSegment> {
        match self {
            Segment::Text(t) => Some(t),
            _ => None,
        }
    }

    /// `isStandalone()`.
    fn is_standalone(&self) -> bool {
        match self {
            Segment::Text(_) | Segment::Variable { .. } => false,
            Segment::Section(s) => s.standalone_end,
            Segment::Include(i) => i.standalone,
            Segment::Parent(p) => p.standalone_end,
            Segment::Faux => true,
        }
    }

    /// `isStandaloneStart()` of an `AbstractSectionSegment` (sections, inverted sections, blocks).
    fn section_standalone_start(&self) -> Option<bool> {
        match self {
            Segment::Section(s) => Some(s.standalone_start),
            _ => None,
        }
    }

    /// `indent(indent, first, last)`: `None` when the segment is unchanged (the same object).
    fn indent(&self, indent: &str, first: bool, last: bool) -> Option<Segment> {
        match self {
            Segment::Text(t) => (!indent.is_empty()).then(|| Segment::Text(t.reindented(indent, first, last))),
            Segment::Variable { .. } | Segment::Faux => None,
            Segment::Section(s) => indent_segs(&s.segs, indent, first, last).map(|segs| Segment::Section(s.copy(segs))),
            Segment::Include(i) => {
                if indent.is_empty() || !i.standalone {
                    return None;
                }
                Some(Segment::Include(Include {
                    name: i.name.clone(),
                    line: i.line,
                    indent: format!("{indent}{}", i.indent),
                    standalone: i.standalone,
                    compiler: i.compiler.clone(),
                    loaded: Arc::new(Mutex::new(None)),
                }))
            }
            Segment::Parent(p) => {
                if indent.is_empty() || !p.standalone_start {
                    return None;
                }
                Some(Segment::Parent(p.derive(p.segs.clone(), indent, &[])))
            }
        }
    }

    /// `replaceBlocks(blocks)` of a `BlockReplaceable`: `None` when unchanged.
    fn replace_blocks(&self, blocks: &[(String, Section)]) -> Option<Segment> {
        match self {
            Segment::Section(s) if s.kind == SectionKind::Block => match blocks.iter().find(|(k, _)| *k == s.name) {
                Some((_, b)) => Some(Segment::Section(s.copy(b.segs.clone()))),
                None => replace_block_segs(&s.segs, blocks).map(|segs| Segment::Section(s.copy(segs))),
            },
            Segment::Section(s) => replace_block_segs(&s.segs, blocks).map(|segs| Segment::Section(s.copy(segs))),
            Segment::Parent(p) => Some(Segment::Parent(p.derive(p.segs.clone(), "", blocks))),
            _ => None,
        }
    }

    pub(crate) fn execute(&self, tmpl: &Template, ctx: &crate::template::Context, out: &mut String) -> Result<(), MustacheError> {
        match self {
            Segment::Text(t) => {
                out.push_str(&t.text);
                Ok(())
            }
            Segment::Faux => Ok(()),
            Segment::Variable { name, line, escape } => tmpl.execute_variable(ctx, name, *line, *escape, out),
            Segment::Section(s) => tmpl.execute_section(s, ctx, out),
            // Partials run in the caller's context, with their own template's segments.
            Segment::Include(i) => i.template()?.execute_segs(ctx, out),
            Segment::Parent(p) => p.template()?.execute_segs(ctx, out),
        }
    }

    pub(crate) fn decompile(&self, delims: &Delims, into: &mut String) {
        match self {
            Segment::Text(t) => into.push_str(&t.text),
            Segment::Faux => {}
            Segment::Variable { name, .. } => delims.add_tag(' ', name, into),
            // Upstream decompiles an include with `<`, like a parent.
            Segment::Include(i) => delims.add_tag('<', &i.name, into),
            Segment::Parent(p) => delims.add_tag('<', &p.name, into),
            Segment::Section(s) => {
                let prefix = match s.kind {
                    SectionKind::Section => '#',
                    SectionKind::Inverted => '^',
                    SectionKind::Block => '$',
                };
                delims.add_tag(prefix, &s.name, into);
                for seg in &s.segs {
                    seg.decompile(delims, into);
                }
                delims.add_tag('/', &s.name, into);
            }
        }
    }

    pub(crate) fn visit(&self, visitor: &mut dyn Visitor) -> Result<(), MustacheError> {
        match self {
            Segment::Text(t) => visitor.visit_text(&t.text),
            Segment::Faux => {}
            Segment::Variable { name, .. } => visitor.visit_variable(name),
            Segment::Include(i) => {
                if visitor.visit_include(&i.name) {
                    i.template()?.visit(visitor)?;
                }
            }
            Segment::Parent(p) => {
                if visitor.visit_parent(&p.name) {
                    p.template()?.visit(visitor)?;
                }
            }
            Segment::Section(s) => {
                let go = match s.kind {
                    SectionKind::Section => visitor.visit_section(&s.name),
                    SectionKind::Inverted => visitor.visit_inverted_section(&s.name),
                    SectionKind::Block => visitor.visit_block(&s.name),
                };
                if go {
                    for seg in &s.segs {
                        seg.visit(visitor)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// `StandaloneSection` over a section or a parent.
trait Standalone {
    fn segs(&self) -> &Vec<Segment>;
    fn segs_mut(&mut self) -> &mut Vec<Segment>;
    fn is_parent(&self) -> bool;
    fn set_start(&mut self);
    fn set_end(&mut self);

    fn first_leads_blank(&self) -> bool {
        matches!(self.segs().first(), Some(Segment::Text(t)) if t.leads_blank())
    }

    fn trim_first_blank(&mut self) {
        if let Some(Segment::Text(t)) = self.segs_mut().first_mut() {
            *t = t.trim_lead_blank();
        }
    }

    fn last_trails_blank(&self) -> bool {
        // An empty parent still owns the newline after its close tag.
        if self.is_parent() && self.segs().is_empty() {
            return true;
        }
        matches!(self.segs().last(), Some(Segment::Text(t)) if t.trails_blank())
    }

    fn trim_last_blank(&mut self) {
        if let Some(Segment::Text(t)) = self.segs_mut().last_mut() {
            *t = t.trim_trail_blank();
        }
    }
}

impl Standalone for Section {
    fn segs(&self) -> &Vec<Segment> {
        &self.segs
    }
    fn segs_mut(&mut self) -> &mut Vec<Segment> {
        &mut self.segs
    }
    fn is_parent(&self) -> bool {
        false
    }
    fn set_start(&mut self) {
        self.standalone_start = true;
    }
    fn set_end(&mut self) {
        self.standalone_end = true;
    }
}

impl Standalone for Parent {
    fn segs(&self) -> &Vec<Segment> {
        &self.segs
    }
    fn segs_mut(&mut self) -> &mut Vec<Segment> {
        &mut self.segs
    }
    fn is_parent(&self) -> bool {
        true
    }
    fn set_start(&mut self) {
        self.standalone_start = true;
    }
    fn set_end(&mut self) {
        self.standalone_end = true;
    }
}

/// `Mustache.trim(segs, top)`.
pub(crate) fn trim(mut segs: Vec<Segment>, top: bool) -> Vec<Segment> {
    let len = segs.len();
    for ii in 0..len {
        let prev = if ii > 0 { segs[ii - 1].as_text().cloned() } else { None };
        let next = if ii + 1 < len { segs[ii + 1].as_text().cloned() } else { None };
        let prev_blank = (ii == 0 && top) || prev.as_ref().is_some_and(StringSegment::trails_blank);
        let next_blank = (ii + 1 == len && top) || next.as_ref().is_some_and(StringSegment::leads_blank);
        let (first, last) = (ii == 0, ii + 1 == len);
        let standalone: Option<&mut dyn Standalone> = match &mut segs[ii] {
            Segment::Section(s) => Some(s),
            Segment::Parent(p) => Some(p),
            _ => None,
        };
        if let Some(sect) = standalone {
            let mut indent = String::new();
            let mut trim_prev = false;
            let mut trim_next = false;
            if prev_blank && sect.first_leads_blank() {
                if let Some(p) = &prev {
                    indent = p.indent();
                    trim_prev = true;
                }
                sect.trim_first_blank();
                sect.set_start();
            }
            if next_blank && sect.last_trails_blank() {
                sect.trim_last_blank();
                trim_next = next.is_some();
                sect.set_end();
            }
            if trim_prev {
                segs[ii - 1] = Segment::Text(prev.as_ref().expect("checked").trim_trail_blank());
            }
            if trim_next {
                segs[ii + 1] = Segment::Text(next.as_ref().expect("checked").trim_lead_blank());
            }
            if let Segment::Parent(_) = &segs[ii] {
                if !indent.is_empty() {
                    if let Some(indented) = segs[ii].indent(&indent, first, last) {
                        segs[ii] = indented;
                    }
                }
            }
            continue;
        }
        match &mut segs[ii] {
            Segment::Include(include) => {
                if let Some(prev) = prev.as_ref().filter(|_| prev_blank && next_blank) {
                    let indent = prev.indent();
                    include.standalone = true;
                    if !indent.is_empty() {
                        if let Some(indented) = segs[ii].indent(&indent, first, last) {
                            segs[ii] = indented;
                        }
                    }
                    // The previous text keeps its blank: it is the partial's indentation.
                    if let Some(next) = &next {
                        segs[ii + 1] = Segment::Text(next.trim_lead_blank());
                    }
                }
            }
            Segment::Faux if prev_blank && next_blank => {
                if let Some(prev) = &prev {
                    segs[ii - 1] = Segment::Text(prev.trim_trail_blank());
                }
                if let Some(next) = &next {
                    segs[ii + 1] = Segment::Text(next.trim_lead_blank());
                }
            }
            _ => {}
        }
    }
    segs
}

/// `Mustache.indentSegs`: `None` when no segment changed.
pub(crate) fn indent_segs(segs: &[Segment], indent: &str, first_outer: bool, last_outer: bool) -> Option<Vec<Segment>> {
    if indent.is_empty() {
        return None;
    }
    let len = segs.len();
    let mut copy = Vec::with_capacity(len);
    let mut changed = false;
    for i in 0..len {
        let seg = &segs[i];
        let pseg = if i > 0 { Some(&segs[i - 1]) } else { None };
        let nseg = segs.get(i + 1);
        let indented = match seg {
            Segment::Section(s) => {
                let first = match pseg {
                    None => first_outer,
                    Some(_) => s.standalone_start,
                };
                let last = if s.standalone_end {
                    false
                } else {
                    match nseg {
                        None => last_outer,
                        Some(_) => true,
                    }
                };
                seg.indent(indent, first, last)
            }
            Segment::Text(_) => {
                let first = match pseg {
                    None => first_outer,
                    Some(p) => p.is_standalone(),
                };
                let last = match nseg {
                    None => last_outer,
                    Some(n) => match n.section_standalone_start() {
                        Some(start) => !start,
                        None => !n.is_standalone(),
                    },
                };
                seg.indent(indent, first, last)
            }
            Segment::Include(_) => {
                if seg.is_standalone() {
                    let last = match nseg {
                        None => last_outer,
                        Some(n) => !n.is_standalone(),
                    };
                    seg.indent(indent, false, last)
                } else {
                    None
                }
            }
            _ => seg.indent(indent, first_outer, last_outer),
        };
        match indented {
            Some(s) => {
                changed = true;
                copy.push(s);
            }
            None => copy.push(seg.clone()),
        }
    }
    changed.then_some(copy)
}

/// `Mustache.replaceBlockSegs`: `None` when no segment changed.
pub(crate) fn replace_block_segs(segs: &[Segment], blocks: &[(String, Section)]) -> Option<Vec<Segment>> {
    if blocks.is_empty() {
        return None;
    }
    let mut changed = false;
    let copy = segs
        .iter()
        .map(|s| match s.replace_blocks(blocks) {
            Some(r) => {
                changed = true;
                r
            }
            None => s.clone(),
        })
        .collect();
    changed.then_some(copy)
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Text,
    MatchingStart,
    MatchingEnd,
    Tag,
}

#[derive(Clone, Copy, PartialEq)]
enum Open {
    Section,
    Inverted,
    Block,
    Parent,
}

struct Frame {
    /// `None` for the top level.
    open: Option<(Open, String, usize)>,
    segs: Vec<Segment>,
}

pub(crate) struct Parser<'a> {
    compiler: &'a Compiler,
    delims: Delims,
    text: String,
    state: State,
    line: usize,
    stack: Vec<Frame>,
    source: Vec<char>,
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(compiler: &'a Compiler) -> Parser<'a> {
        Parser {
            compiler,
            delims: compiler.delims,
            text: String::new(),
            state: State::Text,
            line: 1,
            stack: vec![Frame { open: None, segs: Vec::new() }],
            source: Vec::new(),
            pos: 0,
        }
    }

    pub fn parse(mut self, source: &str) -> Result<Vec<Segment>, MustacheError> {
        self.source = source.chars().collect();
        while let Some(c) = self.next_char() {
            self.parse_char(c)?;
            if c == '\n' {
                self.line += 1;
            }
        }
        match self.state {
            State::Tag => {
                let mut text = std::mem::take(&mut self.text);
                self.delims.restore_start(&mut text);
                self.text = text;
            }
            State::MatchingEnd => {
                let mut text = std::mem::take(&mut self.text);
                self.delims.restore_start(&mut text);
                text.push(self.delims.end1);
                self.text = text;
            }
            State::MatchingStart => self.text.push(self.delims.start1),
            State::Text => {}
        }
        self.add_text_segment();
        let frame = self.stack.pop().expect("top frame");
        match frame.open {
            None => Ok(frame.segs),
            Some((kind, name, line)) => {
                let what = match kind {
                    Open::Section => "Section",
                    Open::Inverted => "Inverted section",
                    Open::Block => "Block",
                    Open::Parent => "Parent",
                };
                Err(MustacheError::parse(format!("{what} missing close tag '{name}'"), line))
            }
        }
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.source.get(self.pos).copied();
        self.pos += 1;
        c
    }

    fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("at least the top frame")
    }

    fn add_text_segment(&mut self) {
        if !self.text.is_empty() {
            let top_level = self.stack.len() == 1;
            let text = std::mem::take(&mut self.text);
            let frame = self.top();
            let first = frame.segs.is_empty() && top_level;
            frame.segs.push(Segment::Text(StringSegment::new(&text, first)));
        }
    }

    fn parse_char(&mut self, c: char) -> Result<(), MustacheError> {
        match self.state {
            State::Text => {
                if c == self.delims.start1 {
                    self.state = State::MatchingStart;
                    if self.delims.start2 == NO_CHAR {
                        self.parse_char(NO_CHAR)?;
                    }
                } else {
                    self.text.push(c);
                }
            }
            State::MatchingStart => {
                if c == self.delims.start2 {
                    self.add_text_segment();
                    self.state = State::Tag;
                } else {
                    self.text.push(self.delims.start1);
                    self.state = State::Text;
                    self.parse_char(c)?;
                }
            }
            State::Tag => {
                if c == self.delims.end1 {
                    self.state = State::MatchingEnd;
                    if self.delims.end2 == NO_CHAR {
                        self.parse_char(NO_CHAR)?;
                    }
                } else if c == self.delims.start1 && !self.text.is_empty() && !self.text.starts_with('!') {
                    // "{{foo {": the matched text was plain text; start a new tag here.
                    let mut text = std::mem::take(&mut self.text);
                    self.delims.restore_start(&mut text);
                    self.text = text;
                    self.add_text_segment();
                    if self.delims.start2 == NO_CHAR {
                        self.add_text_segment();
                        self.state = State::Tag;
                    } else {
                        self.state = State::MatchingStart;
                    }
                } else {
                    self.text.push(c);
                }
            }
            State::MatchingEnd => {
                if c == self.delims.end2 {
                    let first = self.text.chars().next().ok_or_else(|| {
                        MustacheError::Java(java_compat::lang::JError::new("java.lang.StringIndexOutOfBoundsException", "Index 0 out of bounds for length 0"))
                    })?;
                    if first == '=' {
                        let chars: Vec<char> = self.text.chars().collect();
                        let inner: String = if chars.len() >= 2 { chars[1..chars.len() - 1].iter().collect() } else { String::new() };
                        self.delims.update(&inner)?;
                        self.text.clear();
                        self.top().segs.push(Segment::Faux);
                    } else {
                        if self.delims.is_staches() && first == self.delims.start1 {
                            let end3 = self.next_char();
                            if end3 != Some('}') {
                                let got = end3.map(String::from).unwrap_or_default();
                                return Err(MustacheError::parse(format!("Invalid triple-mustache tag: {{{{{}}}}}{got}", self.text), self.line));
                            }
                            self.text.replace_range(..first.len_utf8(), "&");
                        }
                        let text = std::mem::take(&mut self.text);
                        self.add_tag_segment(&text)?;
                    }
                    self.state = State::Text;
                } else {
                    self.text.push(self.delims.end1);
                    self.state = State::Tag;
                    self.parse_char(c)?;
                }
            }
        }
        Ok(())
    }

    fn add_tag_segment(&mut self, accum: &str) -> Result<(), MustacheError> {
        let tag_line = self.line;
        let tag = java_trim(accum).to_string();
        let Some(first) = tag.chars().next() else {
            // `tag.substring(1)` on an empty tag, in the wording of MockServer 8.0.0's JDK.
            return Err(MustacheError::Java(java_compat::lang::JError::new("java.lang.StringIndexOutOfBoundsException", "Range [1, 0) out of bounds for length 0")));
        };
        let tag1 = java_trim(&tag[first.len_utf8()..]).to_string();
        let require_no_newlines = |tag: &str| {
            if tag.contains('\n') || tag.contains('\r') {
                Err(MustacheError::parse(format!("Invalid tag name: contains newline '{tag}'"), tag_line))
            } else {
                Ok(())
            }
        };
        match first {
            '#' | '<' | '$' | '^' => {
                require_no_newlines(&tag)?;
                let kind = match first {
                    '#' => Open::Section,
                    '<' => Open::Parent,
                    '$' => Open::Block,
                    _ => Open::Inverted,
                };
                self.stack.push(Frame { open: Some((kind, tag1, tag_line)), segs: Vec::new() });
            }
            '>' => {
                let compiler = self.compiler.clone();
                self.top().segs.push(Segment::Include(Include {
                    name: tag1,
                    line: tag_line,
                    indent: String::new(),
                    standalone: false,
                    compiler,
                    loaded: Arc::new(Mutex::new(None)),
                }));
            }
            '/' => {
                require_no_newlines(&tag)?;
                if self.stack.len() == 1 {
                    return Err(MustacheError::parse(format!("Section close tag with no open tag '{tag1}'"), tag_line));
                }
                let frame = self.stack.pop().expect("checked");
                let (kind, name, line) = frame.open.expect("not the top frame");
                if name != tag1 {
                    return Err(MustacheError::parse(format!("Section close tag with mismatched open tag '{tag1}' != '{name}'"), tag_line));
                }
                let seg = match kind {
                    Open::Section => Segment::Section(Section::new(SectionKind::Section, tag1, frame.segs, line)),
                    Open::Inverted => Segment::Section(Section::new(SectionKind::Inverted, tag1, frame.segs, line)),
                    Open::Block => Segment::Section(Section::new(SectionKind::Block, tag1, frame.segs, line)),
                    Open::Parent => {
                        // Only the blocks inside a parent tag count, trimmed as if at the top level.
                        let blocks = frame.segs.into_iter().filter(|s| matches!(s, Segment::Section(b) if b.kind == SectionKind::Block)).collect();
                        Segment::Parent(Parent {
                            name: tag1,
                            line,
                            indent: String::new(),
                            segs: trim(blocks, true),
                            standalone_start: false,
                            standalone_end: false,
                            blocks: Vec::new(),
                            compiler: self.compiler.clone(),
                            loaded: Arc::new(Mutex::new(None)),
                        })
                    }
                };
                self.top().segs.push(seg);
            }
            '!' => self.top().segs.push(Segment::Faux),
            '&' => {
                require_no_newlines(&tag)?;
                self.top().segs.push(Segment::Variable { name: tag1, line: tag_line, escape: false });
            }
            _ => {
                require_no_newlines(&tag)?;
                self.top().segs.push(Segment::Variable { name: tag, line: tag_line, escape: true });
            }
        }
        Ok(())
    }
}
