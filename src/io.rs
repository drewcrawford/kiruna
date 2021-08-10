/*! Provides utilities for IO.

In Kiruna, io operations are associated with a *source* (like a file descriptor), and a *personality*.  Some sources
may force use of a particular personality, on the other hand other sources may work with one of many personalities.
In that case, you must choose a personality based on your desired requirements and usecase.

Personalities include:
* [stream], a personality for streaming io, like networks and pipes
* seek, a personality for seekable io, like files.  (**Unimplemented.**)  This generally provides more performance
  and flexilbility than [stream], although it requires more features in the underlying source.

|              | stream                  | seek               |
|--------------|-------------------------|--------------------|
| Read pattern | in order                | in order or random |
| Total size   | unknown (may be hinted) | known              |

Note that, files do not cleanly map to seek, nor do network requests cleanly map to [stream].  That is,

1.  Files like `/dev/urandom` are not really seekable and are more appropriately modeled as [stream].
2.  HTTP Range Requests may allow a seek operation and are more appropriately modeled as seek.

*/
#[cfg(feature="io_stream")]
pub mod stream;