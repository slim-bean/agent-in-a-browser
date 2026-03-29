//! Either type matching tokio_util::either.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> Future for Either<L, R>
where
    L: Future + Unpin,
    R: Future<Output = L::Output> + Unpin,
{
    type Output = L::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.get_mut() {
            Either::Left(l) => Pin::new(l).poll(cx),
            Either::Right(r) => Pin::new(r).poll(cx),
        }
    }
}
