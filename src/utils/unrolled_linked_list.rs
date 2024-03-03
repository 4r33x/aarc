use crate::utils::helpers::{alloc_box_ptr, dealloc_box_ptr};
use std::array;
use std::ptr::null_mut;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicPtr, Ordering};

/// A specialized linked list; each node contains an array of N items.
#[derive(Default)]
pub(crate) struct UnrolledLinkedList<T: Default, const N: usize> {
    head: ULLNode<T, N>,
}

impl<T: Default, const N: usize> UnrolledLinkedList<T, N> {
    pub(crate) fn iter(&self, order: Ordering) -> impl Iterator<Item = &'_ T> {
        self.head.iter(order)
    }
    pub(crate) fn try_for_each_with_append<R, F: Fn(&T) -> Option<R>>(&self, f: F) -> R {
        let mut curr = &self.head;
        loop {
            for item in curr.items.iter() {
                if let Some(result) = f(item) {
                    return result;
                }
            }
            let mut next = curr.next.load(SeqCst);
            if next.is_null() {
                let new_node = alloc_box_ptr(ULLNode::default());
                match curr
                    .next
                    .compare_exchange(null_mut(), new_node, SeqCst, SeqCst)
                {
                    Ok(_) => next = new_node,
                    Err(actual) => unsafe {
                        dealloc_box_ptr(new_node);
                        next = actual;
                    },
                }
            }
            unsafe {
                curr = &*next;
            }
        }
    }
}

struct ULLNode<T, const N: usize> {
    items: [T; N],
    next: AtomicPtr<ULLNode<T, N>>,
}

impl<T, const N: usize> ULLNode<T, N> {
    fn iter(&self, order: Ordering) -> impl Iterator<Item = &'_ T> {
        let mut iters = vec![self.items.iter()];
        let mut curr = self.next.load(order);
        while !curr.is_null() {
            unsafe {
                iters.push((*curr).items.iter());
                curr = (*curr).next.load(order);
            }
        }
        iters.into_iter().flatten()
    }
}

impl<T: Default, const N: usize> Default for ULLNode<T, N> {
    fn default() -> Self {
        Self {
            items: array::from_fn(|_| T::default()),
            next: AtomicPtr::default(),
        }
    }
}

impl<T, const N: usize> Drop for ULLNode<T, N> {
    fn drop(&mut self) {
        let next = self.next.load(SeqCst);
        if !next.is_null() {
            unsafe {
                dealloc_box_ptr(next);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::unrolled_linked_list::UnrolledLinkedList;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering::SeqCst;
    use std::thread;

    #[test]
    fn test_concurrent_iter_and_append() {
        const N: usize = 2;
        const THREADS: usize = N * 2 + 1;

        let ull: UnrolledLinkedList<AtomicBool, N> = UnrolledLinkedList::default();
        thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    ull.try_for_each_with_append(|b| {
                        match b.compare_exchange(false, true, SeqCst, SeqCst) {
                            Ok(_) => Some(true),
                            Err(_) => None,
                        }
                    });
                });
            }
        });
        for (i, b) in ull.iter(SeqCst).enumerate() {
            assert_eq!(b.load(SeqCst), i < THREADS);
        }
    }
}
