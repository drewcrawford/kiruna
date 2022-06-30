/*! Provides utilities for IO.

In Kiruna, io operations are associated with a *source* (like a file descriptor), and a *personality*.  Some sources
may force use of a particular personality, on the other hand other sources may work with one of many personalities.
In that case, you must choose a personality based on your desired requirements and usecase.

Personalities include:
* [stream], a personality for streaming io, like networks and pipes
* [seek], a personality for seekable io, like files.  (**Partially-implemented.**)  This generally provides more performance
  and flexilbility than [stream], although it requires more features in the underlying source.

|              | stream                  | seek               |
|--------------|-------------------------|--------------------|
| Read pattern | in order                | in order or random |
| Total size   | unknown (may be hinted) | known              |

Note that, files do not cleanly map to seek, nor do network requests cleanly map to [stream].  That is,

1.  Files like `/dev/urandom` are not really seekable and are more appropriately modeled as [stream].
2.  HTTP Range Requests may allow a seek operation and are more appropriately modeled as seek.

# Memory mapping

You should also consider memory mapping instead of any async IO personality.  Generally, memory-mapping is preferable in situations where

1.  The number of syscalls is thought to be high (e.g., many reads).  memory-mapping can perform all reads in a fixed number of syscalls.
    Note that reading the entire file is generally a single (or small number) of syscalls, so it isn't based on the size of the data.
2.  The memory lifetime is thought to be long.  This is because, usually, memory-mapped data does not require actual allocation
    in common situations, and all pages are clean, so they can be paged out in memory-pressure situations.  Reads, on the other
    hand, generally read into dirty pages.
*/
#[cfg(feature="io_stream")]
pub use ::stream as stream;

#[cfg(feature="io_seek")]
pub use ::seek as seek;