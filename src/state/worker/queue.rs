//! The bounded queue the background lanes are built from: one side fills it, the other
//! empties it, so pending work and pending results cannot grow without end. Shutdown
//! happens under the queue's own mutex and discards queued jobs, so no producer can
//! slip a request in after `close` decided the queue is finished.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

/// Why a request did not go in.
#[derive(Debug)]
pub enum Rejected {
    /// The lane's queue is full; the request was dropped. Retry it or give up.
    Busy,
    /// The worker is shutting down; no result will arrive.
    Disconnected,
}

/// A bounded queue: pending work and pending results cannot grow without end.
pub struct Lane<T> {
    items: Mutex<VecDeque<T>>,
    ready: Condvar,
    open: AtomicBool,
    capacity: usize,
}

impl<T> Lane<T> {
    pub fn new(capacity: usize) -> Lane<T> {
        Lane {
            items: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            open: AtomicBool::new(true),
            capacity,
        }
    }

    /// Puts `item` in without waiting; full means `Busy`, shut down means `Disconnected`
    /// — in both cases the item is dropped, never kept for later.
    pub fn push(&self, item: T) -> Result<(), Rejected> {
        let items = self.items.lock().unwrap();
        if !self.open.load(Ordering::Relaxed) {
            return Err(Rejected::Disconnected);
        }
        if items.len() >= self.capacity {
            return Err(Rejected::Busy);
        }
        self.push_locked(items, item);
        Ok(())
    }

    /// `push` that first drops queued items `replaced` accepts, so a newer request takes
    /// the place of a queued one instead of queueing behind it: last request wins.
    pub fn push_replacing(&self, item: T, replaced: impl Fn(&T) -> bool) -> Result<(), Rejected> {
        let mut items = self.items.lock().unwrap();
        if !self.open.load(Ordering::Relaxed) {
            return Err(Rejected::Disconnected);
        }
        if !items.iter().any(&replaced) && items.len() >= self.capacity {
            return Err(Rejected::Busy);
        }
        items.retain(|queued| !replaced(queued));
        self.push_locked(items, item);
        Ok(())
    }

    fn push_locked(&self, items: std::sync::MutexGuard<VecDeque<T>>, item: T) {
        let mut items = items;
        items.push_back(item);
        drop(items);
        self.ready.notify_one();
    }

    /// Hands `item` to the reader, waiting while the lane is full, so a result is never
    /// dropped and never piles up without bound; `Err` once shut down: the reader is gone.
    pub fn deliver(&self, item: T) -> Result<(), ()> {
        let mut items = self.items.lock().unwrap();
        loop {
            if !self.open.load(Ordering::Relaxed) {
                return Err(());
            }
            if items.len() < self.capacity {
                self.push_locked(items, item);
                return Ok(());
            }
            items = self.ready.wait(items).unwrap();
        }
    }

    /// Takes the next item, waiting for one; `None` once the lane is shut down and empty.
    pub fn pop(&self) -> Option<T> {
        let mut items = self.items.lock().unwrap();
        loop {
            if let Some(item) = items.pop_front() {
                drop(items);
                self.ready.notify_all();
                return Some(item);
            }
            if !self.open.load(Ordering::Relaxed) {
                return None;
            }
            items = self.ready.wait(items).unwrap();
        }
    }

    /// The next item without waiting, freeing room for a blocked `deliver`.
    pub fn try_pop(&self) -> Option<T> {
        let item = self.items.lock().unwrap().pop_front();
        if item.is_some() {
            self.ready.notify_all();
        }
        item
    }

    /// Stops the lane under its own mutex — no push can slip in between the flag and the
    /// wake-up — and discards the queued jobs, so `pop` ends immediately. Waiting
    /// producers and consumers wake and leave.
    pub fn close(&self) {
        let mut items = self.items.lock().unwrap();
        self.open.store(false, Ordering::Relaxed);
        items.clear();
        drop(items);
        self.ready.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    struct Dot(u32);

    #[test]
    fn a_full_lane_is_busy_and_a_closed_lane_is_disconnected() {
        let lane: Lane<Dot> = Lane::new(1);
        assert!(lane.push(Dot(1)).is_ok());
        assert!(matches!(lane.push(Dot(2)), Err(Rejected::Busy)));
        assert_eq!(lane.pop().map(|Dot(n)| n), Some(1));

        lane.close();
        assert!(matches!(lane.push(Dot(3)), Err(Rejected::Disconnected)));
        assert!(
            lane.pop().is_none(),
            "a closed empty lane has nothing left to pop"
        );
    }

    #[test]
    fn a_full_results_lane_blocks_a_delivery_until_the_ui_drains() {
        let lane = Arc::new(Lane::new(2));
        lane.push(Dot(1)).unwrap();
        lane.push(Dot(2)).unwrap();
        let background = {
            let lane = Arc::clone(&lane);
            std::thread::spawn(move || {
                // A worker thread with a result and a full results lane waits: it does not
                // drop the result and does not pile results up.
                assert!(lane.deliver(Dot(3)).is_ok());
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !background.is_finished(),
            "the delivery is waiting, not queued beyond the cap"
        );
        lane.try_pop();
        lane.try_pop();
        background.join().unwrap();
        assert_eq!(lane.try_pop().map(|Dot(n)| n), Some(3));
    }

    #[test]
    fn closing_discards_queued_jobs_and_unblocks_reader_and_writer() {
        // A blocked writer wakes with an error: its result is not kept.
        let full = Arc::new(Lane::new(1));
        full.push(Dot(1)).unwrap();
        let writer = {
            let lane = Arc::clone(&full);
            std::thread::spawn(move || {
                assert!(lane.deliver(Dot(2)).is_err(), "no result after shutdown");
            })
        };
        let empty: Arc<Lane<Dot>> = Arc::new(Lane::new(1));
        let waiting_reader = {
            let lane = Arc::clone(&empty);
            std::thread::spawn(move || lane.pop())
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(!writer.is_finished(), "the writer is blocked, not queued");
        full.close();
        empty.close();
        writer.join().unwrap();
        assert!(
            waiting_reader.join().unwrap().is_none(),
            "an empty closed lane ends the reader"
        );
        assert!(
            full.try_pop().is_none(),
            "the queued job was discarded at close"
        );
    }
}
