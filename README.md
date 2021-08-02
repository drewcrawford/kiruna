# Kiruna
Kiruna is a simple async runtime in a few hundred lines of code.  It's designed to be readable and small while still providing great performance.  

Kiruna is also a remote town in the arctic circle.  Programs using it will be cold, beautiful, and isolated from more popular async runtimes.

# Executor

Currently Kiruna provides one local executor, `sync::Executor`.  This executor runs on a single thread, although it can poll operations that run concurrently.  This is a typical pattern for async code, and in many workloads is the fastest architecture.

In the future, other executors may be implemented, probably behind feature flags.