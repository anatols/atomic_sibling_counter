# Atomic Sibling Counter

This crate provides an implementation of a shared counter that allows parallel threads/tasks to check how many 'siblings' they have. The counter is atomic and threads/tasks are not synchronized or coupled in any way.

A practical example of where such counter would be useful is a service-global rate limiter. Imagine a service that processes requests by spawning handler tasks (e.g. a new handler task per HTTP request). Inside the task handler you might want to know how many of the handlers are running in parallel ('siblings'), and throttle or fail the processing. In such example, you normally don't want the tasks to have any synchronization points or contention, and you don't want to explicitly deal with the lifetime of the counter itself.

With this crate, a sibling is marked by making it retain a [`SiblingToken`]. The token can be retained, for example, by a thread handler, an async task, or even just in some container. Calling [`sibling_count()`](SiblingToken::sibling_count()) on the token gives you the total number of siblings. Dropping the token decrements the counter.

You can have an outside view on your siblings by holding on to a [`SiblingCounter`] that has a [`sibling_count()`](SiblingCounter::sibling_count()) method too. Instances of [`SiblingCounter`] are *not* counted as siblings, but can be used to mark new siblings. Sibling counter instances are clonable, and all clones refer to the same underlying counter.

You can mark new siblings (issue new sibling tokens) by either cloning an existing token, or by calling [`add_sibling()`](SiblingCounter::add_sibling()) on a counter.

The underlying counter stays alive as long as at least one referring [`SiblingToken`] or [`SiblingCounter`] instance is alive.

# Example

```rust
use atomic_sibling_counter::SiblingCounter;
use std::time::Duration;

let counter = SiblingCounter::new();

for _ in 0..10 {
    let token = counter.add_sibling(); // new sibling token, to be moved into a spawned thread
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1)); // let all threads start

        // each thread now knows how many siblings it has
        assert_eq!(token.sibling_count(), 10);

        std::thread::sleep(Duration::from_secs(1)); // stay alive for a bit
    });
}

// from the main thread, we can check how many sibling threads there are
assert_eq!(counter.sibling_count(), 10);

std::thread::sleep(Duration::from_secs(5)); // let all threads finish
assert_eq!(counter.sibling_count(), 0);
```

# License

Licensed under either of Apache License, Version
2.0 or MIT license at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
