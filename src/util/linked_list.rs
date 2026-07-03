pub trait Pointer: Clone + Copy + PartialEq + Eq {
    /// Returns a pointer to the [`Node`] links embedded in the pointed-to
    /// element.
    ///
    /// # Safety
    ///
    /// - The pointer must reference a live element that stays alive and at a
    ///   stable address for as long as it is linked into a [`LinkedList`].
    /// - The returned pointer must be non-null, properly aligned, and always
    ///   refer to the *same* `Node` for a given element.
    /// - The caller must not create aliasing `&mut Node` references through the
    ///   returned pointer; `LinkedList` relies on being the sole mutator of the
    ///   links while an element is linked.
    unsafe fn node(&self) -> *mut Node<Self>;
}

pub struct Node<T> {
    prev: Option<T>,
    next: Option<T>,
}

impl<T> Default for Node<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Node<T> {
    pub const fn new() -> Self {
        Self {
            prev: None,
            next: None,
        }
    }
}

pub struct LinkedList<P: Pointer> {
    head: Option<P>,
}

impl<P: Pointer> Default for LinkedList<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Pointer> LinkedList<P> {
    pub const fn new() -> Self {
        Self { head: None }
    }

    pub fn head(&self) -> Option<P> {
        self.head
    }

    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Inserts `value` at the front of the list.
    ///
    /// # Safety
    ///
    /// - `value` must not already be linked into this or any other list. Its
    ///   `Node` links must both be `None`; re-inserting a still-linked element
    ///   corrupts the list (in release builds this is not checked and can form
    ///   a self-referential cycle).
    /// - `value` and the current head must satisfy the [`Pointer::node`]
    ///   contract: each references a live element whose `Node` stays valid and
    ///   at a stable address while linked.
    /// - No other references to the `Node`s of `value` or the current head may
    ///   be alive for the duration of this call.
    pub unsafe fn push_front(&mut self, value: P) {
        let head = self.head;

        {
            let node = self.node(value);
            debug_assert!(node.prev.is_none());
            debug_assert!(node.next.is_none());

            node.prev = None;
            node.next = head;
        }

        if let Some(head) = head {
            let head = self.node(head);
            debug_assert!(head.prev.is_none());
            head.prev = Some(value);
        }

        self.head = Some(value);
    }

    /// Unlinks `value` from the list.
    ///
    /// # Safety
    ///
    /// - `value` must currently be linked into *this* list. Removing an element
    ///   that is not in this list is undefined: in release builds the head
    ///   check is skipped, so removing an unlinked element silently drops the
    ///   list's head, and removing an element of another list corrupts both.
    /// - `value`, along with its neighbours, must satisfy the
    ///   [`Pointer::node`] contract: each references a live element whose `Node`
    ///   stays valid and at a stable address while linked.
    /// - No other references to the `Node`s of `value` or its neighbours may be
    ///   alive for the duration of this call.
    pub unsafe fn remove(&mut self, value: P) {
        let (prev, next) = {
            let node = self.node(value);
            (node.prev, node.next)
        };

        if let Some(prev) = prev {
            let prev = self.node(prev);
            debug_assert!(prev.next == Some(value));
            prev.next = next;
        } else {
            debug_assert!(self.head == Some(value));
            self.head = next;
        }

        if let Some(next) = next {
            let next = self.node(next);
            debug_assert!(next.prev == Some(value));
            next.prev = prev;
        }

        let node = self.node(value);
        node.prev = None;
        node.next = None;
    }

    #[allow(clippy::mut_from_ref)]
    fn node(&self, value: P) -> &mut Node<P> {
        unsafe { &mut *value.node() }
    }
}

#[cfg(test)]
mod tests {
    use core::ptr::NonNull;
    use std::boxed::Box;

    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Ptr(NonNull<Node<Ptr>>);

    impl Pointer for Ptr {
        unsafe fn node(&self) -> *mut Node<Self> {
            self.0.as_ptr()
        }
    }

    impl Ptr {
        fn alloc() -> Self {
            Ptr(NonNull::new(Box::into_raw(Box::new(Node::new()))).unwrap())
        }

        fn prev(self) -> Option<Ptr> {
            unsafe { self.0.as_ref().prev }
        }

        fn next(self) -> Option<Ptr> {
            unsafe { self.0.as_ref().next }
        }
    }

    #[test]
    fn push_front_single_element_becomes_head() {
        let mut list = LinkedList::new();
        assert!(list.is_empty());
        assert_eq!(list.head(), None);

        let a = Ptr::alloc();
        unsafe { list.push_front(a) };

        assert!(!list.is_empty());
        assert_eq!(list.head(), Some(a));
        assert_eq!(a.prev(), None);
        assert_eq!(a.next(), None);
    }

    #[test]
    fn remove_only_element_empties_list() {
        let mut list = LinkedList::new();
        let a = Ptr::alloc();
        unsafe { list.push_front(a) };

        unsafe { list.remove(a) };

        assert!(list.is_empty());
        assert_eq!(list.head(), None);
        // links are cleared, so the element can be re-inserted safely.
        assert_eq!(a.prev(), None);
        assert_eq!(a.next(), None);
    }

    #[test]
    fn remove_head_promotes_next() {
        let mut list = LinkedList::new();
        let a = Ptr::alloc();
        let b = Ptr::alloc();
        unsafe { list.push_front(a) };
        unsafe { list.push_front(b) }; // list: b -> a, head is b

        unsafe { list.remove(b) };

        assert_eq!(list.head(), Some(a));
        assert_eq!(a.prev(), None);
        assert_eq!(a.next(), None);
        assert_eq!(b.prev(), None);
        assert_eq!(b.next(), None);
    }

    #[test]
    fn remove_middle_relinks_neighbors() {
        let mut list = LinkedList::new();
        let a = Ptr::alloc();
        let b = Ptr::alloc();
        let c = Ptr::alloc();
        unsafe { list.push_front(a) };
        unsafe { list.push_front(b) };
        unsafe { list.push_front(c) }; // list: c -> b -> a, head is c

        unsafe { list.remove(b) };

        assert_eq!(list.head(), Some(c));
        assert_eq!(c.next(), Some(a));
        assert_eq!(a.prev(), Some(c));
        assert_eq!(b.prev(), None);
        assert_eq!(b.next(), None);
    }
}
