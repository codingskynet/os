pub trait Pointer: Clone + Copy + PartialEq + Eq {
    /// Returns a pointer to the [`Node`] links embedded in the pointed-to
    /// element.
    fn node(&mut self) -> &mut Node<Self>;

    fn pop(&mut self) {
        let node = self.node();
        if let Some(mut prev) = node.prev {
            let prev = prev.node();
            prev.next = node.next;
        }
        if let Some(mut next) = node.next {
            let next = next.node();
            next.prev = node.prev;
        }

        node.prev = None;
        node.next = None;
    }

    fn push_front(self, mut value: Self) {
        debug_assert!(self != value);

        let mut s = self;
        let node = s.node();

        let head = value.node();
        debug_assert!(!head.is_linked());

        head.next = Some(self);
        node.prev = Some(value);
    }
}

pub struct Node<P: Pointer> {
    prev: Option<P>,
    next: Option<P>,
}

impl<P: Pointer> Default for Node<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Pointer> Node<P> {
    pub const fn new() -> Self {
        Self {
            prev: None,
            next: None,
        }
    }

    pub fn is_linked(&self) -> bool {
        self.prev.is_some() || self.next.is_some()
    }

    pub fn next(&self) -> Option<P> {
        self.next
    }
}

#[cfg(test)]
mod tests {
    use core::ptr::NonNull;
    use std::boxed::Box;

    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Ptr(NonNull<Entry>);

    struct Entry {
        node: Node<Ptr>,
    }

    impl Pointer for Ptr {
        fn node(&mut self) -> &mut Node<Self> {
            unsafe { &mut self.0.as_mut().node }
        }
    }

    impl Ptr {
        fn alloc() -> Self {
            Ptr(NonNull::new(Box::into_raw(Box::new(Entry { node: Node::new() }))).unwrap())
        }

        fn is_linked(self) -> bool {
            unsafe { self.0.as_ref().node.is_linked() }
        }

        fn prev(self) -> Option<Ptr> {
            unsafe { self.0.as_ref().node.prev }
        }

        fn next(self) -> Option<Ptr> {
            unsafe { self.0.as_ref().node.next }
        }
    }

    fn push_front(head: &mut Option<Ptr>, value: Ptr) {
        if let Some(current) = *head {
            current.push_front(value);
        }
        *head = Some(value);
    }

    fn pop_head(head: &mut Option<Ptr>) -> Option<Ptr> {
        let mut current = head.take()?;
        *head = current.node().next();
        current.pop();
        Some(current)
    }

    #[test]
    fn push_front_single_element_becomes_head() {
        let mut head = None;
        assert_eq!(head, None);

        let a = Ptr::alloc();
        push_front(&mut head, a);

        assert_eq!(head, Some(a));
        assert!(!a.is_linked());
        assert_eq!(a.prev(), None);
        assert_eq!(a.next(), None);
    }

    #[test]
    fn pop_only_element_empties_head() {
        let mut head = None;
        let a = Ptr::alloc();
        push_front(&mut head, a);

        assert_eq!(pop_head(&mut head), Some(a));

        assert_eq!(head, None);
        // links are cleared, so the element can be re-inserted safely.
        assert!(!a.is_linked());
        assert_eq!(a.prev(), None);
        assert_eq!(a.next(), None);
    }

    #[test]
    fn pop_head_promotes_next() {
        let mut head = None;
        let a = Ptr::alloc();
        let b = Ptr::alloc();
        push_front(&mut head, a);
        push_front(&mut head, b); // list: b -> a, head is b

        assert_eq!(pop_head(&mut head), Some(b));

        assert_eq!(head, Some(a));
        assert!(!a.is_linked());
        assert_eq!(a.prev(), None);
        assert_eq!(a.next(), None);
        assert!(!b.is_linked());
        assert_eq!(b.prev(), None);
        assert_eq!(b.next(), None);
    }

    #[test]
    fn pop_middle_relinks_neighbors() {
        let mut head = None;
        let a = Ptr::alloc();
        let mut b = Ptr::alloc();
        let c = Ptr::alloc();
        push_front(&mut head, a);
        push_front(&mut head, b);
        push_front(&mut head, c); // list: c -> b -> a, head is c

        b.pop();

        assert_eq!(head, Some(c));
        assert_eq!(c.next(), Some(a));
        assert_eq!(a.prev(), Some(c));
        assert!(!b.is_linked());
        assert_eq!(b.prev(), None);
        assert_eq!(b.next(), None);
    }
}
