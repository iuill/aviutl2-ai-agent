use std::{
    sync::{Mutex, TryLockError},
    thread,
    time::{Duration, Instant},
};

const GATE_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EditorError {
    #[error("EditorGate is busy")]
    Busy,
    #[error("AviUtl2 did not accept the read")]
    Unavailable,
}

pub struct EditorGate {
    lock: Mutex<()>,
    timeout: Duration,
}

impl EditorGate {
    pub fn new(timeout: Duration) -> Self {
        Self {
            lock: Mutex::new(()),
            timeout,
        }
    }

    pub fn read<T>(
        &self,
        operation: impl FnOnce() -> Result<T, EditorError>,
    ) -> Result<T, EditorError> {
        let deadline = Instant::now() + self.timeout;
        loop {
            match self.lock.try_lock() {
                Ok(_guard) => return operation(),
                Err(TryLockError::Poisoned(error)) => {
                    // The mutex protects no data; poisoning only records that a
                    // previous SDK operation panicked. Recover the serialization
                    // token so later reads are not disabled until process restart.
                    let _guard = error.into_inner();
                    self.lock.clear_poison();
                    return operation();
                }
                Err(TryLockError::WouldBlock) if Instant::now() >= deadline => {
                    return Err(EditorError::Busy);
                }
                Err(TryLockError::WouldBlock) => thread::sleep(GATE_POLL_INTERVAL),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::{Duration, Instant},
    };

    use super::{EditorError, EditorGate};

    #[test]
    fn serializes_operations() {
        let gate = Arc::new(EditorGate::new(Duration::from_secs(1)));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let holder = {
            let gate = Arc::clone(&gate);
            thread::spawn(move || {
                gate.read(|| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .unwrap();
            })
        };
        entered_rx.recv().unwrap();

        let second_entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let waiter = {
            let gate = Arc::clone(&gate);
            let second_entered = Arc::clone(&second_entered);
            thread::spawn(move || {
                gate.read(|| {
                    second_entered.store(true, std::sync::atomic::Ordering::Release);
                    Ok(())
                })
                .unwrap();
            })
        };
        thread::sleep(Duration::from_millis(10));
        assert!(!second_entered.load(std::sync::atomic::Ordering::Acquire));

        release_tx.send(()).unwrap();
        holder.join().unwrap();
        waiter.join().unwrap();
        assert!(second_entered.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn returns_busy_after_the_deadline() {
        let gate = Arc::new(EditorGate::new(Duration::from_millis(20)));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let holder = {
            let gate = Arc::clone(&gate);
            thread::spawn(move || {
                gate.read(|| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(())
                })
                .unwrap();
            })
        };
        entered_rx.recv().unwrap();

        let started = Instant::now();
        assert_eq!(gate.read(|| Ok(())), Err(EditorError::Busy));
        assert!(started.elapsed() >= Duration::from_millis(20));

        release_tx.send(()).unwrap();
        holder.join().unwrap();
    }

    #[test]
    fn recovers_after_an_operation_panics() {
        let gate = Arc::new(EditorGate::new(Duration::from_millis(20)));
        let panicking_gate = Arc::clone(&gate);
        assert!(
            thread::spawn(move || {
                let _ = panicking_gate.read::<()>(|| panic!("injected SDK panic"));
            })
            .join()
            .is_err()
        );

        assert_eq!(gate.read(|| Ok("recovered")), Ok("recovered"));
    }
}
