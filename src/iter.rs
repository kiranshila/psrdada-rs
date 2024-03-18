//! A trait that we will use that leverages [generic associated types](https://blog.rust-lang.org/2022/10/28/gats-stabilization.html)
//! to create a dada iterator that garuntees that references to a given buffer only exist when it is safe to do so.

pub trait DadaIterator {
    type Item<'next>
    where
        Self: 'next;

    /// Gets the next block a la [`Iterator::next`].
    fn next(&mut self) -> Option<Self::Item<'_>>;
}
