//! `ThreadSafetyTest`: a template shared between threads loads each partial once.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use java_compat::lang::{JError, JMap, JValue, MapKind};
use jmustache::{Compiler, TemplateLoader};

const TEMPLATE_A: &str = "Template A content\n{{> templateB}}";
const TEMPLATE_B: &str = "Template B content";

#[derive(Default)]
struct CountingLoader {
    counter: AtomicUsize,
}

impl TemplateLoader for CountingLoader {
    fn get_template(&self, name: &str) -> Result<String, JError> {
        self.counter.fetch_add(1, Ordering::SeqCst);
        if name != "templateB" {
            return Err(JError::without_message("java.lang.IllegalArgumentException"));
        }
        Ok(TEMPLATE_B.into())
    }
}

fn template_loading_is_thread_safe(n_threads: usize) {
    let barrier = Arc::new(Barrier::new(n_threads));
    let loader = Arc::new(CountingLoader::default());
    let compiler = Compiler::new().with_loader(loader.clone());
    let template = Arc::new(compiler.compile(TEMPLATE_A).unwrap());
    let threads: Vec<_> = (0..n_threads)
        .map(|_| {
            let (barrier, template) = (barrier.clone(), template.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let value = template.execute(JValue::Map(JMap::new(MapKind::Fixed))).unwrap();
                assert!(value.contains("Template A content\nTemplate B content"), "Invalid template result {value}");
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(loader.counter.load(Ordering::SeqCst), 1);
}

#[test]
fn template_loading() {
    for n in [1, 2, 4, 8, 16, 32, 256, 1024] {
        template_loading_is_thread_safe(n);
    }
}

#[test]
fn partial_thread_safe() {
    let load_count = Arc::new(AtomicUsize::new(0));
    let counter = load_count.clone();
    let loader = move |name: &str| -> Result<String, JError> {
        if name == "partial" {
            counter.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(20));
            return Ok("Hello".into());
        }
        Err(JError::new("java.io.IOException", name))
    };
    let template = Arc::new(Compiler::new().with_loader(Arc::new(loader)).compile("{{stuff}}\n\t{{> partial }}").unwrap());
    let m = JValue::Map(JMap::from_entries(MapKind::Hash, [(JValue::str("stuff"), JValue::str("Foo"))]));
    let errors = Arc::new(Mutex::new(Vec::new()));
    // A pool of 64 workers runs 100 renders.
    let jobs = Arc::new(Mutex::new((1..=100).rev().collect::<Vec<u64>>()));
    let workers: Vec<_> = (0..64)
        .map(|_| {
            let (jobs, template, m, errors) = (jobs.clone(), template.clone(), m.clone(), errors.clone());
            std::thread::spawn(move || {
                loop {
                    let Some(i) = jobs.lock().unwrap().pop() else { break };
                    std::thread::sleep(Duration::from_millis(i % 10));
                    if let Err(e) = template.execute(m.clone()) {
                        errors.lock().unwrap().push(e);
                    }
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    assert!(errors.lock().unwrap().is_empty(), "{:?}", errors.lock().unwrap());
    assert_eq!(load_count.load(Ordering::SeqCst), 1);
}
