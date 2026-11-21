//! Fila estéreo sem bloqueio, um produtor e um consumidor (SPSC).

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Inner {
    buf: UnsafeCell<Vec<(f32, f32)>>,
    cap: usize,
    head: AtomicUsize, // próxima escrita (só o produtor muda)
    tail: AtomicUsize, // próxima leitura (só o consumidor muda)
}

// Um escritor e um leitor, cada um só toca no próprio índice: seguro entre threads.
unsafe impl Sync for Inner {}
unsafe impl Send for Inner {}

pub struct Producer(Arc<Inner>);
pub struct Consumer(Arc<Inner>);

/// Fila com capacidade para `frames` quadros estéreo.
pub fn channel(frames: usize) -> (Producer, Consumer) {
    let cap = frames.max(2) + 1;
    let inner = Arc::new(Inner { buf: UnsafeCell::new(vec![(0.0, 0.0); cap]), cap, head: AtomicUsize::new(0), tail: AtomicUsize::new(0) });
    (Producer(inner.clone()), Consumer(inner))
}

impl Inner {
    fn len(&self) -> usize {
        let (h, t) = (self.head.load(Ordering::Acquire), self.tail.load(Ordering::Acquire));
        (h + self.cap - t) % self.cap
    }
}

impl Producer {
    /// Fila cheia = descarta o quadro (o lado de saída está parado ou muito atrás).
    pub fn push(&self, l: f32, r: f32) -> bool {
        let i = &self.0;
        let h = i.head.load(Ordering::Relaxed);
        let next = (h + 1) % i.cap;
        if next == i.tail.load(Ordering::Acquire) {
            return false;
        }
        unsafe { (&mut *i.buf.get())[h] = (l, r) };
        i.head.store(next, Ordering::Release);
        true
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Consumer {
    pub fn pop(&self) -> Option<(f32, f32)> {
        let i = &self.0;
        let t = i.tail.load(Ordering::Relaxed);
        if t == i.head.load(Ordering::Acquire) {
            return None;
        }
        let v = unsafe { (&*i.buf.get())[t] };
        i.tail.store((t + 1) % i.cap, Ordering::Release);
        Some(v)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_and_full() {
        let (p, c) = channel(3);
        assert!(p.push(1.0, 1.0) && p.push(2.0, 2.0) && p.push(3.0, 3.0));
        assert!(!p.push(4.0, 4.0), "cheia descarta");
        assert_eq!(c.pop(), Some((1.0, 1.0)));
        assert_eq!(c.len(), 2);
        assert!(p.push(5.0, 5.0));
        assert_eq!((c.pop(), c.pop(), c.pop(), c.pop()), (Some((2.0, 2.0)), Some((3.0, 3.0)), Some((5.0, 5.0)), None));
    }

    #[test]
    fn across_threads() {
        let (p, c) = channel(1024);
        let t = std::thread::spawn(move || {
            for k in 0..100_000 {
                while !p.push(k as f32, -(k as f32)) {
                    std::thread::yield_now();
                }
            }
        });
        let mut next = 0;
        while next < 100_000 {
            if let Some((l, r)) = c.pop() {
                assert_eq!((l, r), (next as f32, -(next as f32)));
                next += 1;
            }
        }
        t.join().unwrap();
    }
}
