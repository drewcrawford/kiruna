/*although this crate is only useful on windows, if you use the `--all` flag on another platform you will build it
maybe this is not what we want */
#[cfg(target_os = "windows")]
windows::include_bindings!();
