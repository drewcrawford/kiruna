# Kiruna
Kiruna is a simple async runtime in a few hundred lines of code.  It's designed to be readable and small while still providing great performance.  

Kiruna is also a remote town in the arctic circle.  Programs using it will be cold, beautiful, and isolated from more popular async runtimes.

# Features
By default, Kira does nothing.  To use stuff, enable specific features:
* `sync`: An executor for a single thread
* `react`: A reactor.  This requires one or more backends,
    * `react-kqueue`, a kqueue backend


