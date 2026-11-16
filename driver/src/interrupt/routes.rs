//! Device-independent IRQ registrations, grouped by interrupt controller.
extern crate alloc;
use alloc::vec::Vec;

struct Source {
    irq: u32,
    handler: fn() -> bool,
    enable: Option<fn()>,
}

pub struct Routes {
    pub parent: u32,
    sources: Vec<Source>,
}
impl Routes {
    pub fn irqs(&self) -> impl Iterator<Item = u32> + '_ {
        self.sources.iter().map(|source| source.irq)
    }
    pub fn enable_devices(&self) {
        for source in &self.sources {
            if let Some(enable) = source.enable {
                enable();
            }
        }
    }
    pub fn dispatch(&self, irq: u32) -> Option<bool> {
        let source = self.sources.iter().find(|source| source.irq == irq)?;
        Some((source.handler)())
    }
}

/// Boot-time registrations are frozen before controllers are published.
pub struct Registry {
    routes: Option<Vec<Routes>>,
}
impl Registry {
    pub const fn new() -> Self {
        Self {
            routes: Some(Vec::new()),
        }
    }
    pub fn register(
        &mut self,
        parent: u32,
        irq: u32,
        handler: fn() -> bool,
        enable: Option<fn()>,
    ) -> Result<(), &'static str> {
        let routes = self.routes.as_mut().ok_or("IRQ registration is closed")?;
        let index = match routes.iter().position(|routes| routes.parent == parent) {
            Some(index) => index,
            None => {
                routes.push(Routes {
                    parent,
                    sources: Vec::new(),
                });
                routes.len() - 1
            }
        };
        let routes = &mut routes[index];
        if routes.irqs().any(|source| source == irq) {
            return Err("IRQ already registered; shared IRQ lines are not supported");
        }
        routes.sources.push(Source {
            irq,
            handler,
            enable,
        });
        Ok(())
    }
    pub fn finish(&mut self) -> Result<Vec<Routes>, &'static str> {
        self.routes.take().ok_or("IRQ registration is closed")
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_independent_devices_by_parent_and_irq() {
        let mut registry = Registry::new();
        registry.register(1, 3, || false, None).unwrap();
        registry.register(2, 3, || true, None).unwrap();
        registry.register(1, 4, || true, None).unwrap();
        let routes = registry.finish().unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].dispatch(3), Some(false));
        assert_eq!(routes[0].dispatch(4), Some(true));
        assert_eq!(routes[1].dispatch(3), Some(true));
        assert_eq!(routes[1].dispatch(4), None);
    }

    #[test]
    fn rejects_duplicate_irq_without_replacing_handler() {
        let mut registry = Registry::new();
        registry.register(1, 3, || false, None).unwrap();
        assert!(registry.register(1, 3, || true, None).is_err());
        assert_eq!(registry.finish().unwrap()[0].dispatch(3), Some(false));
    }

    #[test]
    fn freezes_registrations_before_interrupt_dispatch() {
        let mut registry = Registry::new();
        registry.finish().unwrap();
        assert!(registry.register(1, 3, || false, None).is_err());
        assert!(registry.finish().is_err());
    }

    #[test]
    fn enables_devices_only_when_requested() {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        let mut registry = Registry::new();
        registry
            .register(
                1,
                3,
                || false,
                Some(|| {
                    CALLS.fetch_add(1, Ordering::Relaxed);
                }),
            )
            .unwrap();
        registry.register(1, 4, || true, None).unwrap();
        let routes = registry.finish().unwrap();
        assert_eq!(CALLS.load(Ordering::Relaxed), 0);
        routes[0].enable_devices();
        assert_eq!(CALLS.load(Ordering::Relaxed), 1);
    }
}
