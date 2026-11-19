//! `Mustache.Collector`, `BasicCollector` and `DefaultCollector` (and MockServer's `ExtendedCollector`): how a
//! value is iterated and how a name is read from a context object.

use java_compat::lang::{JError, JObject, JValue, ListKind, Members, Reflection, Returns};
use java_compat::methods;

use crate::CustomContext;

/// The result of looking a name up in one context object.
#[derive(Debug, Clone)]
pub enum Fetched {
    /// The name resolved to this value (null included).
    Value(JValue),
    /// `Template.NO_FETCHER_FOUND`.
    NotFound,
}

/// `Mustache.Collector`.
pub trait Collector: Send + Sync {
    /// `toIterator(value)`: the elements when the value is a collection, `None` otherwise.
    fn to_iterator(&self, value: &JValue) -> Option<Vec<JValue>>;

    /// `createFetcher(ctx, name).get(ctx, name)`; `Err` when the fetcher throws.
    fn fetch(&self, ctx: &JValue, name: &str) -> Result<Fetched, JError>;
}

/// `DefaultCollector`: `Iterable`s, iterators and arrays are collections; names resolve against custom
/// contexts, maps, list/iterator/array indexes, then by reflection.
#[derive(Debug, Clone, Copy)]
pub struct DefaultCollector {
    /// `allowAccessCoercion`: non-public members are reachable (`setAccessible(true)`).
    pub allow_access_coercion: bool,
    /// MockServer's `ExtendedCollector`: maps also answer `values` and `keySet`.
    pub extended_maps: bool,
}

impl Default for DefaultCollector {
    fn default() -> DefaultCollector {
        DefaultCollector { allow_access_coercion: true, extended_maps: false }
    }
}

impl DefaultCollector {
    /// `new DefaultCollector(allowAccessCoercion)`.
    pub fn new(allow_access_coercion: bool) -> DefaultCollector {
        DefaultCollector { allow_access_coercion, extended_maps: false }
    }

    /// MockServer's `ExtendedCollector`.
    pub fn extended() -> DefaultCollector {
        DefaultCollector { allow_access_coercion: true, extended_maps: true }
    }

    /// `BasicCollector.toIterator`.
    pub fn basic_iterator(value: &JValue) -> Option<Vec<JValue>> {
        match value {
            JValue::Map(_) => None,
            other => other.iterate(),
        }
    }
}

impl Collector for DefaultCollector {
    fn to_iterator(&self, value: &JValue) -> Option<Vec<JValue>> {
        DefaultCollector::basic_iterator(value)
    }

    fn fetch(&self, ctx: &JValue, name: &str) -> Result<Fetched, JError> {
        // `BasicCollector.createFetcher`.
        if let JValue::Object(o) = ctx {
            if let Some(custom) = o.as_any().downcast_ref::<CustomContext>() {
                let v = (custom.0)(name)?;
                return Ok(if v.is_null() { Fetched::NotFound } else { Fetched::Value(v) });
            }
        }
        if let JValue::Map(map) = ctx {
            if map.contains_key(&JValue::str(name)) {
                return Ok(Fetched::Value(map.get_str(name).unwrap_or(JValue::Null)));
            }
            let view = match name {
                "entrySet" => Some("entrySet"),
                "values" | "keySet" if self.extended_maps => Some(name),
                _ => None,
            };
            return match view {
                Some(method) => invoke(ctx, method).map(|v| Fetched::Value(v.unwrap_or(JValue::Null))),
                None => Ok(Fetched::NotFound),
            };
        }
        if name.starts_with(|c: char| c.is_ascii_digit()) {
            let index = || name.parse::<i32>().ok().and_then(|i| usize::try_from(i).ok());
            match ctx {
                // `LIST_FETCHER` for lists, `ITER_FETCHER` for an iterator (which skips n elements).
                JValue::List(l) if l.kind().is_array_list() || matches!(l.kind(), ListKind::Fixed | ListKind::Iterator) => {
                    return Ok(match index().and_then(|i| l.get(i)) {
                        Some(v) => Fetched::Value(v),
                        None => Fetched::NotFound,
                    });
                }
                JValue::Array(a) => {
                    return Ok(match index().and_then(|i| a.get(i)) {
                        Some(v) => Fetched::Value(v.clone()),
                        None => Fetched::NotFound,
                    });
                }
                _ => {}
            }
        }
        if let JValue::Object(o) = ctx {
            if let Some(r) = o.reflection() {
                return self.reflect(o.as_ref(), &r, name);
            }
        }
        builtin(ctx, name)
    }
}

fn upper_first(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

impl DefaultCollector {
    /// `getMethodOn(clazz, name)`: `name()`, `getName()` (non-void), `isName()` (boolean), declared on this level.
    fn method_on<'m>(&self, members: &'m Members, name: &str) -> Option<&'m str> {
        let upper = upper_first(name);
        let find = |n: &str| members.methods.iter().find(|m| m.name == n);
        // `makeAccessible`: a method is not initially accessible to reflection; only coercion makes it so.
        let pick = |m: &'m java_compat::lang::MethodDecl| self.allow_access_coercion.then_some(m.name.as_str());
        if let Some(m) = find(name).filter(|m| m.returns != Returns::Void) {
            return pick(m);
        }
        if let Some(m) = find(&format!("get{upper}")).filter(|m| m.returns != Returns::Void) {
            return pick(m);
        }
        if let Some(m) = find(&format!("is{upper}")).filter(|m| m.returns == Returns::Boolean) {
            return pick(m);
        }
        None
    }

    /// `DefaultCollector.createFetcher` for an object that describes its class.
    fn reflect(&self, o: &dyn JObject, r: &Reflection, name: &str) -> Result<Fetched, JError> {
        let call = |method: &str| -> Result<Fetched, JError> {
            match o.invoke(method, &[]) {
                Some(Ok(v)) => Ok(Fetched::Value(v.unwrap_or(JValue::Null))),
                Some(Err(e)) => Err(e),
                None => Ok(Fetched::NotFound),
            }
        };
        // `getMethod`.
        if self.allow_access_coercion {
            for level in &r.classes {
                if let Some(m) = self.method_on(level, name) {
                    return call(m);
                }
            }
        } else if r.classes.iter().chain(&r.interfaces).any(|l| l.methods.iter().any(|m| m.name == name && m.public)) {
            // `clazz.getMethod(name)`: a public method of exactly this name, whatever it returns.
            return call(name);
        }
        // `getField`.
        let field = r.classes.iter().flat_map(|l| &l.fields).find(|f| f.name == name && (self.allow_access_coercion || f.public));
        if field.is_some() {
            return Ok(Fetched::Value(o.field(name).unwrap_or(JValue::Null)));
        }
        // `getIfaceMethod`: default methods.
        for iface in &r.interfaces {
            if let Some(m) = self.method_on(iface, name) {
                return call(m);
            }
        }
        Ok(Fetched::NotFound)
    }
}

/// Zero-argument `void` methods of the built-in classes.
const VOID_METHODS: &[&str] = &["clear", "trimToSize", "notify", "notifyAll", "wait"];

/// Reflection on the JDK types [`JValue`] models (and objects that only answer `invoke`): `name()`, then
/// `getName()`, then `isName()` returning a boolean, then a field.
fn builtin(ctx: &JValue, name: &str) -> Result<Fetched, JError> {
    let upper = upper_first(name);
    for candidate in [name.to_string(), format!("get{upper}"), format!("is{upper}")] {
        // `void` methods are skipped without being called.
        if !matches!(ctx, JValue::Object(_)) && VOID_METHODS.contains(&candidate.as_str()) {
            continue;
        }
        if matches!(candidate.as_str(), "getClass" | "hashCode" | "toString" | "equals") && !declares(ctx, &candidate) {
            continue;
        }
        match methods::invoke(ctx, &candidate, &[]) {
            Some(Ok(Some(v))) => {
                if candidate.starts_with("is") && candidate != name && !matches!(v, JValue::Bool(_)) {
                    continue;
                }
                return Ok(Fetched::Value(v));
            }
            Some(Ok(None)) | None => continue,
            Some(Err(e)) => return Err(e),
        }
    }
    if let JValue::Object(o) = ctx {
        if let Some(v) = o.field(name) {
            return Ok(Fetched::Value(v));
        }
    }
    // An array's `length` is not a field to reflection (`Class.getField("length")` fails), so it is not found.
    Ok(Fetched::NotFound)
}

fn invoke(ctx: &JValue, method: &str) -> Result<Option<JValue>, JError> {
    match methods::invoke(ctx, method, &[]) {
        Some(r) => r,
        None => Ok(None),
    }
}

/// Whether the class itself (not `Object`) declares `toString` / `hashCode` / `equals`: JMustache stops its
/// method search before `java.lang.Object`.
fn declares(ctx: &JValue, method: &str) -> bool {
    match ctx {
        // String, the boxed types, collections and maps all override these.
        JValue::Object(o) => o.invoke(method, &[]).is_some(),
        JValue::Array(_) | JValue::Null => false,
        _ => method != "getClass",
    }
}
