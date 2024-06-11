use atomic_sibling_counter::{SiblingCounter, SiblingToken};
use cap::Cap;
use std::{
    alloc,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        mpsc, Arc,
    },
};

#[global_allocator]
static ALLOCATOR: Cap<alloc::System> = Cap::new(alloc::System, usize::max_value());

#[test]
fn basic_counting() {
    let counter = SiblingCounter::new();
    assert_eq!(counter.sibling_count(), 0);

    let counter2 = counter.clone();
    assert_eq!(counter.sibling_count(), 0);
    assert_eq!(counter2.sibling_count(), 0);

    let sibling1 = counter.add_sibling();
    assert_eq!(counter.sibling_count(), 1);
    assert_eq!(counter2.sibling_count(), 1);
    assert_eq!(sibling1.sibling_count(), 1);

    let sibling2 = sibling1.add_sibling();
    assert_eq!(counter.sibling_count(), 2);
    assert_eq!(counter2.sibling_count(), 2);
    assert_eq!(sibling1.sibling_count(), 2);
    assert_eq!(sibling2.sibling_count(), 2);

    let sibling3 = sibling2.clone();
    assert_eq!(counter.sibling_count(), 3);
    assert_eq!(counter2.sibling_count(), 3);
    assert_eq!(sibling1.sibling_count(), 3);
    assert_eq!(sibling2.sibling_count(), 3);
    assert_eq!(sibling3.sibling_count(), 3);

    drop(sibling3);
    assert_eq!(counter.sibling_count(), 2);
    assert_eq!(counter2.sibling_count(), 2);
    assert_eq!(sibling1.sibling_count(), 2);
    assert_eq!(sibling2.sibling_count(), 2);

    drop(sibling1);
    assert_eq!(counter.sibling_count(), 1);
    assert_eq!(counter2.sibling_count(), 1);
    assert_eq!(sibling2.sibling_count(), 1);

    drop(sibling2);
    assert_eq!(counter.sibling_count(), 0);
    assert_eq!(counter2.sibling_count(), 0);

    let sibling4 = counter2.add_sibling();
    assert_eq!(counter.sibling_count(), 1);

    drop(counter2);
    drop(counter);
    assert_eq!(sibling4.sibling_count(), 1);

    let counter3 = sibling4.counter();
    assert_eq!(counter3.sibling_count(), 1);
    assert_eq!(sibling4.sibling_count(), 1);
}

#[test]
fn allocating_just_one_atomic() {
    let initial = ALLOCATOR.allocated();
    let with_counter = initial + std::mem::size_of::<AtomicU64>();

    {
        // Start with a counter
        let counter = SiblingCounter::new();
        assert_eq!(ALLOCATOR.allocated(), with_counter);

        let token = counter.add_sibling();
        assert_eq!(ALLOCATOR.allocated(), with_counter);

        drop(token);
        assert_eq!(ALLOCATOR.allocated(), with_counter);

        drop(counter);
        assert_eq!(ALLOCATOR.allocated(), initial);
    }

    {
        // Start with a token
        let token = SiblingToken::new();
        assert_eq!(ALLOCATOR.allocated(), with_counter);

        let counter = token.counter();
        assert_eq!(ALLOCATOR.allocated(), with_counter);

        drop(counter);
        assert_eq!(ALLOCATOR.allocated(), with_counter);

        drop(token);
        assert_eq!(ALLOCATOR.allocated(), initial);
    }
}

#[test]
fn parallel_threads() {
    const NUM_THREADS: usize = 512;

    let counter = SiblingCounter::new();
    let (sender, receiver) = mpsc::channel::<usize>();
    let mut join_handles = Vec::with_capacity(NUM_THREADS);
    let current_thread_steps: Vec<_> = (0..NUM_THREADS)
        .map(|_| Arc::new(AtomicU8::new(0)))
        .collect();

    #[allow(clippy::needless_range_loop)]
    for i in 0..NUM_THREADS {
        let token = counter.add_sibling();
        let sender = sender.clone();
        let current_step = current_thread_steps[i].clone();
        join_handles.push(Some(std::thread::spawn(move || {
            let wait_for_step = |step| {
                while current_step.load(Ordering::Acquire) != step {
                    std::thread::yield_now();
                }
            };

            sender.send(token.sibling_count()).unwrap();

            wait_for_step(1);
            sender.send(token.sibling_count()).unwrap();

            wait_for_step(2);
            sender.send(token.sibling_count()).unwrap();
        })));
    }

    // Step 0, just wait for all threads to start up and report
    for _ in 0..NUM_THREADS {
        let _ = receiver.recv().unwrap(); // ignore reported count since threads start unpredictably
    }

    // At this point all threads have reported and are waiting for the next step
    assert_eq!(counter.sibling_count(), NUM_THREADS);

    // Step 1, make all threads report again, each should now see all siblings
    current_thread_steps
        .iter()
        .for_each(|step| step.store(1, Ordering::Relaxed));
    for _ in 0..NUM_THREADS {
        let count = receiver.recv().unwrap();
        assert_eq!(count, NUM_THREADS)
    }

    // Step 2a, progress some threads one by one to first report, then finish.
    for i in 0..NUM_THREADS / 2 {
        current_thread_steps[i].store(2, Ordering::Relaxed);
        let count = receiver.recv().unwrap();
        assert_eq!(count, NUM_THREADS - i); // reported from thread before it finished
        join_handles[i].take().unwrap().join().unwrap();
        assert_eq!(counter.sibling_count(), NUM_THREADS - i - 1); // after the thread is done (the token was dropped)
    }
    assert_eq!(counter.sibling_count(), NUM_THREADS / 2);

    // Step 2b, progress remaining threads all at once to first report, then finish.
    #[allow(clippy::needless_range_loop)]
    for i in NUM_THREADS / 2..NUM_THREADS {
        current_thread_steps[i].store(2, Ordering::Relaxed);
    }
    #[allow(clippy::needless_range_loop)]
    for i in NUM_THREADS / 2..NUM_THREADS {
        join_handles[i].take().unwrap().join().unwrap();
    }
    #[allow(clippy::needless_range_loop)]
    for _ in NUM_THREADS / 2..NUM_THREADS {
        let count = receiver.recv().unwrap();
        assert!(count <= NUM_THREADS / 2);
    }

    assert_eq!(counter.sibling_count(), 0);
}
