Kiruna is an experimental, tiny, fast, and simple async executor and IO library.  Kiruna is an answer to these questions:

1.  ~~What if Rust had *EVEN MOAR* async runtimes than it does already?~~
2.  What's missing for writing interactive desktop/mobile applications?
3.  What if you 'stepped into' some Future implementation and found 1-2 small files you could read, understand, debug, and optimize?
4.  What if we just call some API from the operating system?
5.  What if users provided a bit more information that could fix performance issues before they happen?
6.  ~~What if you are a hipster and too cool to use tokio?~~

Kiruna is also a remote town in the arctic circle.  Programs that use it will be cold, beautiful, and isolated from more popular async runtimes.

# Practicalities

* Kiruna is an experimental/research quality.  But I am depending on it from real projects so I don't intend it to be a toy library.
* Many features are missing, partly implemented or still being designed.  Some APIs will change.
* macOS support is decent.  Windows is early but passing most tests.  Linux is planned but not implemented, iOS might happen if I end up needing it
* Free for noncommerial or 'small commercial' use, commercial licensing is available.

# Kiruna manifesto

## Interactive apps

Runtimes like tokio and even most alternatives like smol are thinking about servers.  Their assumption is you have a bajillion requests, and the more requests per second
the better they are.

In an interactive application, you have 15 tasks, perhaps 2 of which are realtime.  Success means "not interrupting the realtime tasks",
which might be *delaying* outstanding work.  For that reason, many async libraries that exist are at loggerheads with application use.

There are some cool solutions to this problem like [thin_main_loop](https://github.com/diwic/thin_main_loop).  However, they tend to view
GUI problems as a separate "thing".  To some extent they are, to some extent though it creates distance with more 'mainline' async Rust.

The whole problem is resolved by assigning every task a *priority*.  Tasks with the same priority target maximum throughput,
so that's the same as in serverland.  Tasks with different priorities target not interrupting a higher-priority task.

Of course, the key difference of this design is it requires the application developer to specify the priority of the task.  This
implicitly means that any use of Kiruna expects more options and configuration than other async runtimes.

## More information

This illustrates an important principle of Kiruna's design, which is 'ask the developer what they want to do'.  Reading a file
isn't just a question of file descriptors, it is a question of semantics and intent, because opening a document and caching data
should have different task priorities.

Another dimension of difference is in types of files.  If you are reading from a socket, there is nothing for it
besides copying all the data into memory which has such complications as residency, buffer sizes, and deallocations that
ultimately someone has to manage.  Whereas with an ordinary file, any modern OS can [memory-map](https://en.wikipedia.org/wiki/Memory-mapped_I/O) that file
for reading which is usually way cheaper and simpler.

But even though the application programmer knows whether they're reading from a file or a socket, many async IO
libraries do not.  If they assume the common denominator, the missed optimizations contribute to the common
observation that using async might make code *slower*. Alternatively, if libraries perform checks or heuristics,
there's also a runtime cost for that, and the increased complexity of maintaining that code.  And if the application
works with an owned buffer, the optimization might be moot entirely.

In Kiruna, IO operations have 'personalities' that distinguish various cases with a zero-cost abstraction and
helps the application developer understand what is going on, and will naturally choose the right API.  It's a bit like
[C-CALLER-CONTROL](https://rust-lang.github.io/api-guidelines/flexibility.html#c-caller-control) principle
of making functions that need owned data have owned data parameters.  It makes the performance easier to reason about,
at the cost of some additional API.

The tradeoff is you have to tell Kiruna more information just to 'read a file' than you
would in another library.

## Small implementations

Async implementations can be quite complicated, and many libraries create shared, reusable components to manage this complexity.
For example, they may introduce a "reactor", and this reactor may have various "backends" that vary by platform.  While
these designs improve consistency internally in large codebases like tokio, these concepts are distant from 'reading a file' which is allegedly
why we entered the async tarpit to begin with.  The result is that opening the hood is much harder for application developers
than it could be.

In Kiruna, the "async read function on macOS" *looks* like it does that. Here it is:

```rust
    ///Performs a single read.
    ///
    /// In practice, this function reads 0 bytes if the stream is closed.
    fn once<'a, O: Into<OSReadOptions<'a>>>(&self, os_read_options: O) -> impl Future<Output=Result<Managed,OSError>> {
        let (continuation, completion) = blocksr::continuation::Continuation::<(),_>::new();
        read_completion(dispatch_fd_t::new(self.fd), usize::MAX, os_read_options.into().queue, |data,err| {
            if err==0 {
                completion.complete(Ok(Managed::retain(data)))
            }
            else {
                completion.complete(Err(OSError(err)))
            }
        });
        continuation
    }
```

Admittedly there are a few more `<>` in there than I'd like, but you don't need a degree in reactorology to profile this code or
set a breakpoint.  You might compare this to some [similar implementation in tokio](https://github.com/tokio-rs/tokio/blob/1073f6e8be93837803704e770c7c54ddb4dcde27/tokio-util/src/lib.rs#L106)
(at least, I think they're similar, but I'm not certain.) I don't mean tokio code is bad in any way, I just mean it's not obvious
what it does exactly, which is not what I want when debugging my application.

The design principle in Kiruna is that we start with the problem, which is reading a file, and then "*just write code that does that*".

If it's that easy, you might well ask why other libraries don't have that design.  The tradeoff is that there are a lot of tiny functions in Kiruna
and they lack consistency in their implementations.  Personally I think this is a feature, since memory-mapped IO is
different than other kinds and it deserves a different implementation.  The result is you might encounter
many more implementation differences, particularly across different OS.  Please file bugs.

In limited cases, such as for read/write on the same OS and personality, Kiruna has some code reuse, but it is quite
narrow in scope and only has to worry about a very small set of situations within a single platform so it's easy
to jump into and see what it does.

## OS leverage

When you have a "big reactor" design, porting your library to an OS becomes all about porting the reactor, which ultimately calls
some very low-level kernel call regardless of read/write, file/socket, etc.
For example, [kqueue](https://en.wikipedia.org/wiki/Kqueue) or [epoll](https://man7.org/linux/man-pages/man7/epoll.7.html).

However, kqueue is [not anywhere close to the preferred way to do IO on macOS](https://news.ycombinator.com/item?id=12687257), that is [dispatch_read](https://developer.apple.com/documentation/dispatch/1388933-dispatch_read).
Similarly, `epoll` is not the preferred way
to do IO on linux, that is [io_uring](https://kernel.dk/io_uring.pdf).  Because these APIs are all very different from each other,
and the reactor should be the same everywhere, it's quite difficult for reactors to adopt them.

However, because Kiruna is built around many little functions that "just read a file on macOS", they can "just" call the OS's preferred
API to read files on macOS.  In fact, one function can call `kqueue` and another might call `dispatch`, which is great for experimentation,
profiling, debugging, and incrementally adopting OS innovation.

## Cargo features

Kiruna is "batteries included but off by default".  When you build kiruna you build basically nothing,
to do something you need to turn it on.  Table of features:

* `io_stream` implements the [io::stream] personality (only one that's implemented so far)
* `sync` implements the default single-threaded [Executor].  Note that individual operations generally have some 'other' way to
  do concurrency, so this is the right default for small to medium workloads.
* `test` is the [world's smallest async 'runtime'](https://github.com/drewcrawford/kiruna/blob/f516f2ad8f493577b0fd2a6f2feef8bde35a8a30/src/test.rs#L23), which polls your futures in a busyloop.
  This is very silly but surprisingly great for use in tests.  It compiles quickly, has no startup time and is badly-behaved generally,
  which flushes out unusual bugs in a unit test.
* `join` implements some join functions like in the [futures](https://crates.io/crates/futures) crate.  But they compile faster
  and support other behaviors like 'fail fast' that are interesting to applications
* `futures` is a grab bag of convenient APIs for futures.
* `all` is all the stuff we can do on your platform

Coming someday:
* Additional runtimes, such as multithreaded
* Additional personalities, like files or sockets
* Additional priorities

Out of scope:
* Lesser-used IO, like multiprocess communication.  Stay tuned for future crates!
* High-level IO, like TLS or HTTP

